//! Indicator settings modal renderer.
//!
//! Renders the modal dialog that allows users to configure indicator parameters,
//! styles (output colors), timeframe visibility, signal detection, and view
//! indicator metadata.

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::{IndicatorSettingsModalResult, SliderTrackInfo};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::modal_settings::{IndicatorSettingsState, IndicatorSettingsTab, IndicatorDisplayInfo, IndicatorParamType, DualSliderHandle};
use crate::ui::widgets::{
    draw_input, draw_input_cursor, InputConfig, InputType,
    render_dual_slider, SliderConfig, SliderEditingInfo,
    render_modal_frame_only, ModalTheme,
    WidgetState, WidgetTheme,
};
use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig};
use crate::ui::dropdown::{render_dropdown, DropdownConfig, DropdownItem, DropdownTheme};
use crate::ui::Icon;
use crate::ui::z_order::ZLayer;
use crate::drawing::TimeframeVisibilityConfig;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::indicator_source::{SignalDisplayConfig, SignalShape};
use uzor::types::Rect as WidgetRect;
use uzor::render::{TextAlign, TextBaseline};

/// Render the indicator settings modal.
///
/// # Arguments
///
/// * `ctx` - Render context for drawing
/// * `screen_w` - Total screen width (used for default centering)
/// * `screen_h` - Total screen height (used for default centering)
/// * `chart_x` - Left edge of the chart content area (right of left toolbar)
/// * `chart_y` - Top edge of the chart content area (below top toolbar)
/// * `indicator_state` - Current state of the indicator settings modal
/// * `indicator_name` - Name of the indicator being edited
/// * `params` - Indicator parameters as key-value pairs
/// * `outputs` - Indicator outputs with colors
/// * `definition` - Optional indicator display info for metadata
/// * `signals_enabled` - Whether signals are currently enabled
/// * `signal_display` - Visual configuration for signal markers (shape, colors, size, offset)
/// * `timeframe_visibility` - Optional timeframe visibility config
/// * `current_time_ms` - Current time in milliseconds (for cursor blink)
/// * `frame_theme` - Frame theme
/// * `toolbar_theme` - Toolbar theme
/// * `input_coordinator` - Input coordinator for registering hit zones
pub fn render_indicator_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    chart_x: f64,
    chart_y: f64,
    indicator_state: &IndicatorSettingsState,
    indicator_name: &str,
    params: &[(String, String)],
    outputs: &[(String, String)],
    definition: Option<&IndicatorDisplayInfo>,
    signals_enabled: bool,
    signal_display: &SignalDisplayConfig,
    timeframe_visibility: Option<&TimeframeVisibilityConfig>,
    current_time_ms: u64,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    templates: &[crate::templates::IndicatorTemplate],
) -> IndicatorSettingsModalResult {
    let mut result = IndicatorSettingsModalResult {
        signals_enabled,
        ..IndicatorSettingsModalResult::default()
    };

    // Modal dimensions
    let modal_width = 500.0;
    let modal_height = 420.0;
    let modal_x = if let Some((px, _py)) = indicator_state.position {
        px.max(0.0).min(screen_w - modal_width)
    } else {
        chart_x + (screen_w - modal_width) / 2.0
    };
    let modal_y = if let Some((_, py)) = indicator_state.position {
        py.max(0.0).min(screen_h - modal_height)
    } else {
        chart_y + (screen_h - modal_height) / 2.0
    };

    // Layout constants
    let header_height = 44.0;
    let sidebar_width = 48.0;
    let footer_height = 52.0; // single row: Шаблон left, Отмена + OK right
    let content_width = modal_width - sidebar_width;
    let content_height = modal_height - header_height - footer_height;

    // Store modal rect for hit testing (MUST be set before rendering)
    result.modal_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);
    result.header_rect = WidgetRect::new(modal_x, modal_y, modal_width, header_height);

    // Push input layer for this modal
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "indicator_settings");

    // Register modal background catch-all
    input_coordinator.register_on_layer(
        "ind_settings:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_width, modal_height),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Render modal frame
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, result.modal_rect, &modal_theme, 0.0);

    // =========================================================================
    // Header
    // =========================================================================

    // Title (indicator name)
    let text_color = &toolbar_theme.item_text;
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(indicator_name, modal_x + 16.0, modal_y + header_height / 2.0);

    // Close button
    let close_size = 18.0;
    let close_x = modal_x + modal_width - close_size - 12.0;
    let close_y = modal_y + (header_height - close_size) / 2.0;
    result.close_btn_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, text_color);

    // Register close button
    input_coordinator.register_on_layer(
        "ind_settings:close",
        uzor::types::Rect::new(close_x, close_y, close_size, close_size),
        uzor::input::sense::Sense::CLICK,
        &layer_id,
    );

    // Header bottom border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_height);
    ctx.line_to(modal_x + modal_width, modal_y + header_height);
    ctx.stroke();

    // =========================================================================
    // Left sidebar (vertical tabs)
    // =========================================================================
    let sidebar_x = modal_x;
    let sidebar_y = modal_y + header_height;

    // Sidebar right border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(sidebar_x + sidebar_width, sidebar_y);
    ctx.line_to(sidebar_x + sidebar_width, sidebar_y + content_height);
    ctx.stroke();

    // Tab icons
    let tab_icon_size = 20.0;
    let tab_button_height = 44.0;
    let active_tab_idx = indicator_state.active_tab.index();

    for (i, tab) in IndicatorSettingsTab::all().iter().enumerate() {
        let tab_y = sidebar_y + i as f64 * tab_button_height;
        let is_active = i == active_tab_idx;

        // Active indicator (left border + background)
        if is_active {
            ctx.draw_sidebar_active_item(
                sidebar_x, tab_y, sidebar_width, tab_button_height,
                &toolbar_theme.accent, &toolbar_theme.item_bg_active, 3.0
            );
        }

        // Icon
        let icon_x = sidebar_x + (sidebar_width - tab_icon_size) / 2.0;
        let icon_y = tab_y + (tab_button_height - tab_icon_size) / 2.0;
        let icon_color = if is_active { &toolbar_theme.item_text_active } else { text_color };

        let icon = match *tab {
            IndicatorSettingsTab::Inputs => Icon::Settings,
            IndicatorSettingsTab::Style => Icon::Palette,
            IndicatorSettingsTab::Visibility => Icon::Eye,
            IndicatorSettingsTab::Signals => Icon::Alert,
            IndicatorSettingsTab::Info => Icon::Info,
        };
        draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, tab_icon_size, tab_icon_size, icon_color);

        // Register tab with coordinator
        input_coordinator.register_on_layer(
            format!("ind_settings:tab:{}", tab.id()),
            uzor::types::Rect::new(sidebar_x, tab_y, sidebar_width, tab_button_height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // =========================================================================
    // Content area
    // =========================================================================
    let content_x = modal_x + sidebar_width;
    let content_y = modal_y + header_height;
    let content_padding = 20.0;

    // Content background
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(content_x, content_y, content_width, content_height);

    // Tab title
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(indicator_state.active_tab.label(), content_x + content_padding, content_y + content_padding);

    // Content based on active tab
    let settings_y = content_y + content_padding + 30.0;
    let row_height = 36.0;

    // Shared viewport dimensions for all tabs
    let tab_viewport_height = content_height - 30.0 - content_padding;
    let tab_viewport_rect = WidgetRect::new(content_x, settings_y, content_width, tab_viewport_height);
    let tab_scroll_config = ScrollableConfig {
        scrollbar_width: 8.0,
        scrollbar_padding: 4.0,
        always_show_scrollbar: false,
    };

    match indicator_state.active_tab {
        IndicatorSettingsTab::Inputs => {
            // Calculate total content height for scroll
            let total_content_height = if params.is_empty() {
                row_height
            } else {
                params.len() as f64 * row_height
            };
            result.total_content_height = total_content_height;
            result.viewport_height = tab_viewport_height;

            let scrollable = ScrollableContainer::new(tab_viewport_rect, &indicator_state.scroll, Some(tab_scroll_config));
            scrollable.begin(ctx);

            ctx.set_font("13px sans-serif");
            let mut row_y = scrollable.content_y();

            for (param_name, param_value) in params.iter() {
                // Get param type from definition
                let param_type = definition.as_ref()
                    .and_then(|d| d.params.iter().find(|p| &p.name == param_name))
                    .map(|p| &p.param_type);

                // Check if editing this param
                let field_id = format!("indicator_param:{}", param_name);
                let is_editing = indicator_state.editing_text_state
                    .as_ref()
                    .map(|e| e.field_id == field_id)
                    .unwrap_or(false)
                    || indicator_state.is_editing_field(param_name);
                let is_hovered = indicator_state.hovered_item_id.as_deref() == Some(param_name);

                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(param_name, content_x + content_padding, row_y + row_height / 2.0);

                // Input field position
                let input_x = content_x + content_padding + 150.0;
                let input_height = 24.0;
                let input_y = row_y + (row_height - input_height) / 2.0;

                // Render based on param type
                match param_type {
                    Some(IndicatorParamType::Bool) => {
                        // Render toggle switch for boolean
                        let is_on = param_value == "true";
                        let toggle_width = 44.0;
                        let toggle_height = 22.0;
                        let toggle_y = row_y + (row_height - toggle_height) / 2.0;

                        // Toggle track (pill/capsule shape)
                        let toggle_bg = if is_on { &toolbar_theme.accent } else { &toolbar_theme.item_bg_hover };
                        ctx.set_fill_color(toggle_bg);
                        ctx.fill_rounded_rect(input_x, toggle_y, toggle_width, toggle_height, toggle_height / 2.0);

                        // Toggle knob (white ball)
                        let knob_radius = 8.0;
                        let knob_x = if is_on {
                            input_x + toggle_width - knob_radius - 4.0
                        } else {
                            input_x + knob_radius + 4.0
                        };
                        let knob_y = toggle_y + toggle_height / 2.0;
                        ctx.set_fill_color("#ffffff");
                        ctx.fill_rounded_rect(knob_x - knob_radius, knob_y - knob_radius, knob_radius * 2.0, knob_radius * 2.0, knob_radius);

                        // Store toggle rect for hit testing
                        let toggle_rect = WidgetRect::new(input_x, toggle_y, toggle_width, toggle_height);
                        result.content_items.push((format!("toggle:{}", param_name), toggle_rect));
                    }
                    Some(IndicatorParamType::Source) | Some(IndicatorParamType::Select { .. }) => {
                        // Render split dropdown for source/select
                        let dropdown_width = 80.0;
                        let chevron_width = 20.0;
                        let text_width = dropdown_width - chevron_width;
                        let dropdown_rect = WidgetRect::new(input_x, input_y, dropdown_width, input_height);

                        // Use centralized draw_input for the base
                        let dropdown_config = InputConfig::new(param_value)
                            .with_type(InputType::Text)
                            .with_font_size(13.0)
                            .with_padding(8.0)
                            .with_radius(0.0);

                        let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
                        let widget_state = if is_hovered {
                            WidgetState::Hovered
                        } else {
                            WidgetState::Normal
                        };
                        draw_input(ctx, &dropdown_config, widget_state, dropdown_rect, &widget_theme);

                        // Vertical separator line between text and chevron
                        ctx.set_stroke_color(&toolbar_theme.separator);
                        ctx.set_stroke_width(1.0);
                        ctx.begin_path();
                        ctx.move_to(input_x + text_width, input_y);
                        ctx.line_to(input_x + text_width, input_y + input_height);
                        ctx.stroke();

                        // Dropdown arrow (chevron)
                        let arrow_x = input_x + text_width + chevron_width / 2.0 - 3.0;
                        let arrow_y = row_y + row_height / 2.0;
                        ctx.set_fill_color(text_color);
                        ctx.begin_path();
                        ctx.move_to(arrow_x, arrow_y - 3.0);
                        ctx.line_to(arrow_x + 6.0, arrow_y - 3.0);
                        ctx.line_to(arrow_x + 3.0, arrow_y + 3.0);
                        ctx.close_path();
                        ctx.fill();

                        // Store TWO hit areas:
                        // 1. Left part (text area) -> cycle through options
                        let cycle_rect = WidgetRect::new(input_x, input_y, text_width, input_height);
                        result.content_items.push((format!("dropdown_cycle:{}", param_name), cycle_rect));

                        // 2. Right part (chevron area) -> open dropdown menu
                        let menu_rect = WidgetRect::new(input_x + text_width, input_y, chevron_width, input_height);
                        result.content_items.push((format!("dropdown_menu:{}", param_name), menu_rect));
                    }
                    _ => {
                        // Default: render text input for Int/Float
                        let input_width = 80.0;

                        // Get text and cursor from editing_text_state or legacy fields
                        let (display_text, cursor_pos, selection_start, selection_end) = if let Some(ref edit) = indicator_state.editing_text_state {
                            if edit.field_id == field_id {
                                (edit.text.as_str(), edit.cursor, edit.selection_start, Some(edit.cursor))
                            } else {
                                (param_value.as_str(), param_value.len(), None, None)
                            }
                        } else if is_editing {
                            // Legacy fallback
                            (indicator_state.editing_text.as_str(), indicator_state.editing_cursor, None, None)
                        } else {
                            (param_value.as_str(), param_value.len(), None, None)
                        };

                        let input_rect = WidgetRect::new(input_x, input_y, input_width, input_height);

                        // Draw input using centralized system
                        let param_input_config = InputConfig::new(display_text)
                            .with_focused(is_editing)
                            .with_type(InputType::Number)
                            .with_font_size(13.0)
                            .with_padding(8.0)
                            .with_radius(0.0)
                            .with_cursor(cursor_pos)
                            .with_selection(selection_start, selection_end);

                        let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
                        let widget_state = if is_hovered && !is_editing {
                            WidgetState::Hovered
                        } else {
                            WidgetState::Normal
                        };
                        let param_input_result = draw_input(ctx, &param_input_config, widget_state, input_rect, &widget_theme);

                        // Draw cursor when editing (with blink)
                        if is_editing {
                            let cursor_visible = indicator_state.editing_text_state
                                .as_ref()
                                .map(|e| e.is_cursor_visible(current_time_ms))
                                .unwrap_or(true);

                            if cursor_visible {
                                draw_input_cursor(
                                    ctx,
                                    param_input_result.cursor_x,
                                    param_input_result.cursor_y,
                                    param_input_result.cursor_height,
                                    text_color,
                                );
                            }
                            // Expose char positions for drag-to-select
                            result.active_input_char_positions = param_input_result.char_x_positions;
                            result.active_input_rect = Some(input_rect);
                        }

                        // Store input rect for hit testing
                        result.content_items.push((format!("input:{}", param_name), input_rect));
                    }
                }

                row_y += row_height;
            }

            if params.is_empty() {
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text("Нет настраиваемых параметров", content_x + content_padding, scrollable.content_y() + row_height / 2.0);
            }

            let widget_theme = WidgetTheme::default();
            let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
            result.scrollbar_handle_rect = scroll_result.handle_rect;
            result.scrollbar_track_rect = scroll_result.track_rect;
        }
        IndicatorSettingsTab::Style => {
            // Calculate total content height for scroll
            let total_content_height = if outputs.is_empty() {
                row_height
            } else {
                outputs.len() as f64 * row_height
            };
            result.total_content_height = total_content_height;
            result.viewport_height = tab_viewport_height;

            let scrollable = ScrollableContainer::new(tab_viewport_rect, &indicator_state.scroll, Some(tab_scroll_config));
            scrollable.begin(ctx);

            // Render output style settings (colors)
            ctx.set_font("13px sans-serif");
            let mut row_y = scrollable.content_y();
            let swatch_size = 20.0;

            for (output_name, output_color) in outputs.iter() {
                let is_hovered = indicator_state.hovered_item_id.as_deref() == Some(output_name.as_str());
                let is_color_picker_open = indicator_state.color_picker_field.as_deref() == Some(output_name.as_str());

                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(output_name, content_x + content_padding, row_y + row_height / 2.0);

                // Color swatch
                let swatch_x = content_x + content_padding + 150.0;
                let swatch_y = row_y + (row_height - swatch_size) / 2.0;

                // Hover effect
                if is_hovered || is_color_picker_open {
                    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                    ctx.fill_rect(swatch_x - 2.0, swatch_y - 2.0, swatch_size + 4.0, swatch_size + 4.0);
                }

                ctx.set_fill_color(output_color);
                ctx.fill_rect(swatch_x, swatch_y, swatch_size, swatch_size);

                // Border - accent when color picker open
                let border_color = if is_color_picker_open {
                    &toolbar_theme.accent
                } else {
                    &toolbar_theme.separator
                };
                ctx.set_stroke_color(border_color);
                ctx.set_stroke_width(if is_color_picker_open { 2.0 } else { 1.0 });
                ctx.stroke_rect(swatch_x, swatch_y, swatch_size, swatch_size);

                // Store swatch rect for hit testing
                let swatch_rect = WidgetRect::new(swatch_x, swatch_y, swatch_size, swatch_size);
                result.content_items.push((format!("color:{}", output_name), swatch_rect));

                row_y += row_height;
            }

            if outputs.is_empty() {
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text("Нет настраиваемых выходов", content_x + content_padding, scrollable.content_y() + row_height / 2.0);
            }

            let widget_theme = WidgetTheme::default();
            let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
            result.scrollbar_handle_rect = scroll_result.handle_rect;
            result.scrollbar_track_rect = scroll_result.track_rect;
        }
        IndicatorSettingsTab::Visibility => {
            // Timeframe visibility settings
            let tf_config = timeframe_visibility.cloned()
                .unwrap_or_else(TimeframeVisibilityConfig::all);

            let timeframes: [(&str, bool, bool, Option<(u32, u32)>, u32, u32); 7] = [
                ("Тики", true, tf_config.ticks, None, 0, 0),
                ("Секунды", false, tf_config.seconds.is_some(), tf_config.seconds, 1, 59),
                ("Минуты", false, tf_config.minutes.is_some(), tf_config.minutes, 1, 59),
                ("Часы", false, tf_config.hours.is_some(), tf_config.hours, 1, 24),
                ("Дни", false, tf_config.days.is_some(), tf_config.days, 1, 366),
                ("Недели", false, tf_config.weeks.is_some(), tf_config.weeks, 1, 52),
                ("Месяцы", false, tf_config.months.is_some(), tf_config.months, 1, 12),
            ];

            let row_gap = 4.0;
            let total_content_height = timeframes.len() as f64 * (row_height + row_gap);
            result.total_content_height = total_content_height;
            result.viewport_height = tab_viewport_height;

            let scrollable = ScrollableContainer::new(tab_viewport_rect, &indicator_state.scroll, Some(tab_scroll_config));
            scrollable.begin(ctx);

            let checkbox_size = 16.0;
            let label_width = 70.0;
            let input_width = 32.0;
            let gap = 6.0;
            // Dynamic slider width: use remaining space after fixed elements (matches compare_settings formula)
            let available_w = content_width - content_padding;
            let slider_width = (available_w - checkbox_size - 6.0 - label_width - input_width * 2.0 - gap * 2.0).max(80.0);
            let content_left = content_x + content_padding;
            let mut row_y = scrollable.content_y();

            for (i, (label, is_bool_only, enabled, range, min_allowed, max_allowed)) in timeframes.iter().enumerate() {
                let check_x = content_left;
                let check_y = row_y + (row_height - checkbox_size) / 2.0;

                // Checkbox
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

                // Label
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, check_x + checkbox_size + 6.0, row_y + row_height / 2.0);

                // Checkbox hit area
                let checkbox_hit = WidgetRect::new(check_x, row_y, checkbox_size + 6.0 + label_width, row_height);
                result.content_items.push((format!("tf_{}_toggle", i), checkbox_hit));

                // If not bool-only and enabled, show min/max controls
                if !*is_bool_only && *enabled {
                    let controls_x = content_left + checkbox_size + 6.0 + label_width + gap;
                    let (current_min, current_max) = range.unwrap_or((*min_allowed, *max_allowed));

                    let slider_config = SliderConfig::new(*min_allowed as f64, *max_allowed as f64)
                        .with_step(1.0);

                    let total_width = input_width * 2.0 + slider_width + gap * 2.0;
                    let slider_rect = WidgetRect::new(controls_x, row_y, total_width, row_height);
                    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

                    let min_field_id = format!("tf_{}_min", i);
                    let max_field_id = format!("tf_{}_max", i);

                    let editing_min_info = indicator_state.editing_text_state.as_ref()
                        .filter(|e| e.field_id == min_field_id)
                        .map(|e| SliderEditingInfo {
                            text: &e.text,
                            cursor: e.cursor,
                            selection_start: e.selection_start,
                        });

                    let editing_max_info = indicator_state.editing_text_state.as_ref()
                        .filter(|e| e.field_id == max_field_id)
                        .map(|e| SliderEditingInfo {
                            text: &e.text,
                            cursor: e.cursor,
                            selection_start: e.selection_start,
                        });

                    let slider_field_id = format!("tf_{}_slider", i);
                    let hovered = indicator_state.slider_drag.as_ref()
                        .map(|d| d.field_id == slider_field_id)
                        .unwrap_or_else(|| indicator_state.hovered_item_id.as_deref() == Some(slider_field_id.as_str()));

                    // During drag, apply floating preview to the dragged handle.
                    let (display_min, display_max) = if let Some(ref drag) = indicator_state.slider_drag {
                        if drag.field_id == slider_field_id {
                            if let Some(float_val) = drag.floating_value {
                                let fv = float_val.round() as u32;
                                match drag.dual_handle {
                                    Some(DualSliderHandle::Min) => (fv.min(current_max), current_max),
                                    Some(DualSliderHandle::Max) => (current_min, fv.max(current_min)),
                                    None => (current_min, current_max),
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

                    // Register slider track for drag handling (use full_rect for better hit detection)
                    result.content_items.push((slider_field_id.clone(), slider_result.full_rect));

                    // Register input fields and populate active_input_rect/char_positions
                    if let Some(min_input_rect) = slider_result.min_input_rect {
                        result.content_items.push((min_field_id.clone(), min_input_rect));
                        if indicator_state.editing_text_state.as_ref()
                            .map(|e| e.field_id == min_field_id)
                            .unwrap_or(false)
                        {
                            result.active_input_rect = Some(min_input_rect);
                            if let Some(ref ir) = slider_result.min_input_result {
                                result.active_input_char_positions = ir.char_x_positions.clone();
                            }
                        }
                    }
                    if let Some(max_input_rect) = slider_result.max_input_rect {
                        result.content_items.push((max_field_id.clone(), max_input_rect));
                        if indicator_state.editing_text_state.as_ref()
                            .map(|e| e.field_id == max_field_id)
                            .unwrap_or(false)
                        {
                            result.active_input_rect = Some(max_input_rect);
                            if let Some(ref ir) = slider_result.max_input_result {
                                result.active_input_char_positions = ir.char_x_positions.clone();
                            }
                        }
                    }

                    // Add slider track info
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

            let widget_theme = WidgetTheme::default();
            let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
            result.scrollbar_handle_rect = scroll_result.handle_rect;
            result.scrollbar_track_rect = scroll_result.track_rect;
        }
        IndicatorSettingsTab::Signals => {
            // Calculate total content height for scroll
            let signals_on = result.signals_enabled;
            let total_content_height = if signals_on {
                // toggle row + gap + 2 desc lines + gap
                // + shape row + bullish color row + bearish color row
                // + size row + offset row
                let base = row_height + 10.0 + row_height * 0.6 + row_height + 12.0;
                let config_rows = row_height // shape selector
                    + row_height // bullish color
                    + row_height // bearish color
                    + row_height // size
                    + row_height; // offset
                base + config_rows
            } else {
                // toggle row + gap + 2 desc lines
                row_height + 10.0 + row_height * 0.6 + row_height
            };
            result.total_content_height = total_content_height;
            result.viewport_height = tab_viewport_height;

            let scrollable = ScrollableContainer::new(tab_viewport_rect, &indicator_state.scroll, Some(tab_scroll_config));
            scrollable.begin(ctx);

            // Signal detection settings
            ctx.set_font("13px sans-serif");
            let mut row_y = scrollable.content_y();

            // Enable/disable signals toggle
            ctx.set_fill_color(text_color);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Включить сигналы", content_x + content_padding, row_y + row_height / 2.0);

            // Toggle switch
            let toggle_x = content_x + content_padding + 180.0;
            let toggle_width = 44.0;
            let toggle_height = 22.0;
            let toggle_y_offset = row_y + (row_height - toggle_height) / 2.0;

            // Toggle track (pill/capsule shape)
            let track_color = if signals_on { &toolbar_theme.accent } else { &toolbar_theme.item_bg_hover };
            ctx.set_fill_color(track_color);
            ctx.fill_rounded_rect(toggle_x, toggle_y_offset, toggle_width, toggle_height, toggle_height / 2.0);

            // Toggle knob (white ball)
            let knob_radius = toggle_height / 2.0 - 2.0;
            let knob_center_x = if signals_on {
                toggle_x + toggle_width - knob_radius - 2.0
            } else {
                toggle_x + knob_radius + 2.0
            };
            let knob_center_y = toggle_y_offset + toggle_height / 2.0;
            ctx.set_fill_color("#ffffff");
            ctx.fill_rounded_rect(knob_center_x - knob_radius, knob_center_y - knob_radius, knob_radius * 2.0, knob_radius * 2.0, knob_radius);

            // Store toggle rect for hit testing
            result.signals_toggle_rect = Some(WidgetRect::new(toggle_x, toggle_y_offset, toggle_width, toggle_height));

            row_y += row_height + 10.0;

            // Description
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text("Автоматически определяет сигналы на основе", content_x + content_padding, row_y);
            row_y += row_height * 0.6;
            ctx.fill_text("значений индикатора (пересечения, уровни и т.д.)", content_x + content_padding, row_y);
            row_y += row_height;

            // Signal display configuration controls (shown only when signals are enabled)
            if signals_on {
                row_y += 12.0; // visual gap before config section

                // --- Shape selector ---
                ctx.set_fill_color(text_color);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Форма:", content_x + content_padding, row_y + row_height / 2.0);

                let shape_btn_size = 24.0;
                let shape_btn_gap = 6.0;
                let shapes_start_x = content_x + content_padding + 100.0;
                let shapes = [
                    (SignalShape::Arrow,    "ind_set:signal_shape:arrow"),
                    (SignalShape::Triangle, "ind_set:signal_shape:triangle"),
                    (SignalShape::Circle,   "ind_set:signal_shape:circle"),
                    (SignalShape::Diamond,  "ind_set:signal_shape:diamond"),
                ];

                for (idx, (shape, shape_id)) in shapes.iter().enumerate() {
                    let btn_x = shapes_start_x + idx as f64 * (shape_btn_size + shape_btn_gap);
                    let btn_y = row_y + (row_height - shape_btn_size) / 2.0;
                    let is_active = signal_display.shape == *shape;

                    // Button background
                    let btn_bg = if is_active {
                        &toolbar_theme.item_bg_active
                    } else {
                        &toolbar_theme.item_bg_hover
                    };
                    ctx.set_fill_color(btn_bg);
                    ctx.fill_rounded_rect(btn_x, btn_y, shape_btn_size, shape_btn_size, 4.0);

                    // Border — accent when active
                    let border_color = if is_active {
                        &toolbar_theme.accent
                    } else {
                        &toolbar_theme.separator
                    };
                    ctx.set_stroke_color(border_color);
                    ctx.set_stroke_width(if is_active { 2.0 } else { 1.0 });
                    ctx.stroke_rounded_rect(btn_x, btn_y, shape_btn_size, shape_btn_size, 4.0);

                    // Draw shape programmatically, centered in button
                    let icon_color = if is_active { &toolbar_theme.item_text_active } else { text_color };
                    let cx = btn_x + shape_btn_size / 2.0;
                    let cy = btn_y + shape_btn_size / 2.0;
                    match shape {
                        SignalShape::Arrow => {
                            // Draw a proper arrow with stem + head pointing up
                            let s = 5.0; // half-size
                            let stem_w = 1.2;
                            let head_h = s * 0.9;
                            let head_w = s * 0.9;
                            ctx.set_fill_color(icon_color);
                            ctx.begin_path();
                            ctx.move_to(cx, cy - s);                    // tip
                            ctx.line_to(cx + head_w, cy - s + head_h);  // head right
                            ctx.line_to(cx + stem_w, cy - s + head_h);  // stem top-right
                            ctx.line_to(cx + stem_w, cy + s);           // stem bottom-right
                            ctx.line_to(cx - stem_w, cy + s);           // stem bottom-left
                            ctx.line_to(cx - stem_w, cy - s + head_h);  // stem top-left
                            ctx.line_to(cx - head_w, cy - s + head_h);  // head left
                            ctx.close_path();
                            ctx.fill();
                        }
                        SignalShape::Triangle => {
                            ctx.set_fill_color(icon_color);
                            ctx.begin_path();
                            ctx.move_to(cx, cy - 5.0);
                            ctx.line_to(cx - 5.0, cy + 5.0);
                            ctx.line_to(cx + 5.0, cy + 5.0);
                            ctx.close_path();
                            ctx.fill();
                        }
                        SignalShape::Circle => {
                            ctx.set_fill_color(icon_color);
                            ctx.begin_path();
                            ctx.arc(cx, cy, 5.0, 0.0, std::f64::consts::TAU);
                            ctx.fill();
                        }
                        SignalShape::Diamond => {
                            ctx.set_fill_color(icon_color);
                            ctx.begin_path();
                            ctx.move_to(cx, cy - 6.0);
                            ctx.line_to(cx + 5.0, cy);
                            ctx.line_to(cx, cy + 6.0);
                            ctx.line_to(cx - 5.0, cy);
                            ctx.close_path();
                            ctx.fill();
                        }
                    }

                    // Store rect for hit testing
                    result.content_items.push((
                        (*shape_id).to_string(),
                        WidgetRect::new(btn_x, btn_y, shape_btn_size, shape_btn_size),
                    ));
                }

                // Restore font and alignment for subsequent rows
                ctx.set_font("13px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                row_y += row_height;

                // --- Bullish color swatch ---
                let swatch_size = 20.0;
                let swatch_col_x = content_x + content_padding + 120.0;
                let is_bullish_picker_open =
                    indicator_state.color_picker_field.as_deref() == Some("signal_bullish_color");

                ctx.set_fill_color(text_color);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Цвет бычий:", content_x + content_padding, row_y + row_height / 2.0);

                let bull_swatch_y = row_y + (row_height - swatch_size) / 2.0;
                if is_bullish_picker_open {
                    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                    ctx.fill_rect(swatch_col_x - 2.0, bull_swatch_y - 2.0, swatch_size + 4.0, swatch_size + 4.0);
                }
                ctx.set_fill_color(&signal_display.bullish_color);
                ctx.fill_rect(swatch_col_x, bull_swatch_y, swatch_size, swatch_size);
                let bull_border = if is_bullish_picker_open { &toolbar_theme.accent } else { &toolbar_theme.separator };
                ctx.set_stroke_color(bull_border);
                ctx.set_stroke_width(if is_bullish_picker_open { 2.0 } else { 1.0 });
                ctx.stroke_rect(swatch_col_x, bull_swatch_y, swatch_size, swatch_size);

                result.content_items.push((
                    "ind_set:signal_bullish_color".to_string(),
                    WidgetRect::new(swatch_col_x, bull_swatch_y, swatch_size, swatch_size),
                ));
                row_y += row_height;

                // --- Bearish color swatch ---
                let is_bearish_picker_open =
                    indicator_state.color_picker_field.as_deref() == Some("signal_bearish_color");

                ctx.set_fill_color(text_color);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Цвет медведь:", content_x + content_padding, row_y + row_height / 2.0);

                let bear_swatch_y = row_y + (row_height - swatch_size) / 2.0;
                if is_bearish_picker_open {
                    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                    ctx.fill_rect(swatch_col_x - 2.0, bear_swatch_y - 2.0, swatch_size + 4.0, swatch_size + 4.0);
                }
                ctx.set_fill_color(&signal_display.bearish_color);
                ctx.fill_rect(swatch_col_x, bear_swatch_y, swatch_size, swatch_size);
                let bear_border = if is_bearish_picker_open { &toolbar_theme.accent } else { &toolbar_theme.separator };
                ctx.set_stroke_color(bear_border);
                ctx.set_stroke_width(if is_bearish_picker_open { 2.0 } else { 1.0 });
                ctx.stroke_rect(swatch_col_x, bear_swatch_y, swatch_size, swatch_size);

                result.content_items.push((
                    "ind_set:signal_bearish_color".to_string(),
                    WidgetRect::new(swatch_col_x, bear_swatch_y, swatch_size, swatch_size),
                ));
                row_y += row_height;

                // --- Size stepper (8–24 range) ---
                let stepper_btn_w = 22.0;
                let stepper_btn_h = 22.0;
                let stepper_val_w = 36.0;
                let stepper_x = content_x + content_padding + 100.0;

                ctx.set_fill_color(text_color);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Размер:", content_x + content_padding, row_y + row_height / 2.0);

                // Dec button
                let dec_size_x = stepper_x;
                let dec_size_y = row_y + (row_height - stepper_btn_h) / 2.0;
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(dec_size_x, dec_size_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(dec_size_x, dec_size_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text("−", dec_size_x + stepper_btn_w / 2.0, dec_size_y + stepper_btn_h / 2.0);

                result.content_items.push((
                    "ind_set:signal_size_dec".to_string(),
                    WidgetRect::new(dec_size_x, dec_size_y, stepper_btn_w, stepper_btn_h),
                ));

                // Value label
                let val_size_x = dec_size_x + stepper_btn_w + 4.0;
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text(
                    &format!("{}", signal_display.size as i64),
                    val_size_x + stepper_val_w / 2.0,
                    row_y + row_height / 2.0,
                );

                // Inc button
                let inc_size_x = val_size_x + stepper_val_w + 4.0;
                let inc_size_y = dec_size_y;
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(inc_size_x, inc_size_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(inc_size_x, inc_size_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text("+", inc_size_x + stepper_btn_w / 2.0, inc_size_y + stepper_btn_h / 2.0);

                result.content_items.push((
                    "ind_set:signal_size_inc".to_string(),
                    WidgetRect::new(inc_size_x, inc_size_y, stepper_btn_w, stepper_btn_h),
                ));

                ctx.set_text_align(TextAlign::Left);
                row_y += row_height;

                // --- Offset stepper (0–16 range) ---
                ctx.set_fill_color(text_color);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Отступ:", content_x + content_padding, row_y + row_height / 2.0);

                // Dec button
                let dec_off_x = stepper_x;
                let dec_off_y = row_y + (row_height - stepper_btn_h) / 2.0;
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(dec_off_x, dec_off_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(dec_off_x, dec_off_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text("−", dec_off_x + stepper_btn_w / 2.0, dec_off_y + stepper_btn_h / 2.0);

                result.content_items.push((
                    "ind_set:signal_offset_dec".to_string(),
                    WidgetRect::new(dec_off_x, dec_off_y, stepper_btn_w, stepper_btn_h),
                ));

                // Value label
                let val_off_x = dec_off_x + stepper_btn_w + 4.0;
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text(
                    &format!("{}", signal_display.offset as i64),
                    val_off_x + stepper_val_w / 2.0,
                    row_y + row_height / 2.0,
                );

                // Inc button
                let inc_off_x = val_off_x + stepper_val_w + 4.0;
                let inc_off_y = dec_off_y;
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(inc_off_x, inc_off_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(inc_off_x, inc_off_y, stepper_btn_w, stepper_btn_h, 3.0);
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Center);
                ctx.fill_text("+", inc_off_x + stepper_btn_w / 2.0, inc_off_y + stepper_btn_h / 2.0);

                result.content_items.push((
                    "ind_set:signal_offset_inc".to_string(),
                    WidgetRect::new(inc_off_x, inc_off_y, stepper_btn_w, stepper_btn_h),
                ));

                ctx.set_text_align(TextAlign::Left);
                let _ = row_y; // row_y advanced past last used row
            }

            let widget_theme = WidgetTheme::default();
            let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
            result.scrollbar_handle_rect = scroll_result.handle_rect;
            result.scrollbar_track_rect = scroll_result.track_rect;
        }
        IndicatorSettingsTab::Info => {
            // Scrollbar setup
            let scrollbar_width = 8.0;
            let viewport_height = content_height - 30.0 - content_padding;
            let viewport_y = settings_y;

            // First pass: calculate total content height
            let mut total_content_height = 0.0;
            if let Some(def) = definition {
                total_content_height += row_height; // name
                total_content_height += row_height * 0.8; // short name
                total_content_height += row_height * 0.8; // category
                total_content_height += row_height * 0.8; // overlay
                if def.bounds.is_some() {
                    total_content_height += row_height * 0.8; // bounds
                }
                total_content_height += row_height * 0.3; // gap
                total_content_height += row_height * 0.7; // description label
                if def.description.is_empty() {
                    total_content_height += row_height * 0.7;
                } else {
                    let max_width = content_width - content_padding * 2.0 - scrollbar_width - 10.0;
                    let chars_per_line = (max_width / 7.0) as usize;
                    let num_lines = def.description.len().div_ceil(chars_per_line);
                    total_content_height += num_lines as f64 * row_height * 0.7;
                }
                total_content_height += row_height * 0.3; // gap
                total_content_height += row_height * 0.7; // outputs label
                total_content_height += def.outputs.len() as f64 * row_height * 0.6;
            } else {
                total_content_height += row_height * 2.0;
            }

            // Store for result
            result.total_content_height = total_content_height;
            result.viewport_height = viewport_height;

            // Set up scrollable container
            let viewport_rect = WidgetRect::new(content_x, viewport_y, content_width, viewport_height);

            let scroll_config = ScrollableConfig {
                scrollbar_width,
                scrollbar_padding: 4.0,
                always_show_scrollbar: false,
            };
            let scrollable = ScrollableContainer::new(viewport_rect, &indicator_state.scroll, Some(scroll_config));
            scrollable.begin(ctx);

            // Render content with scroll offset
            let mut row_y = scrollable.content_y();

            if let Some(def) = definition {
                // Full name
                ctx.set_font("bold 14px sans-serif");
                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.fill_text(&def.name, content_x + content_padding, row_y);
                row_y += row_height;

                ctx.set_font("13px sans-serif");

                // Short name
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text("Короткое имя:", content_x + content_padding, row_y);
                ctx.set_fill_color(text_color);
                ctx.fill_text(&def.short_name, content_x + content_padding + 120.0, row_y);
                row_y += row_height * 0.8;

                // Category
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text("Категория:", content_x + content_padding, row_y);
                ctx.set_fill_color(text_color);
                ctx.fill_text(&def.category_name, content_x + content_padding + 120.0, row_y);
                row_y += row_height * 0.8;

                // Overlay
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text("Оверлей:", content_x + content_padding, row_y);
                ctx.set_fill_color(text_color);
                ctx.fill_text(if def.overlay { "Да" } else { "Нет" }, content_x + content_padding + 120.0, row_y);
                row_y += row_height * 0.8;

                // Bounds (if any)
                if let Some((min, max)) = def.bounds {
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.fill_text("Границы:", content_x + content_padding, row_y);
                    ctx.set_fill_color(text_color);
                    ctx.fill_text(&format!("{} - {}", min, max), content_x + content_padding + 120.0, row_y);
                    row_y += row_height * 0.8;
                }

                row_y += row_height * 0.3;

                // Description
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text("Описание:", content_x + content_padding, row_y);
                row_y += row_height * 0.7;

                ctx.set_fill_color(text_color);
                if def.description.is_empty() {
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.fill_text("Описание отсутствует", content_x + content_padding, row_y);
                    row_y += row_height * 0.7;
                } else {
                    // Word wrap description
                    let max_width = content_width - content_padding * 2.0 - scrollbar_width - 10.0;
                    let chars_per_line = (max_width / 7.0) as usize;
                    let desc = &def.description;

                    for line in desc.chars().collect::<Vec<_>>().chunks(chars_per_line) {
                        let line_str: String = line.iter().collect();
                        ctx.fill_text(&line_str, content_x + content_padding, row_y);
                        row_y += row_height * 0.7;
                    }
                }

                row_y += row_height * 0.3;

                // Outputs count
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text(&format!("Выходы: {}", def.outputs.len()), content_x + content_padding, row_y);
                row_y += row_height * 0.7;

                for output in &def.outputs {
                    ctx.fill_text(&format!("  • {} ({})", output.display_name, output.output_type.as_str()),
                        content_x + content_padding, row_y);
                    row_y += row_height * 0.6;
                }
            } else {
                ctx.set_font("13px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                ctx.set_fill_color(text_color);
                ctx.fill_text("Информация об индикаторе", content_x + content_padding, row_y);
                row_y += row_height;

                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text("Метаданные недоступны", content_x + content_padding, row_y);
            }

            // End scrollable area and draw scrollbar
            let widget_theme = WidgetTheme::default();
            let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);

            result.scrollbar_handle_rect = scroll_result.handle_rect;
            result.scrollbar_track_rect = scroll_result.track_rect;
        }
    }

    // =========================================================================
    // Render dropdown menu if open (only for Inputs tab)
    // =========================================================================
    if indicator_state.active_tab == IndicatorSettingsTab::Inputs {
        if let Some(ref open_param_name) = indicator_state.open_param_dropdown {
            // Find the parameter to get its options
            if let Some(def) = definition {
                if let Some(param_def) = def.params.iter().find(|p| p.name == *open_param_name) {
                    let options = param_def.get_options_as_strings();
                    if !options.is_empty() {
                        // Build dropdown items from options
                        let dropdown_items: Vec<DropdownItem> = options.iter()
                            .map(|opt| DropdownItem::item(opt, opt))
                            .collect();

                        let dropdown_config = DropdownConfig {
                            items: dropdown_items,
                            min_width: 120.0,
                            max_width: 200.0,
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

                        // Find the dropdown button position from content_items.
                        // content_items stores rects in screen-space (the scroll widget
                        // already accounts for scroll_offset when laying out rows), so
                        // button_rect.y is already the on-screen Y coordinate.
                        let dropdown_id = format!("dropdown_menu:{}", open_param_name);
                        if let Some((_, button_rect)) = result.content_items.iter().find(|(id, _)| id == &dropdown_id) {
                            let button_rect = *button_rect;
                            // button_rect.y is already in screen-space — use it directly.
                            let screen_button_y = button_rect.y;
                            let dropdown_x = button_rect.x;
                            let dropdown_y = screen_button_y + button_rect.height + 2.0;

                            // Only render if the button is visible within the scroll viewport.
                            let viewport_top = tab_viewport_rect.y;
                            let viewport_bottom = tab_viewport_rect.y + tab_viewport_rect.height;
                            if screen_button_y < viewport_bottom && (screen_button_y + button_rect.height) > viewport_top {
                                // Get current value for highlighting
                                let current_value = params.iter()
                                    .find(|(name, _)| name == open_param_name)
                                    .map(|(_, val)| val.as_str())
                                    .unwrap_or("");

                                let dropdown_result = render_dropdown(
                                    ctx,
                                    &dropdown_config,
                                    (dropdown_x, dropdown_y),
                                    &dropdown_theme,
                                    Some(current_value),
                                    |_ctx, _icon, _rect, _color| {
                                        // No icons in simple dropdown
                                    },
                                );

                                // Store dropdown menu items for hit testing
                                // Note: dropdown uses toolbar_core::WidgetRect; we convert to uzor::types::Rect
                                for (item_id, item_rect) in &dropdown_result.item_rects {
                                    result.content_items.push((
                                        format!("param_option:{}:{}", open_param_name, item_id),
                                        WidgetRect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // =========================================================================
    // Footer — single 52px row: "Шаблон" left, "Отмена" + "OK" right
    // =========================================================================
    let footer_y = modal_y + modal_height - footer_height;

    // Footer top border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(modal_x, footer_y);
    ctx.line_to(modal_x + modal_width, footer_y);
    ctx.stroke();

    let button_height = 32.0;
    let button_y = footer_y + (footer_height - button_height) / 2.0;
    let button_padding = 12.0;
    let hovered_btn = indicator_state.hovered_footer_button.as_deref();

    // ── "Шаблон" button (left side, outline only) ────────────────────────────
    let template_btn_width = 80.0;
    let template_btn_x = modal_x + 16.0;
    let is_tmpl_hovered = hovered_btn == Some("template_dropdown");
    ctx.set_stroke_color(if is_tmpl_hovered { &toolbar_theme.item_text } else { &toolbar_theme.separator });
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(template_btn_x, button_y, template_btn_width, button_height);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(if is_tmpl_hovered { &toolbar_theme.item_text_hover } else { text_color });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Шаблон", template_btn_x + template_btn_width / 2.0, button_y + button_height / 2.0);

    result.footer_buttons.push(("template_dropdown".to_string(), WidgetRect::new(template_btn_x, button_y, template_btn_width, button_height)));

    // ── "OK" button (right side, filled #2962ff) ─────────────────────────────
    let ok_btn_width = 70.0;
    let cancel_btn_width = 80.0;
    let ok_btn_x = modal_x + modal_width - 16.0 - ok_btn_width;
    let ok_hovered = hovered_btn == Some("ok");
    ctx.set_fill_color(if ok_hovered { "#4080ff" } else { "#2962ff" });
    ctx.fill_rect(ok_btn_x, button_y, ok_btn_width, button_height);
    ctx.set_fill_color("#ffffff");
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("OK", ok_btn_x + ok_btn_width / 2.0, button_y + button_height / 2.0);

    // ── "Отмена" button (before OK, outline only) ─────────────────────────────
    let cancel_btn_x = ok_btn_x - button_padding - cancel_btn_width;
    let cancel_hovered = hovered_btn == Some("cancel");
    if cancel_hovered {
        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
        ctx.fill_rect(cancel_btn_x, button_y, cancel_btn_width, button_height);
    }
    ctx.set_stroke_color(if cancel_hovered { &toolbar_theme.item_text } else { &toolbar_theme.separator });
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(cancel_btn_x, button_y, cancel_btn_width, button_height);
    ctx.set_fill_color(text_color);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Отмена", cancel_btn_x + cancel_btn_width / 2.0, button_y + button_height / 2.0);

    // ── Template dropdown menu (opens BELOW the "Шаблон" button) ─────────────
    if indicator_state.template_dropdown_open {
        let opt_h = 28.0;
        let sep_h = 1.0;
        let delete_btn_w = 24.0;
        let fixed_rows = 2_usize;
        let tmpl_rows = templates.len().max(1); // at least 1 for "(нет шаблонов)"
        let total_h = opt_h * (fixed_rows + tmpl_rows) as f64 + sep_h + 6.0;
        let dd_x = template_btn_x;
        let menu_w = template_btn_width.max(180.0);
        let menu_y = button_y + button_height + 2.0;

        ctx.set_fill_color(&toolbar_theme.dropdown_bg);
        ctx.fill_rounded_rect(dd_x, menu_y, menu_w, total_h, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(dd_x, menu_y, menu_w, total_h, 4.0);

        // Register background FIRST with coordinator (last-registered wins, so
        // items registered AFTER will override this in hit_test_at)
        input_coordinator.register_on_layer(
            "ind_settings:footer:template_dropdown_menu".to_string(),
            uzor::types::Rect::new(dd_x, menu_y, menu_w, total_h),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );

        let mut row_y = menu_y + 3.0;

        // Row 1: "Сохранить как..."
        let is_sa_hovered = indicator_state.hovered_item_id.as_deref() == Some("template_save_as");
        if is_sa_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
        }
        ctx.set_fill_color(text_color);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Сохранить как...", dd_x + 8.0, row_y + opt_h / 2.0);
        result.footer_buttons.push(("template_save_as".to_string(), WidgetRect::new(dd_x, row_y, menu_w, opt_h)));
        row_y += opt_h;

        // Row 2: "Применить по умолчанию"
        let is_def_hovered = indicator_state.hovered_item_id.as_deref() == Some("template_default");
        if is_def_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
        }
        ctx.set_fill_color(text_color);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Применить по умолчанию", dd_x + 8.0, row_y + opt_h / 2.0);
        result.footer_buttons.push(("template_default".to_string(), WidgetRect::new(dd_x, row_y, menu_w, opt_h)));
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
                let is_row_hovered = indicator_state.hovered_item_id.as_deref() == Some(row_id.as_str());
                let is_del_hovered = indicator_state.hovered_item_id.as_deref() == Some(del_id.as_str());

                if is_row_hovered || is_del_hovered {
                    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                    ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
                }

                ctx.set_fill_color(text_color);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&tmpl.name, dd_x + 8.0, row_y + opt_h / 2.0);

                let del_x = dd_x + menu_w - delete_btn_w - 4.0;
                let del_y = row_y + (opt_h - 24.0) / 2.0;
                let del_color = if is_del_hovered { "#EF5350" } else { &toolbar_theme.item_text_muted };
                draw_svg_icon(ctx, crate::ui::Icon::Close.svg(), del_x, del_y, 24.0, 24.0, del_color);

                let name_w = menu_w - delete_btn_w - 8.0;
                result.footer_buttons.push((row_id, WidgetRect::new(dd_x, row_y, name_w, opt_h)));
                result.footer_buttons.push((del_id, WidgetRect::new(del_x, del_y, delete_btn_w, 24.0)));

                row_y += opt_h;
            }
        }

        // Register dropdown background LAST so specific items win in hit-test
        result.footer_buttons.push(("template_dropdown_menu".to_string(), WidgetRect::new(dd_x, menu_y, menu_w, total_h)));

        let _ = is_sa_hovered;
        let _ = is_def_hovered;
    }

    // Populate result for hit testing
    result.modal_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);
    result.header_rect = WidgetRect::new(modal_x, modal_y, modal_width, header_height);
    result.close_btn_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    result.content_rect = WidgetRect::new(content_x, content_y, content_width, content_height);

    // Tab rects
    for (i, tab) in IndicatorSettingsTab::all().iter().enumerate() {
        let tab_y = sidebar_y + i as f64 * tab_button_height;
        result.tab_rects.push((tab.id().to_string(), WidgetRect::new(sidebar_x, tab_y, sidebar_width, tab_button_height)));
    }

    // Footer buttons
    result.footer_buttons.push(("cancel".to_string(), WidgetRect::new(cancel_btn_x, button_y, cancel_btn_width, button_height)));
    result.footer_buttons.push(("ok".to_string(), WidgetRect::new(ok_btn_x, button_y, ok_btn_width, button_height)));

    // Register footer buttons with coordinator (skip template_dropdown_menu —
    // already registered early so it has lowest priority in hit_test_at)
    for (btn_id, rect) in &result.footer_buttons {
        if btn_id == "template_dropdown_menu" {
            continue;
        }
        input_coordinator.register_on_layer(
            format!("ind_settings:footer:{}", btn_id),
            uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // Register content items with coordinator
    for (item_id, rect) in &result.content_items {
        input_coordinator.register_on_layer(
            format!("ind_settings:item:{}", item_id),
            uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // Register signals toggle if present
    if let Some(ref toggle_rect) = result.signals_toggle_rect {
        input_coordinator.register_on_layer(
            "ind_settings:signals_toggle",
            uzor::types::Rect::new(toggle_rect.x, toggle_rect.y, toggle_rect.width, toggle_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // Register scrollbar track if present
    if let Some(ref track_rect) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "ind_settings:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            uzor::input::sense::Sense::CLICK,
            &layer_id,
        );
    }

    // Pop layer before returning
    input_coordinator.pop_layer(&layer_id);

    result
}
