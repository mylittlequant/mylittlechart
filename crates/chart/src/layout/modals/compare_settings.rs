//! Compare Settings Modal renderer.
//!
//! Renders the modal dialog for configuring a compare series (line color,
//! line width, line style, visibility, and series info).
//!
//! Tabs: Style | Visibility | Info
//!
//! Widget ID prefix: `"cmp_settings:"`
//!
//! Display data is read from `CompareSettingsState::cached_*` fields that were
//! populated when the modal was opened (and refreshed each frame by the caller).

use crate::engine::render::{draw_svg_icon, RenderContext};
use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{CompareSettingsState, CompareSettingsTab, DualSliderHandle};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{
    render_modal_frame_only, ModalTheme,
    render_dual_slider, SliderConfig, SliderEditingInfo,
};
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use crate::drawing::TimeframeVisibilityConfig;
use crate::layout::render_ui::toolbar_to_widget_theme;
use uzor::input::Sense;
use uzor::types::Rect as WidgetRect;
use uzor::render::{TextAlign, TextBaseline};

// =============================================================================
// Result type
// =============================================================================

/// Render result from the Compare Settings modal.
#[derive(Clone, Debug, Default)]
pub struct CompareSettingsResult {
    /// Bounding rect of the whole modal.
    pub modal_rect: WidgetRect,
    /// Header rect (for drag detection).
    pub header_rect: WidgetRect,
    /// Close button rect.
    pub close_btn_rect: WidgetRect,
    /// Color swatch rect (Style tab) — used to anchor the color picker.
    pub color_swatch_rect: Option<WidgetRect>,
    /// Slider track info for the line-width slider (Style tab).
    pub line_width_slider: Option<SliderTrackInfo>,
    /// Dual-handle slider tracks for tf_*_slider fields (Visibility tab).
    pub tf_slider_tracks: Vec<SliderTrackInfo>,
    /// Content items (item_id, rect) registered in the Visibility tab for hit-testing.
    pub tf_content_items: Vec<(String, WidgetRect)>,
    /// Active input rect (when a text field is being edited in the Visibility tab).
    pub tf_active_input_rect: Option<WidgetRect>,
    /// Char x-positions for the active input (for click-to-cursor).
    pub tf_active_input_char_positions: Vec<f64>,
}

/// Track metadata for a slider so the input handler can start a drag with the
/// correct coordinate range.
#[derive(Clone, Debug, Default)]
pub struct SliderTrackInfo {
    pub field_id: String,
    pub track_x: f64,
    pub track_width: f64,
    /// Top edge of the slider hit zone (track center minus handle radius).
    pub track_y: f64,
    /// Height of the slider hit zone (handle diameter).
    pub track_height: f64,
    pub min_val: f64,
    pub max_val: f64,
}

// =============================================================================
// Layout constants
// =============================================================================

const MODAL_W: f64 = 460.0;
const MODAL_H: f64 = 392.0;
const HEADER_H: f64 = 40.0;
const TAB_BAR_H: f64 = 32.0;
const FOOTER_H: f64 = 52.0;
const PADDING: f64 = 14.0;
const ROW_H: f64 = 30.0;
const LABEL_W: f64 = 100.0;
const SWATCH_SIZE: f64 = 18.0;
const RADIUS: f64 = 6.0;

// =============================================================================
// Public entry point
// =============================================================================

