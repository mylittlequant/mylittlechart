//! Indicator settings modal renderer.
//!
//! Renders the modal dialog that allows users to configure indicator parameters,
//! styles (output colors), timeframe visibility, signal detection, and view
//! indicator metadata.

use mlc_core::engine::render::RenderContext;
use mlc_core::engine::render::draw_svg_icon;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::{IndicatorSettingsModalResult, SliderTrackInfo};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::modal_settings::ResizableModal;
use crate::ui::modal_settings::{IndicatorSettingsState, IndicatorSettingsTab, IndicatorDisplayInfo, IndicatorParamType, DualSliderHandle};
use crate::ui::widgets::{
    draw_input, draw_input_cursor, InputConfig, InputType,
    render_dual_slider, SliderConfig, SliderEditingInfo,
    begin_modal_shell, end_modal_shell, ModalFooter, ModalPlacement, ModalShellSpec, ModalTab, ModalTabs,
    ModalTheme, FOOTER_H, HEADER_H, TAB_RAIL_W,
    WidgetState, WidgetTheme,
};
use crate::layout::modals::control_column;
use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig};
use crate::ui::dropdown::{render_dropdown, DropdownConfig, DropdownItem, DropdownTheme};
use crate::ui::Icon;
use mlc_core::drawing::TimeframeVisibilityConfig;
use crate::layout::render_ui::toolbar_to_widget_theme;
use mlc_core::indicator_source::{SignalDisplayConfig, SignalShape};
use uzor::types::Rect as WidgetRect;
use uzor::render::{TextAlign, TextBaseline};
use mlc_core::i18n::{IndicatorKey, t_indicator};

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
/// Height a parameter dropdown of `rows` items will take, matching the
/// `DropdownConfig` below (item_height 28, padding 4 on both edges).
fn param_dropdown_height(rows: usize) -> f64 {
    rows as f64 * 28.0 + 8.0
}