/// Render the Compare Settings modal.
///
/// Returns hit-zone data used by the input handler.
pub fn render_compare_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &CompareSettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    templates: &[crate::templates::CompareTemplate],
) -> CompareSettingsResult {
    let mut result = CompareSettingsResult::default();

    // -------------------------------------------------------------------------
    // Modal position
    // -------------------------------------------------------------------------
    let modal_x = if let Some((px, _)) = state.position {
        px.max(0.0).min(screen_w - MODAL_W)
    } else {
        (screen_w - MODAL_W) / 2.0
    };
    let modal_y = if let Some((_, py)) = state.position {
        py.max(0.0).min(screen_h - MODAL_H)
    } else {
        (screen_h - MODAL_H) / 2.0
    };

    let modal_rect = WidgetRect::new(modal_x, modal_y, MODAL_W, MODAL_H);
    result.modal_rect = modal_rect;
    result.header_rect = WidgetRect::new(modal_x, modal_y, MODAL_W, HEADER_H);

    // -------------------------------------------------------------------------
    // Modal frame (shadow + background + border)
    // -------------------------------------------------------------------------
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, RADIUS);

    // -------------------------------------------------------------------------
    // Input layer (pushed AFTER frame so clicks on the frame are captured)
    // -------------------------------------------------------------------------
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "compare_settings");

    // Backdrop catch-all — absorbs clicks that didn't hit any specific widget
    input_coordinator.register_on_layer(
        "cmp_settings:modal_bg",
        modal_rect,
        Sense::CLICK,
        &layer_id,
    );

    // -------------------------------------------------------------------------
    // Header
    // -------------------------------------------------------------------------
    draw_header(ctx, modal_x, modal_y, MODAL_W, HEADER_H, toolbar_theme, input_coordinator, &layer_id, &mut result);
    input_coordinator.register_on_layer(
        "compare_settings:header",
        result.header_rect,
        Sense::DRAG,
        &layer_id,
    );

    // -------------------------------------------------------------------------
    // Tab bar
    // -------------------------------------------------------------------------
    let tab_y = modal_y + HEADER_H;
    draw_tab_bar(ctx, modal_x, tab_y, MODAL_W, TAB_BAR_H, state.active_tab, toolbar_theme, input_coordinator, &layer_id);

    // -------------------------------------------------------------------------
    // Content area
    // -------------------------------------------------------------------------
    let content_x = modal_x + PADDING;
    let content_y = tab_y + TAB_BAR_H;
    let content_w = MODAL_W - PADDING * 2.0;

    match state.active_tab {
        CompareSettingsTab::Style => {
            draw_style_tab(
                ctx, content_x, content_y, content_w,
                state, toolbar_theme,
                input_coordinator, &layer_id,
                &mut result,
            );
        }
        CompareSettingsTab::Visibility => {
            draw_visibility_tab(
                ctx, content_x, content_y, content_w,
                state, toolbar_theme, frame_theme,
                input_coordinator, &layer_id,
                &mut result,
            );
        }
        CompareSettingsTab::Info => {
            draw_info_tab(ctx, content_x, content_y, state, toolbar_theme);
        }
    }

    // -------------------------------------------------------------------------
    // Footer (template toolbar + OK / Cancel buttons)
    // -------------------------------------------------------------------------
    let footer_y = modal_y + MODAL_H - FOOTER_H;
    draw_footer(ctx, modal_x, footer_y, MODAL_W, FOOTER_H, state, toolbar_theme, input_coordinator, &layer_id, &mut result, templates);

    input_coordinator.pop_layer(&layer_id);

    result
}

// =============================================================================
// Private sub-renderers
// =============================================================================

fn draw_header(
    ctx: &mut dyn RenderContext,
    modal_x: f64,
    modal_y: f64,
    modal_w: f64,
    header_h: f64,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut CompareSettingsResult,
) {
    // Separator below header
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_h);
    ctx.line_to(modal_x + modal_w, modal_y + header_h);
    ctx.stroke();

    // Title text
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Настройки", modal_x + 14.0, modal_y + header_h / 2.0);

    // Close (×) button
    let close_size = 20.0;
    let close_x = modal_x + modal_w - 12.0 - close_size;
    let close_y = modal_y + (header_h - close_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, close_size, close_size);

    input_coordinator.register_on_layer(
        "cmp_settings:close",
        close_rect,
        Sense::click(),
        layer_id,
    );

    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, &toolbar_theme.item_text_muted);

    result.close_btn_rect = close_rect;
}

fn draw_tab_bar(
    ctx: &mut dyn RenderContext,
    modal_x: f64,
    tab_y: f64,
    modal_w: f64,
    tab_h: f64,
    active_tab: CompareSettingsTab,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
) {
    let tabs = CompareSettingsTab::all();
    let tab_w = modal_w / tabs.len() as f64;

    for (i, tab) in tabs.iter().enumerate() {
        let tx = modal_x + i as f64 * tab_w;
        let is_active = *tab == active_tab;

        // Active tab indicator bar at bottom
        if is_active {
            ctx.set_fill_color(&toolbar_theme.item_bg_active);
            ctx.fill_rect(tx, tab_y + tab_h - 2.0, tab_w, 2.0);
        }

        // Tab label
        ctx.set_fill_color(if is_active { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted });
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(tab.label(), tx + tab_w / 2.0, tab_y + tab_h / 2.0);

        // Register click zone
        let widget_id = format!("cmp_settings:tab:{}", tab.id());
        input_coordinator.register_on_layer(
            widget_id.as_str(),
            WidgetRect::new(tx, tab_y, tab_w, tab_h),
            Sense::click(),
            layer_id,
        );
    }

    // Bottom separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, tab_y + tab_h);
    ctx.line_to(modal_x + modal_w, tab_y + tab_h);
    ctx.stroke();
}

#[allow(clippy::too_many_arguments)]
fn draw_style_tab(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    state: &CompareSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut CompareSettingsResult,
) {
    ctx.set_font("12px sans-serif");
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_text_align(TextAlign::Left);

    let mut row_y = content_y + 8.0;

    // --- Row 1: Line color ---
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.fill_text("Цвет", content_x, row_y + ROW_H / 2.0);

    let swatch_x = content_x + LABEL_W;
    let swatch_y = row_y + (ROW_H - SWATCH_SIZE) / 2.0;
    let swatch_rect = WidgetRect::new(swatch_x, swatch_y, SWATCH_SIZE, SWATCH_SIZE);

    ctx.set_fill_color(&state.cached_color);
    ctx.fill_rounded_rect(swatch_x, swatch_y, SWATCH_SIZE, SWATCH_SIZE, 3.0);

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(swatch_x, swatch_y, SWATCH_SIZE, SWATCH_SIZE, 3.0);

    input_coordinator.register_on_layer(
        "cmp_settings:color_swatch",
        swatch_rect,
        Sense::click(),
        layer_id,
    );
    result.color_swatch_rect = Some(swatch_rect);

    row_y += ROW_H;

    // --- Row 2: Line width slider ---
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("Ширина", content_x, row_y + ROW_H / 2.0);

    let track_x = content_x + LABEL_W;
    let track_w = content_w - LABEL_W - 52.0;
    let track_cy = row_y + ROW_H / 2.0;
    let track_h = 4.0;

    let min_w = 0.5_f64;
    let max_w = 8.0_f64;
    let actual_w = state.cached_line_width as f64;

    // Use floating drag value if a drag is in progress for this field
    let display_w = state.slider_drag.as_ref()
        .and_then(|d| if d.field_id == "line_width" { d.floating_value } else { None })
        .unwrap_or(actual_w);

    let t = ((display_w - min_w) / (max_w - min_w)).clamp(0.0, 1.0);
    let filled_w = track_w * t;

    // Track background
    ctx.set_fill_color(&toolbar_theme.separator);
    ctx.fill_rounded_rect(track_x, track_cy - track_h / 2.0, track_w, track_h, 2.0);

    // Filled portion
    if filled_w > 0.0 {
        ctx.set_fill_color(&toolbar_theme.item_bg_active);
        ctx.fill_rounded_rect(track_x, track_cy - track_h / 2.0, filled_w, track_h, 2.0);
    }

    // Handle circle
    let handle_r = 6.0;
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.begin_path();
    ctx.arc(track_x + filled_w, track_cy, handle_r, 0.0, std::f64::consts::TAU);
    ctx.fill();

    // Value label
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(
        &format!("{:.1}px", display_w),
        track_x + track_w + 8.0,
        track_cy,
    );

    // Slider hit zone
    let slider_hit = WidgetRect::new(
        track_x - handle_r,
        track_cy - handle_r,
        track_w + handle_r * 2.0,
        handle_r * 2.0,
    );
    input_coordinator.register_on_layer(
        "cmp_settings:line_width_slider",
        slider_hit,
        Sense::click_and_drag() | Sense::SCROLL,
        layer_id,
    );
    result.line_width_slider = Some(SliderTrackInfo {
        field_id: "line_width".to_string(),
        track_x,
        track_width: track_w,
        track_y: track_cy - handle_r,
        track_height: handle_r * 2.0,
        min_val: min_w,
        max_val: max_w,
    });

    row_y += ROW_H;

    // --- Row 3: Line style dropdown ---
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("Стиль линии", content_x, row_y + ROW_H / 2.0);

    let dd_x = content_x + LABEL_W;
    let dd_w = 120.0;
    let dd_h = 22.0;
    let dd_y = row_y + (ROW_H - dd_h) / 2.0;

    let is_hovered = state.hovered_item_id.as_deref() == Some("line_style_dd");
    ctx.set_fill_color(if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg });
    ctx.fill_rounded_rect(dd_x, dd_y, dd_w, dd_h, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(dd_x, dd_y, dd_w, dd_h, 3.0);

    let style_label = match state.cached_line_style.as_str() {
        "dashed" => "Пунктирная",
        "dotted" => "Точечная",
        _ => "Сплошная",
    };

    // Draw line style preview inside the dropdown button
    {
        let preview_x_start = dd_x + 6.0;
        let preview_x_end = preview_x_start + 24.0;
        let preview_y = dd_y + dd_h / 2.0;
        ctx.set_stroke_color(&toolbar_theme.item_text);
        ctx.set_stroke_width(1.5);
        match state.cached_line_style.as_str() {
            "dashed" => {
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
            "dotted" => {
                ctx.set_fill_color(&toolbar_theme.item_text);
                let gap = 4.0;
                let mut x = preview_x_start;
                while x <= preview_x_end {
                    ctx.begin_path();
                    ctx.arc(x, preview_y, 1.0, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                    x += gap;
                }
            }
            _ => {
                ctx.begin_path();
                ctx.move_to(preview_x_start, preview_y);
                ctx.line_to(preview_x_end, preview_y);
                ctx.stroke();
            }
        }
    }

    // Label text to the right of the preview
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(style_label, dd_x + 36.0, dd_y + dd_h / 2.0);

    let arrow_size = 12.0;
    let arrow_x = dd_x + dd_w - arrow_size - 4.0;
    let arrow_y = dd_y + (dd_h - arrow_size) / 2.0;
    draw_svg_icon(ctx, Icon::ChevronDown.svg(), arrow_x, arrow_y, arrow_size, arrow_size, &toolbar_theme.item_text_muted);

    input_coordinator.register_on_layer(
        "cmp_settings:line_style_dd",
        WidgetRect::new(dd_x, dd_y, dd_w, dd_h),
        Sense::click(),
        layer_id,
    );

    // Line style dropdown options (shown below the dropdown if open)
    if state.line_style_dropdown_open {
        let opts: &[(&str, &str)] = &[
            ("solid",  "Сплошная"),
            ("dashed", "Пунктирная"),
            ("dotted", "Точечная"),
        ];
        let opt_h = 24.0;
        let menu_w = dd_w;
        let menu_x = dd_x;
        let menu_y = dd_y + dd_h;
        let menu_total_h = opt_h * opts.len() as f64;

        // Background
        ctx.set_fill_color(&toolbar_theme.dropdown_bg);
        ctx.fill_rounded_rect(menu_x, menu_y, menu_w, menu_total_h, 3.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(menu_x, menu_y, menu_w, menu_total_h, 3.0);

        for (i, (style_id, label)) in opts.iter().enumerate() {
            let opt_y = menu_y + i as f64 * opt_h;
            let widget_id = format!("cmp_settings:line_style_option:{}", style_id);

            let is_opt_hovered = state.hovered_item_id.as_deref()
                == Some(widget_id.strip_prefix("cmp_settings:").unwrap_or(""));
            let is_selected = state.cached_line_style.as_str() == *style_id;

            if is_opt_hovered || is_selected {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rect(menu_x, opt_y, menu_w, opt_h);
            }

            // Draw line style preview instead of text characters
            let preview_x_start = menu_x + 8.0;
            let preview_x_end = preview_x_start + 32.0;
            let preview_y = opt_y + opt_h / 2.0;
            ctx.set_stroke_color(&toolbar_theme.item_text);
            ctx.set_stroke_width(1.5);
            match *style_id {
                "solid" => {
                    ctx.begin_path();
                    ctx.move_to(preview_x_start, preview_y);
                    ctx.line_to(preview_x_end, preview_y);
                    ctx.stroke();
                }
                "dashed" => {
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
                "dotted" => {
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    let gap = 4.0;
                    let mut x = preview_x_start;
                    while x <= preview_x_end {
                        ctx.begin_path();
                        ctx.arc(x, preview_y, 1.0, 0.0, std::f64::consts::TAU);
                        ctx.fill();
                        x += gap;
                    }
                }
                _ => {
                    ctx.begin_path();
                    ctx.move_to(preview_x_start, preview_y);
                    ctx.line_to(preview_x_end, preview_y);
                    ctx.stroke();
                }
            }

            // Label text to the right of the preview
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, preview_x_end + 6.0, preview_y);

            input_coordinator.register_on_layer(
                widget_id.as_str(),
                WidgetRect::new(menu_x, opt_y, menu_w, opt_h),
                Sense::click(),
                layer_id,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_visibility_tab(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    state: &CompareSettingsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut CompareSettingsResult,
) {
    let tf_config = state.cached_timeframe_visibility.clone()
        .unwrap_or_else(TimeframeVisibilityConfig::all);

    // 7 rows: label, is_bool_only, enabled, range, min_allowed, max_allowed
    let timeframes: [(&str, bool, bool, Option<(u32, u32)>, u32, u32); 7] = [
        ("Тики",    true,  tf_config.ticks,             None,                  0, 0),
        ("Секунды", false, tf_config.seconds.is_some(), tf_config.seconds,     1, 59),
        ("Минуты",  false, tf_config.minutes.is_some(), tf_config.minutes,     1, 59),
        ("Часы",    false, tf_config.hours.is_some(),   tf_config.hours,       1, 24),
        ("Дни",     false, tf_config.days.is_some(),    tf_config.days,        1, 366),
        ("Недели",  false, tf_config.weeks.is_some(),   tf_config.weeks,       1, 52),
        ("Месяцы",  false, tf_config.months.is_some(),  tf_config.months,      1, 12),
    ];

    let checkbox_size = 16.0;
    let label_width_tf = 70.0;
    let input_width = 32.0;
    let slider_width_tf = content_w - checkbox_size - 6.0 - label_width_tf - input_width * 2.0 - 6.0 * 3.0;
    let slider_width_tf = slider_width_tf.max(80.0);
    let gap = 6.0;
    let row_gap = 4.0;
    let row_height = ROW_H;
    let mut row_y = content_y + 8.0;

    for (i, (label, is_bool_only, enabled, range, min_allowed, max_allowed)) in timeframes.iter().enumerate() {
        let check_x = content_x;
        let check_y = row_y + (row_height - checkbox_size) / 2.0;

        // Checkbox fill
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

        // Checkbox + label hit area
        let checkbox_hit = WidgetRect::new(check_x, row_y, checkbox_size + 6.0 + label_width_tf, row_height);
        result.tf_content_items.push((format!("tf_{}_toggle", i), checkbox_hit));

        // Dual slider (min/max) controls — only for non-bool categories when enabled
        if !*is_bool_only && *enabled {
            let controls_x = content_x + checkbox_size + 6.0 + label_width_tf + gap;
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

            // Apply floating preview during drag
            let (display_min, display_max) = if let Some(ref drag) = state.slider_drag {
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

            result.tf_content_items.push((slider_field_id.clone(), slider_result.full_rect));

            if let Some(min_input_rect) = slider_result.min_input_rect {
                result.tf_content_items.push((min_field_id.clone(), min_input_rect));
                if state.editing_text.as_ref().map(|e| e.field_id == min_field_id).unwrap_or(false) {
                    result.tf_active_input_rect = Some(min_input_rect);
                    if let Some(ref ir) = slider_result.min_input_result {
                        result.tf_active_input_char_positions = ir.char_x_positions.clone();
                    }
                }
            }
            if let Some(max_input_rect) = slider_result.max_input_rect {
                result.tf_content_items.push((max_field_id.clone(), max_input_rect));
                if state.editing_text.as_ref().map(|e| e.field_id == max_field_id).unwrap_or(false) {
                    result.tf_active_input_rect = Some(max_input_rect);
                    if let Some(ref ir) = slider_result.max_input_result {
                        result.tf_active_input_char_positions = ir.char_x_positions.clone();
                    }
                }
            }

            if let Some(widget_track_info) = slider_result.track_info {
                let handle_r = 6.0;
                result.tf_slider_tracks.push(SliderTrackInfo {
                    field_id: slider_field_id,
                    track_x: widget_track_info.track_x,
                    track_width: widget_track_info.track_width,
                    track_y: slider_result.full_rect.y,
                    track_height: slider_result.full_rect.height.max(handle_r * 2.0),
                    min_val: *min_allowed as f64,
                    max_val: *max_allowed as f64,
                });
            }
        }

        row_y += row_height + row_gap;
    }

    // Register all content items
    let slider_field_ids: std::collections::HashSet<&str> = result.tf_slider_tracks
        .iter()
        .map(|s| s.field_id.as_str())
        .collect();

    for (item_id, item_rect) in &result.tf_content_items {
        let sense = if slider_field_ids.contains(item_id.as_str()) {
            Sense::DRAG | Sense::SCROLL
        } else {
            Sense::CLICK
        };
        input_coordinator.register_on_layer(
            format!("cmp_settings:item:{}", item_id),
            *item_rect,
            sense,
            layer_id,
        );
    }
}

fn draw_info_tab(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_y: f64,
    state: &CompareSettingsState,
    toolbar_theme: &ToolbarTheme,
) {
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let bars_str = state.cached_bar_count.to_string();
    let base_str = format!("{:.4}", state.cached_base_price);

    let rows: [(&str, &str); 4] = [
        ("Символ",   state.cached_symbol.as_str()),
        ("Название", state.cached_name.as_str()),
        ("Баров",    bars_str.as_str()),
        ("База",     base_str.as_str()),
    ];

    for (i, (label, value)) in rows.iter().enumerate() {
        let row_y = content_y + 8.0 + i as f64 * ROW_H;
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.fill_text(label, content_x, row_y + ROW_H / 2.0);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.fill_text(value, content_x + LABEL_W, row_y + ROW_H / 2.0);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_footer(
    ctx: &mut dyn RenderContext,
    modal_x: f64,
    footer_y: f64,
    modal_w: f64,
    footer_h: f64,
    state: &CompareSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut CompareSettingsResult,
    templates: &[crate::templates::CompareTemplate],
) {
    // Separator above footer
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, footer_y);
    ctx.line_to(modal_x + modal_w, footer_y);
    ctx.stroke();

    // Single 52px row: "Шаблон" outline button left, "Отмена" + "OK" right.
    let button_height = 32.0;
    let button_y = footer_y + (footer_h - button_height) / 2.0;
    let button_padding = 12.0;

    // ── "Шаблон" button (left side, outline only) ────────────────────────────
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

    input_coordinator.register_on_layer(
        "cmp_settings:template_dropdown",
        WidgetRect::new(template_btn_x, button_y, template_btn_width, button_height),
        Sense::click(),
        layer_id,
    );

    // ── "OK" button (right side, filled #2962ff) ─────────────────────────────
    let ok_btn_width = 70.0;
    let cancel_btn_width = 80.0;
    let ok_btn_x = modal_x + modal_w - 16.0 - ok_btn_width;
    let cancel_btn_x = ok_btn_x - button_padding - cancel_btn_width;

    let is_ok_hovered = state.hovered_item_id.as_deref() == Some("ok");
    ctx.set_fill_color(if is_ok_hovered { "#4080ff" } else { "#2962ff" });
    ctx.fill_rect(ok_btn_x, button_y, ok_btn_width, button_height);
    ctx.set_fill_color("#ffffff");
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("OK", ok_btn_x + ok_btn_width / 2.0, button_y + button_height / 2.0);

    input_coordinator.register_on_layer(
        "cmp_settings:ok",
        WidgetRect::new(ok_btn_x, button_y, ok_btn_width, button_height),
        Sense::click(),
        layer_id,
    );

    // ── "Отмена" button (before OK, outline only) ────────────────────────────
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

    input_coordinator.register_on_layer(
        "cmp_settings:cancel",
        WidgetRect::new(cancel_btn_x, button_y, cancel_btn_width, button_height),
        Sense::click(),
        layer_id,
    );

    // ── Template dropdown menu (opens BELOW the "Шаблон" button) ─────────────
    if state.template_dropdown_open {
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

        // Register entire dropdown background to absorb clicks/hover (blocks crosshair)
        input_coordinator.register_on_layer(
            "cmp_settings:template_dropdown_menu",
            WidgetRect::new(dd_x, menu_y, menu_w, total_h),
            Sense::click(),
            layer_id,
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
        input_coordinator.register_on_layer(
            "cmp_settings:template_save_as",
            WidgetRect::new(dd_x, row_y, menu_w, opt_h),
            Sense::click(),
            layer_id,
        );
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
        input_coordinator.register_on_layer(
            "cmp_settings:template_default",
            WidgetRect::new(dd_x, row_y, menu_w, opt_h),
            Sense::click(),
            layer_id,
        );
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
                draw_svg_icon(ctx, Icon::Close.svg(), del_x, del_y, 24.0, 24.0, del_color);

                let name_w = menu_w - delete_btn_w - 8.0;
                input_coordinator.register_on_layer(
                    format!("cmp_settings:{}", row_id),
                    WidgetRect::new(dd_x, row_y, name_w, opt_h),
                    Sense::click(),
                    layer_id,
                );
                input_coordinator.register_on_layer(
                    format!("cmp_settings:{}", del_id),
                    WidgetRect::new(del_x, del_y, delete_btn_w, 24.0),
                    Sense::click(),
                    layer_id,
                );

                row_y += opt_h;
            }
        }

        let _ = is_sa_hovered;
        let _ = is_def_hovered;
    }

    let _ = result;
    let _ = button_padding;
}