/// Whether a dropdown anchored at `anchor_y` (height `anchor_h`) has to open
/// UPWARD because it would otherwise run off the bottom of the window.
///
/// Both the chevron and the menu itself ask this, so they cannot disagree —
/// an arrow pointing down at a menu that opened up is its own small lie, and
/// a menu that opened down off the screen simply could not be used.
fn dropdown_opens_upward(anchor_y: f64, anchor_h: f64, rows: usize, screen_h: f64) -> bool {
    let h = param_dropdown_height(rows);
    // Only flip when there is genuinely more room above — on a window too
    // short for either side, downward at least keeps the first rows reachable.
    anchor_y + anchor_h + 2.0 + h > screen_h && anchor_y - 2.0 - h > 0.0
}

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
    _current_time_ms: u64,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    templates: &[mlc_core::templates::IndicatorTemplate],
) -> IndicatorSettingsModalResult {
    let mut result = IndicatorSettingsModalResult {
        signals_enabled,
        ..IndicatorSettingsModalResult::default()
    };

    // Modal dimensions. The size is fixed, so the shell is handed the body
    // height that leaves the frame exactly where it was: chrome heights are
    // the shell's constants now, not this file's.
    let modal_width = 500.0;
    let modal_height = 420.0;
    let sidebar_width = TAB_RAIL_W;
    let content_height = modal_height - HEADER_H - FOOTER_H;

    let text_color = &toolbar_theme.item_text;
    let tab_items: Vec<ModalTab<'_>> = IndicatorSettingsTab::all()
        .iter()
        .map(|tab| {
            let icon = match *tab {
                IndicatorSettingsTab::Inputs => Icon::Settings,
                IndicatorSettingsTab::Style => Icon::Palette,
                IndicatorSettingsTab::Visibility => Icon::Eye,
                IndicatorSettingsTab::Signals => Icon::Alert,
                IndicatorSettingsTab::Info => Icon::Info,
            };
            ModalTab::with_icon(tab.id(), tab.label(), icon)
        })
        .collect();

    let modal_theme = ModalTheme::from_toolbar(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let spec = ModalShellSpec {
        // The body insets itself by `content_padding`, so the shell adds none.
        padding: 0.0,
        placement: ModalPlacement::At(Some(indicator_state.position.unwrap_or((
            chart_x + (screen_w - modal_width) / 2.0,
            chart_y + (screen_h - modal_height) / 2.0,
        )))),
        ..ModalShellSpec::new("ind_settings", indicator_name, modal_width, content_height)
            // The layer keeps its old name: it is the key
            // `close_topmost_known_modal_layer` matches on.
            .layer_named("indicator_settings")
            .with_tabs(ModalTabs::Vertical {
                width: sidebar_width,
                items: &tab_items,
                active: indicator_state.active_tab.index(),
            })
            .with_footer(ModalFooter::Editor {
                template: Some(t_indicator(IndicatorKey::Template)),
            })
            .resizable(indicator_state.size, indicator_state.min_size())
    };
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();

    // =========================================================================
    // Content area
    // =========================================================================
    let content_x = shell.content.x;
    let content_y = shell.content.y;
    // Re-bound to the VIEWPORT the shell settled on: a frame the user dragged
    // bigger is bigger here too, and the natural size above was only ever the
    // starting point.
    let content_width = shell.content.width;
    let content_height = shell.content.height;
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
                let is_editing = indicator_state.active_field_id.as_deref() == Some(field_id.as_str())
                    || indicator_state.is_editing_field(param_name);
                let is_hovered = indicator_state.hovered_item_id.as_deref() == Some(param_name);

                ctx.set_fill_color(text_color);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(param_name, content_x + content_padding, row_y + row_height / 2.0);

                // Input field position — the control column runs to the row's
                // right edge, so widening the modal widens the field.
                let (input_x, control_w) = control_column(
                    content_x + content_padding,
                    content_width - content_padding * 2.0,
                    150.0,
                    80.0,
                );
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
                    Some(IndicatorParamType::Venues) => {
                        // A set, so the field shows the CHOICE, not one value:
                        // "All" when nothing is narrowed (which folds every
                        // venue serving this layer's stream), otherwise the
                        // venues themselves while they fit and a count past
                        // that. The whole field opens the picker — there is no
                        // "cycle" half, because a set has no next value.
                        let field_width = 150.0;
                        let field_rect = WidgetRect::new(input_x, input_y, field_width, input_height);
                        let chosen: Vec<&str> = indicator_state.venue_options.iter()
                            .filter(|(_, on)| *on)
                            .map(|(v, _)| v.as_str())
                            .collect();
                        let label = if indicator_state.venue_all {
                            // Following the offered list, whatever it becomes.
                            format!("All ({})", chosen.len())
                        } else if chosen.is_empty() {
                            "None".to_string()
                        } else {
                            let joined = chosen.join(", ");
                            if joined.chars().count() <= 16 { joined } else { format!("{} venues", chosen.len()) }
                        };

                        let field_config = InputConfig::new(&label)
                            .with_type(InputType::Text)
                            .with_font_size(13.0)
                            .with_padding(8.0)
                            .with_radius(0.0);
                        let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
                        let widget_state = if is_hovered { WidgetState::Hovered } else { WidgetState::Normal };
                        draw_input(ctx, &field_config, widget_state, field_rect, &widget_theme);

                        let arrow_x = input_x + field_width - 14.0;
                        let arrow_y = row_y + row_height / 2.0;
                        let up = dropdown_opens_upward(
                            input_y, input_height, indicator_state.venue_options.len(), screen_h,
                        );
                        ctx.set_fill_color(text_color);
                        ctx.begin_path();
                        if up {
                            ctx.move_to(arrow_x, arrow_y + 3.0);
                            ctx.line_to(arrow_x + 6.0, arrow_y + 3.0);
                            ctx.line_to(arrow_x + 3.0, arrow_y - 3.0);
                        } else {
                            ctx.move_to(arrow_x, arrow_y - 3.0);
                            ctx.line_to(arrow_x + 6.0, arrow_y - 3.0);
                            ctx.line_to(arrow_x + 3.0, arrow_y + 3.0);
                        }
                        ctx.close_path();
                        ctx.fill();

                        // Same id the Select rows use for their menu half, so
                        // the open/close handling is shared.
                        result.content_items.push((format!("dropdown_menu:{}", param_name), field_rect));
                    }
                    Some(IndicatorParamType::Source) | Some(IndicatorParamType::Select { .. }) => {
                        // Render split dropdown for source/select
                        let dropdown_width = control_w;
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

                        // Dropdown arrow (chevron) — points the way the menu
                        // will actually open.
                        let arrow_x = input_x + text_width + chevron_width / 2.0 - 3.0;
                        let arrow_y = row_y + row_height / 2.0;
                        let option_rows = match param_type {
                            Some(IndicatorParamType::Select { options }) => options.len(),
                            // Source is the OHLCV field list, fixed length.
                            Some(IndicatorParamType::Source) => 8,
                            _ => 0,
                        };
                        let up = dropdown_opens_upward(input_y, input_height, option_rows, screen_h);
                        ctx.set_fill_color(text_color);
                        ctx.begin_path();
                        if up {
                            ctx.move_to(arrow_x, arrow_y + 3.0);
                            ctx.line_to(arrow_x + 6.0, arrow_y + 3.0);
                            ctx.line_to(arrow_x + 3.0, arrow_y - 3.0);
                        } else {
                            ctx.move_to(arrow_x, arrow_y - 3.0);
                            ctx.line_to(arrow_x + 6.0, arrow_y - 3.0);
                            ctx.line_to(arrow_x + 3.0, arrow_y + 3.0);
                        }
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
                        let input_width = control_w;

                        // Get text and cursor from active_render (populated by ChartApp from TextFieldStore).
                        let ar = &indicator_state.active_render;
                        let (display_text, cursor_pos, selection_start, selection_end) = if is_editing {
                            (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
                        } else if indicator_state.is_editing_field(param_name) {
                            // Legacy fallback for LEGACY editing_text/editing_cursor
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
                            let cursor_visible = indicator_state.active_render.cursor_visible;

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
                ctx.fill_text(t_indicator(IndicatorKey::NoParams), content_x + content_padding, scrollable.content_y() + row_height / 2.0);
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
                ctx.fill_text(t_indicator(IndicatorKey::NoOutputs), content_x + content_padding, scrollable.content_y() + row_height / 2.0);
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

                    let ar = &indicator_state.active_render;
                    let editing_min_info = if indicator_state.active_field_id.as_deref() == Some(min_field_id.as_str()) {
                        Some(SliderEditingInfo { text: ar.text.as_str(), cursor: ar.cursor, selection_start: ar.selection_start })
                    } else {
                        None
                    };

                    let editing_max_info = if indicator_state.active_field_id.as_deref() == Some(max_field_id.as_str()) {
                        Some(SliderEditingInfo { text: ar.text.as_str(), cursor: ar.cursor, selection_start: ar.selection_start })
                    } else {
                        None
                    };

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
                        if indicator_state.active_field_id.as_deref() == Some(min_field_id.as_str()) {
                            result.active_input_rect = Some(min_input_rect);
                            if let Some(ref ir) = slider_result.min_input_result {
                                result.active_input_char_positions = ir.char_x_positions.clone();
                            }
                        }
                    }
                    if let Some(max_input_rect) = slider_result.max_input_rect {
                        result.content_items.push((max_field_id.clone(), max_input_rect));
                        if indicator_state.active_field_id.as_deref() == Some(max_field_id.as_str()) {
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
            ctx.fill_text(t_indicator(IndicatorKey::EnableSignals), content_x + content_padding, row_y + row_height / 2.0);

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
            ctx.fill_text(t_indicator(IndicatorKey::SignalsAutoLine1), content_x + content_padding, row_y);
            row_y += row_height * 0.6;
            ctx.fill_text(t_indicator(IndicatorKey::SignalsAutoLine2), content_x + content_padding, row_y);
            row_y += row_height;

            // Signal display configuration controls (shown only when signals are enabled)
            if signals_on {
                row_y += 12.0; // visual gap before config section

                // --- Shape selector ---
                ctx.set_fill_color(text_color);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(t_indicator(IndicatorKey::Shape), content_x + content_padding, row_y + row_height / 2.0);

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
                ctx.fill_text(t_indicator(IndicatorKey::BullColor), content_x + content_padding, row_y + row_height / 2.0);

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
                ctx.fill_text(t_indicator(IndicatorKey::BearColor), content_x + content_padding, row_y + row_height / 2.0);

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
                ctx.fill_text(t_indicator(IndicatorKey::SignalSize), content_x + content_padding, row_y + row_height / 2.0);

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
                ctx.fill_text(t_indicator(IndicatorKey::SignalOffset), content_x + content_padding, row_y + row_height / 2.0);

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
                ctx.fill_text(t_indicator(IndicatorKey::ShortName), content_x + content_padding, row_y);
                ctx.set_fill_color(text_color);
                ctx.fill_text(&def.short_name, content_x + content_padding + 120.0, row_y);
                row_y += row_height * 0.8;

                // Category
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text(t_indicator(IndicatorKey::Category), content_x + content_padding, row_y);
                ctx.set_fill_color(text_color);
                ctx.fill_text(&def.category_name, content_x + content_padding + 120.0, row_y);
                row_y += row_height * 0.8;

                // Overlay
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text(t_indicator(IndicatorKey::Overlay), content_x + content_padding, row_y);
                ctx.set_fill_color(text_color);
                ctx.fill_text(if def.overlay { "Да" } else { "Нет" }, content_x + content_padding + 120.0, row_y);
                row_y += row_height * 0.8;

                // Bounds (if any)
                if let Some((min, max)) = def.bounds {
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.fill_text(t_indicator(IndicatorKey::Bounds), content_x + content_padding, row_y);
                    ctx.set_fill_color(text_color);
                    ctx.fill_text(&format!("{} - {}", min, max), content_x + content_padding + 120.0, row_y);
                    row_y += row_height * 0.8;
                }

                row_y += row_height * 0.3;

                // Description
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text(t_indicator(IndicatorKey::Description), content_x + content_padding, row_y);
                row_y += row_height * 0.7;

                ctx.set_fill_color(text_color);
                if def.description.is_empty() {
                    ctx.set_fill_color(&toolbar_theme.item_text_muted);
                    ctx.fill_text(t_indicator(IndicatorKey::NoDescription), content_x + content_padding, row_y);
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
                ctx.fill_text(t_indicator(IndicatorKey::IndicatorInfo), content_x + content_padding, row_y);
                row_y += row_height;

                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text(t_indicator(IndicatorKey::MetadataUnavailable), content_x + content_padding, row_y);
            }

            // End scrollable area and draw scrollbar
            let widget_theme = WidgetTheme::default();
            let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);

            result.scrollbar_handle_rect = scroll_result.handle_rect;
            result.scrollbar_track_rect = scroll_result.track_rect;
        }
    }

    // =========================================================================
    // Footer
    // =========================================================================
    // The bar itself — separator, Template button, OK — is the shell's. There
    // is no Cancel next to OK, and there was nothing for one to do: every
    // control here applies to the live instance the moment it is touched, so
    // `cancel` and `ok` both ran `close()`, the same thing the ✕ does. A
    // button labelled "Отмена" that cancels nothing is worse than no button.
    //
    // What is left below is the menu that hangs off the Template button.
    let template_btn = shell.template_btn.unwrap_or_default();
    let button_height = template_btn.height;
    let button_y = template_btn.y;
    let template_btn_width = template_btn.width;
    let template_btn_x = template_btn.x;

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
            uzor::input::Sense::CLICK,
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
        ctx.fill_text(t_indicator(IndicatorKey::SaveAs), dd_x + 8.0, row_y + opt_h / 2.0);
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
        ctx.fill_text(t_indicator(IndicatorKey::ApplyDefault), dd_x + 8.0, row_y + opt_h / 2.0);
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

    // Populate result for hit testing. Frame, header and ✕ came from the
    // shell at the top; only the body rect is this file's to report.
    result.content_rect = WidgetRect::new(content_x, content_y, content_width, content_height);

    // Phase 6.1a: register scrollable content area for coordinator dispatch
    input_coordinator.register_on_layer(
        "ind_settings:scroll_viewport",
        uzor::types::Rect::new(content_x, content_y, content_width, content_height),
        uzor::input::Sense::SCROLL,
        &layer_id,
    );

    // Tab rects — where the shell put them.
    result.tab_rects = shell.tab_rects.clone();

    // Painted LAST so it lands on top of the footer: an open list used to
    // be drawn before the OK/Cancel row, which then covered its first
    // items. Registration order is unchanged — the item rects go into
    // `content_items`, registered below the footer's own, so a click on an
    // overlapping row still reaches the list rather than the button under
    // it.
    // =========================================================================
    // Render dropdown menu if open (only for Inputs tab)
    // =========================================================================
    if indicator_state.active_tab == IndicatorSettingsTab::Inputs {
        if let Some(ref open_param_name) = indicator_state.open_param_dropdown {
            // Find the parameter to get its options
            if let Some(def) = definition {
                if let Some(param_def) = def.params.iter().find(|p| p.name == *open_param_name) {
                    let is_venues = param_def.param_type == IndicatorParamType::Venues;
                    // A venue row's options come from the HOST (which
                    // connectors serve this layer's stream right now), not
                    // from the static definition.
                    let options: Vec<String> = if is_venues {
                        indicator_state.venue_options.iter().map(|(v, _)| v.clone()).collect()
                    } else {
                        param_def.get_options_as_strings()
                    };
                    if !options.is_empty() {
                        // Build dropdown items from options. A SET marks every
                        // chosen row, where a single-value dropdown highlights
                        // exactly one — `render_dropdown`'s selected-item
                        // argument can only say "one", so the mark rides in
                        // the label.
                        let dropdown_items: Vec<DropdownItem> = if is_venues {
                            indicator_state.venue_options.iter()
                                .map(|(v, on)| {
                                    // A tick, or nothing. A second glyph on the
                                    // unticked rows only competes with the tick
                                    // it is meant to contrast with; the blank
                                    // keeps the labels aligned.
                                    DropdownItem::item(v, &format!("{} {}", if *on { "✓" } else { " " }, v))
                                })
                                .collect()
                        } else {
                            options.iter().map(|opt| DropdownItem::item(opt, opt)).collect()
                        };

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
                            clamp_x: None,
                        };

                        let dropdown_theme = DropdownTheme {
                            // The modal's OWN panel colour, forced opaque. It
                            // used to take the toolbar's dropdown token, which
                            // a skin may derive with an alpha — and a menu you
                            // can read the chart through is not a menu.
                            background: mlc_core::apply_opacity(&frame_theme.toolbar_bg, 1.0),
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
                            accent: toolbar_theme.accent.clone(),
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
                            // Open upward when there is no room below, so the
                            // menu cannot run off the bottom of the window —
                            // the failure the owner hit with the settings modal
                            // parked low on screen.
                            let rows = dropdown_config.items.len();
                            let dropdown_y = if dropdown_opens_upward(
                                screen_button_y, button_rect.height, rows, screen_h,
                            ) {
                                screen_button_y - 2.0 - param_dropdown_height(rows)
                            } else {
                                screen_button_y + button_rect.height + 2.0
                            };

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


    // Register footer buttons with coordinator (skip template_dropdown_menu —
    // already registered early so it has lowest priority in hit_test_at)
    for (btn_id, rect) in &result.footer_buttons {
        if btn_id == "template_dropdown_menu" {
            continue;
        }
        input_coordinator.register_on_layer(
            format!("ind_settings:footer:{}", btn_id),
            uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            &layer_id,
        );
    }

    // Register content items with coordinator
    for (item_id, rect) in &result.content_items {
        input_coordinator.register_on_layer(
            format!("ind_settings:item:{}", item_id),
            uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            &layer_id,
        );
    }

    // Register signals toggle if present
    if let Some(ref toggle_rect) = result.signals_toggle_rect {
        input_coordinator.register_on_layer(
            "ind_settings:signals_toggle",
            uzor::types::Rect::new(toggle_rect.x, toggle_rect.y, toggle_rect.width, toggle_rect.height),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // Register scrollbar handle (inflated ±5px X) and track
    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "ind_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            &layer_id,
        );
    }
    if let Some(ref track_rect) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "ind_settings:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // Pop layer before returning
    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id);

    result
}
