//! Chart settings modal renderer.
//!
//! Renders the chart settings modal with vertical icon tabs (Instrument, Status Line,
//! Scales & Lines, Appearance). Trading and Alerts tabs are intentionally excluded.

use crate::engine::render::RenderContext;
use crate::engine::render::draw_svg_icon;
use crate::i18n::{current_language, ClockKey, SettingsKey, t_settings};
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::{ChartSettingsModalResult, SliderTrackInfo};
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::modal_settings::{ChartSettingsState, ChartSettingsTab};
use crate::ui::widgets::{
    draw_input, draw_input_cursor, InputConfig, InputType,
    render_single_slider, SliderConfig,
    render_modal_frame_only, ModalTheme,
    WidgetState, WidgetTheme,
    ButtonConfig, draw_button,
};
use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig};
use crate::ui::dropdown::{render_dropdown, DropdownConfig, DropdownItem, DropdownTheme};
use crate::ui::Icon;
use crate::ui::z_order::ZLayer;
use crate::theme::{ThemeManager, ThemeSettingsPanel};
use uzor::types::Rect as WidgetRect;
use uzor::render::{TextAlign, TextBaseline};

// =============================================================================
// Data structs
// =============================================================================

/// Settings for chart instrument (candle colors, etc.)
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InstrumentSettings {
    /// Use previous close for bar color
    pub use_prev_close_color: bool,
    /// Body colors enabled
    pub body_enabled: bool,
    /// Body up color
    pub body_up_color: String,
    /// Body down color
    pub body_down_color: String,
    /// Border colors enabled
    pub border_enabled: bool,
    /// Border up color
    pub border_up_color: String,
    /// Border down color
    pub border_down_color: String,
    /// Wick colors enabled
    pub wick_enabled: bool,
    /// Wick up color
    pub wick_up_color: String,
    /// Wick down color
    pub wick_down_color: String,
    /// Precision display label (e.g. "Авто", "2 (0.00)")
    pub precision_label: String,
    /// Timezone display label (e.g., "(UTC+3) Москва")
    pub timezone_label: String,
    /// 24-hour format enabled
    pub use_24h: bool,
    /// Show UTC prefix in clock display
    pub show_utc_prefix: bool,
    /// Date format label
    pub date_format_label: String,
    /// Show day of week
    pub show_day_of_week: bool,
    /// Show countdown to bar close inside the last price label
    pub show_bar_countdown: bool,
    /// Price tick line style: "dotted", "dashed", "solid"
    pub price_tick_style: String,
    /// Extend price tick line to the right
    pub price_tick_extend_right: bool,
    /// Extend price tick line to the left
    pub price_tick_extend_left: bool,
}

/// Settings for scales and lines
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ScalesLinesSettings {
    /// Show grid
    pub show_grid: bool,
    /// Show vertical grid lines
    pub vert_lines: bool,
    /// Show horizontal grid lines
    pub horz_lines: bool,
    /// Show price scale on right
    pub price_scale_right: bool,
    /// Auto scale price
    pub auto_scale: bool,
    /// Show time scale on bottom
    pub time_scale_bottom: bool,
    /// Crosshair mode: "Normal", "Magnet", "Hidden"
    pub crosshair_mode: String,
    /// Crosshair line style: "Solid", "Dashed", "Dotted", "LargeDashed", "SparseDotted"
    pub crosshair_line_style: String,
    /// Crosshair line width (1.0 - 4.0)
    pub crosshair_line_width: f64,
    /// Crosshair line color (hex color)
    pub crosshair_line_color: String,
    /// Price scale position: "left", "right", "hidden"
    pub price_scale_position: String,
    /// Time scale position: "top", "bottom", "hidden"
    pub time_scale_position: String,
    /// Corner visibility: "always", "on_hover", "never"
    pub corner_visibility: String,
    /// Price scale width in pixels (50-150, default 70)
    pub price_scale_width: f64,
    /// Time scale height in pixels (20-60, default 30)
    pub time_scale_height: f64,
    /// Date format: "day_month_year", "month_day_year", "year_month_day", "day_month_short"
    pub date_format: String,
    /// Use 24-hour format (vs 12-hour AM/PM)
    pub use_24h: bool,
    /// Show day of week on time labels
    pub show_day_of_week: bool,
    /// Show countdown to bar close on price scale
    pub show_bar_countdown: bool,
    /// Show previous close price line
    pub show_prev_close: bool,
    /// Timezone display label (e.g., "(UTC+3) Москва")
    pub timezone_label: String,
}

/// Settings for status line elements
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StatusLineSettings {
    // Legend
    /// Legend position: "top_left", "top_right", "bottom_left", "bottom_right"
    pub legend_position: String,
    /// Show OHLC values in legend
    pub legend_show_ohlc: bool,
    /// Show absolute change in legend
    pub legend_show_change: bool,
    /// Show percentage change in legend
    pub legend_show_percent: bool,

    // Tooltip
    /// Tooltip visibility
    pub tooltip_visible: bool,
    /// Tooltip follows cursor (vs fixed position)
    pub tooltip_follow_cursor: bool,

    // Watermark
    /// Watermark visibility
    pub watermark_visible: bool,
    /// Watermark position: "top_left", "top_right", "bottom_left", "bottom_right", "center"
    pub watermark_position: String,
    /// Watermark color (hex color)
    pub watermark_color: String,
    /// Watermark text
    pub watermark_text: String,

    // Indicator Overlay
    /// Show indicator overlay panel
    pub show_indicator_overlay: bool,
}

/// Settings passed to chart settings modal
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ChartSettingsData {
    /// Instrument (candle) settings
    pub instrument: InstrumentSettings,
    /// Status line settings (Legend, Tooltip, Watermark, Indicator Overlay)
    pub status_line: StatusLineSettings,
    /// Scales and lines settings
    pub scales: ScalesLinesSettings,
}

// =============================================================================
// Main render function
// =============================================================================

/// Render chart settings modal — TradingView style with vertical icon tabs.
///
/// Tabs: Instrument, Status Line, Scales & Lines, Appearance.
/// Trading and Alerts tabs have been removed.
pub fn render_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    chart_area_x: f64,
    chart_area_y: f64,
    chart_settings_state: &ChartSettingsState,
    settings_data: &ChartSettingsData,
    theme_manager: &ThemeManager,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    templates: &[crate::templates::ChartTemplate],
) -> ChartSettingsModalResult {
    let modal_width = 620.0;
    let modal_height = 580.0;

    // Use saved position or center
    let (modal_x, modal_y) = chart_settings_state.position.unwrap_or_else(|| {
        let x = chart_area_x + (screen_w - chart_area_x - modal_width) / 2.0;
        let y = chart_area_y + (screen_h - chart_area_y - modal_height) / 2.0;
        (x, y)
    });

    // Clamp to screen bounds
    let modal_x = modal_x.max(0.0).min(screen_w - modal_width);
    let modal_y = modal_y.max(0.0).min(screen_h - modal_height);

    let mut result = ChartSettingsModalResult::default();

    // Layout constants
    let header_height = 44.0;
    let sidebar_width = 48.0;
    let footer_height = 52.0;
    let content_width = modal_width - sidebar_width;
    let content_height = modal_height - header_height - footer_height;

    // Store modal rect
    result.modal_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);
    result.header_rect = WidgetRect::new(modal_x, modal_y, modal_width, header_height);

    // === InputCoordinator Integration ===
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "chart_settings");

    // Register header drag zone
    input_coordinator.register_on_layer(
        "chart_settings:header",
        uzor::types::Rect::new(modal_x, modal_y, modal_width, header_height),
        uzor::input::Sense::DRAG,
        &layer_id,
    );

    // Register modal background catch-all
    input_coordinator.register_on_layer(
        "chart_settings:modal_bg",
        uzor::types::Rect::new(modal_x, modal_y, modal_width, modal_height),
        uzor::input::Sense::CLICK,
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
    let title = t_settings(SettingsKey::Title);
    let text_color = &toolbar_theme.item_text;
    ctx.set_font("14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, modal_x + 16.0, modal_y + header_height / 2.0);

    // Close button
    let close_size = 18.0;
    let close_x = modal_x + modal_width - close_size - 12.0;
    let close_y = modal_y + (header_height - close_size) / 2.0;

    result.close_btn_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, text_color);

    input_coordinator.register_on_layer(
        "chart_settings:close",
        uzor::types::Rect::new(close_x, close_y, close_size, close_size),
        uzor::input::Sense::CLICK,
        &layer_id,
    );

    // Header bottom border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + header_height);
    ctx.line_to(modal_x + modal_width, modal_y + header_height);
    ctx.stroke();

    // =========================================================================
    // Left sidebar (vertical icon tabs)
    // =========================================================================
    let sidebar_x = modal_x;
    let sidebar_y = modal_y + header_height;

    // Sidebar right border
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(sidebar_x + sidebar_width, sidebar_y);
    ctx.line_to(sidebar_x + sidebar_width, sidebar_y + content_height);
    ctx.stroke();

    // Determine active tab index from state
    let active_tab_idx = ChartSettingsTab::all().iter()
        .position(|t| *t == chart_settings_state.active_tab)
        .unwrap_or(0);

    let tab_icon_size = 20.0;
    let tab_button_height = 44.0;

    for (i, tab) in ChartSettingsTab::all().iter().enumerate() {
        let tab_y = sidebar_y + i as f64 * tab_button_height;
        let is_active = i == active_tab_idx;

        if is_active {
            ctx.draw_sidebar_active_item(
                sidebar_x, tab_y, sidebar_width, tab_button_height,
                &toolbar_theme.accent, &toolbar_theme.item_bg_active, 3.0
            );
        }

        let icon_x = sidebar_x + (sidebar_width - tab_icon_size) / 2.0;
        let icon_y = tab_y + (tab_button_height - tab_icon_size) / 2.0;
        let icon_color = if is_active { &toolbar_theme.item_text_active } else { text_color };

        let icon = match tab {
            ChartSettingsTab::Instrument => Icon::Candlestick,
            ChartSettingsTab::StatusLine => Icon::Legend,
            ChartSettingsTab::ScalesLines => Icon::Grid,
            ChartSettingsTab::Appearance => Icon::Palette,
        };
        draw_svg_icon(ctx, icon.svg(), icon_x, icon_y, tab_icon_size, tab_icon_size, icon_color);
    }

    // =========================================================================
    // Content area
    // =========================================================================
    let content_x = modal_x + sidebar_width;
    let content_y = modal_y + header_height;
    let content_padding = 20.0;

    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(content_x, content_y, content_width, content_height);

    // Tab title
    let active_tab = ChartSettingsTab::from_index(active_tab_idx).unwrap_or_default();
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(active_tab.label(), content_x + content_padding, content_y + content_padding);

    let settings_y = content_y + content_padding + 30.0;
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(text_color);

    match active_tab {
        ChartSettingsTab::Instrument => {
            render_instrument_settings(
                ctx,
                content_x + content_padding,
                settings_y,
                content_width - content_padding * 2.0,
                content_height - 30.0 - content_padding,
                &settings_data.instrument,
                toolbar_theme,
                chart_settings_state,
                &mut result,
            );
        }
        ChartSettingsTab::StatusLine => {
            render_status_line_settings(
                ctx,
                content_x + content_padding,
                settings_y,
                content_width - content_padding * 2.0,
                content_height - 30.0 - content_padding,
                &settings_data.status_line,
                toolbar_theme,
                frame_theme,
                chart_settings_state,
                &mut result,
                current_time_ms,
            );
        }
        ChartSettingsTab::Appearance => {
            let current_time_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);

            render_appearance_settings(
                ctx,
                content_x + content_padding,
                settings_y,
                content_width - content_padding * 2.0,
                content_height - 30.0 - content_padding,
                theme_manager,
                toolbar_theme,
                frame_theme,
                chart_settings_state,
                &mut result,
                current_time_ms,
            );
        }
        ChartSettingsTab::ScalesLines => {
            render_scales_settings(
                ctx,
                content_x + content_padding,
                settings_y,
                content_width - content_padding * 2.0,
                content_height - 30.0 - content_padding,
                &settings_data.scales,
                toolbar_theme,
                frame_theme,
                chart_settings_state,
                &mut result,
                current_time_ms,
            );
        }
    }

    // =========================================================================
    // Footer
    // =========================================================================
    let footer_y = modal_y + modal_height - footer_height;

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.begin_path();
    ctx.move_to(modal_x, footer_y);
    ctx.line_to(modal_x + modal_width, footer_y);
    ctx.stroke();

    let button_height = 32.0;
    let button_y = footer_y + (footer_height - button_height) / 2.0;
    let button_padding = 12.0;

    // "Шаблон" button (left side)
    let template_btn_width = 80.0;
    let template_btn_x = modal_x + 16.0;
    let is_tmpl_hovered = chart_settings_state.hovered_footer_button.as_deref() == Some("template");
    ctx.set_stroke_color(if is_tmpl_hovered { &toolbar_theme.item_text } else { &toolbar_theme.separator });
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(template_btn_x, button_y, template_btn_width, button_height);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(if is_tmpl_hovered { &toolbar_theme.item_text_hover } else { text_color });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::ButtonTemplate), template_btn_x + template_btn_width / 2.0, button_y + button_height / 2.0);

    // Right side buttons
    let ok_btn_width = 70.0;
    let cancel_btn_width = 80.0;

    let ok_btn_x = modal_x + modal_width - 16.0 - ok_btn_width;
    let cancel_btn_x = ok_btn_x - button_padding - cancel_btn_width;

    // "OK" button
    let is_ok_hovered = chart_settings_state.hovered_footer_button.as_deref() == Some("ok");
    ctx.set_fill_color(if is_ok_hovered { "#4080ff" } else { "#2962ff" });
    ctx.fill_rect(ok_btn_x, button_y, ok_btn_width, button_height);
    ctx.set_fill_color("#ffffff");
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::ButtonOk), ok_btn_x + ok_btn_width / 2.0, button_y + button_height / 2.0);

    // "Отмена" button
    let is_cancel_hovered = chart_settings_state.hovered_footer_button.as_deref() == Some("cancel");
    if is_cancel_hovered {
        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
        ctx.fill_rect(cancel_btn_x, button_y, cancel_btn_width, button_height);
    }
    ctx.set_stroke_color(if is_cancel_hovered { &toolbar_theme.item_text } else { &toolbar_theme.separator });
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(cancel_btn_x, button_y, cancel_btn_width, button_height);
    ctx.set_fill_color(text_color);
    ctx.set_font("13px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::ButtonCancel), cancel_btn_x + cancel_btn_width / 2.0, button_y + button_height / 2.0);

    // Populate result
    result.modal_rect = WidgetRect::new(modal_x, modal_y, modal_width, modal_height);
    result.header_rect = WidgetRect::new(modal_x, modal_y, modal_width, header_height);
    result.close_btn_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    result.content_rect = WidgetRect::new(content_x, content_y, content_width, content_height);

    // Phase 6.1a: register scrollable content area for coordinator dispatch
    input_coordinator.register_on_layer(
        "chart_settings:scroll_viewport",
        uzor::types::Rect::new(content_x, content_y, content_width, content_height),
        uzor::input::Sense::SCROLL,
        &layer_id,
    );

    // Tab rects
    for (i, tab) in ChartSettingsTab::all().iter().enumerate() {
        let tab_y = sidebar_y + i as f64 * tab_button_height;
        let tab_rect = WidgetRect::new(sidebar_x, tab_y, sidebar_width, tab_button_height);
        result.tab_rects.push((tab.id().to_string(), tab_rect));

        input_coordinator.register_on_layer(
            format!("chart_settings:tab:{}", tab.id()),
            uzor::types::Rect::new(tab_rect.x, tab_rect.y, tab_rect.width, tab_rect.height),
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            &layer_id,
        );
    }

    // Footer buttons
    let footer_btns = [
        ("template", template_btn_x, template_btn_width),
        ("cancel", cancel_btn_x, cancel_btn_width),
        ("ok", ok_btn_x, ok_btn_width),
    ];
    for (btn_id, btn_x, btn_width) in &footer_btns {
        let btn_rect = WidgetRect::new(*btn_x, button_y, *btn_width, button_height);
        result.footer_buttons.push((btn_id.to_string(), btn_rect));

        input_coordinator.register_on_layer(
            format!("chart_settings:footer:{}", btn_id),
            uzor::types::Rect::new(btn_rect.x, btn_rect.y, btn_rect.width, btn_rect.height),
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            &layer_id,
        );
    }

    // ── Template dropdown menu (opens BELOW the "Шаблон" button) ─────────────
    if chart_settings_state.template_dropdown_open {
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

        // Register dropdown background FIRST with coordinator (last-registered wins in
        // hit_test_at, so items registered AFTER will override this background rect)
        input_coordinator.register_on_layer(
            "chart_settings:footer:template_dropdown_menu",
            uzor::types::Rect::new(dd_x, menu_y, menu_w, total_h),
            uzor::input::Sense::CLICK,
            &layer_id,
        );

        let mut row_y = menu_y + 3.0;

        // Row 1: "Сохранить как..."
        let is_sa_hovered = chart_settings_state.hovered_footer_button.as_deref() == Some("template_save_as");
        if is_sa_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
        }
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(t_settings(SettingsKey::SaveAsTemplate), dd_x + 8.0, row_y + opt_h / 2.0);
        {
            let rect = WidgetRect::new(dd_x, row_y, menu_w, opt_h);
            result.footer_buttons.push(("template_save_as".to_string(), rect));
            input_coordinator.register_on_layer(
                "chart_settings:footer:template_save_as",
                uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
                uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
                &layer_id,
            );
        }
        row_y += opt_h;

        // Row 2: "Применить по умолчанию"
        let is_def_hovered = chart_settings_state.hovered_footer_button.as_deref() == Some("template_default");
        if is_def_hovered {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
        }
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(t_settings(SettingsKey::ApplyDefault), dd_x + 8.0, row_y + opt_h / 2.0);
        {
            let rect = WidgetRect::new(dd_x, row_y, menu_w, opt_h);
            result.footer_buttons.push(("template_default".to_string(), rect));
            input_coordinator.register_on_layer(
                "chart_settings:footer:template_default",
                uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
                uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
                &layer_id,
            );
        }
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
            ctx.fill_text(t_settings(SettingsKey::NoTemplates), dd_x + 8.0, row_y + opt_h / 2.0);
        } else {
            for tmpl in templates {
                let row_id = format!("template_option:{}", tmpl.id);
                let del_id = format!("template_delete:{}", tmpl.id);
                let is_row_hovered = chart_settings_state.hovered_footer_button.as_deref() == Some(row_id.as_str());
                let is_del_hovered = chart_settings_state.hovered_footer_button.as_deref() == Some(del_id.as_str());

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
                {
                    let row_rect = WidgetRect::new(dd_x, row_y, name_w, opt_h);
                    let del_rect = WidgetRect::new(del_x, del_y, delete_btn_w, 24.0);
                    result.footer_buttons.push((row_id.clone(), row_rect));
                    result.footer_buttons.push((del_id.clone(), del_rect));
                    input_coordinator.register_on_layer(
                        format!("chart_settings:footer:{}", row_id),
                        uzor::types::Rect::new(row_rect.x, row_rect.y, row_rect.width, row_rect.height),
                        uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
                        &layer_id,
                    );
                    input_coordinator.register_on_layer(
                        format!("chart_settings:footer:{}", del_id),
                        uzor::types::Rect::new(del_rect.x, del_rect.y, del_rect.width, del_rect.height),
                        uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
                        &layer_id,
                    );
                }

                row_y += opt_h;
            }
        }

        // Push background to footer_buttons LAST (for hover: first-match wins in Vec scan)
        result.footer_buttons.push(("template_dropdown_menu".to_string(), WidgetRect::new(dd_x, menu_y, menu_w, total_h)));

        let _ = is_sa_hovered;
        let _ = is_def_hovered;
    }

    // =========================================================================
    // Centralized Dropdown Rendering (render AFTER footer to appear on top)
    // =========================================================================
    if let Some(active_field) = &chart_settings_state.active_dropdown {
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

        let dropdown_id = match active_tab {
            ChartSettingsTab::StatusLine => format!("status:dropdown_menu:{}", active_field),
            _ => format!("dropdown_menu:{}", active_field),
        };

        if let Some((_, button_rect)) = result.content_items.iter().find(|(id, _)| id == &dropdown_id) {
            let dropdown_x = button_rect.x;
            let dropdown_y = button_rect.y + button_rect.height + 2.0;
            let button_width = button_rect.width;

            let items: Option<Vec<DropdownItem>> = match active_tab {
                ChartSettingsTab::Instrument => {
                    match active_field.as_str() {
                        "precision" => Some(vec![
                            DropdownItem::item("auto", t_settings(SettingsKey::PrecisionAuto)),
                            DropdownItem::item("0", "0 (1)"),
                            DropdownItem::item("1", "1 (0.1)"),
                            DropdownItem::item("2", "2 (0.01)"),
                            DropdownItem::item("3", "3 (0.001)"),
                            DropdownItem::item("4", "4 (0.0001)"),
                            DropdownItem::item("5", "5 (0.00001)"),
                            DropdownItem::item("6", "6 (0.000001)"),
                            DropdownItem::item("7", "7 (0.0000001)"),
                            DropdownItem::item("8", "8 (0.00000001)"),
                        ]),
                        "timezone" => Some(vec![
                            DropdownItem::item("utc", t_settings(SettingsKey::TimezoneUtc)),
                            DropdownItem::item("europe_moscow", t_settings(SettingsKey::TimezoneMoscow)),
                            DropdownItem::item("europe_london", t_settings(SettingsKey::TimezoneLondon)),
                            DropdownItem::item("america_new_york", t_settings(SettingsKey::TimezoneNewYork)),
                            DropdownItem::item("america_chicago", t_settings(SettingsKey::TimezoneChicago)),
                            DropdownItem::item("america_los_angeles", t_settings(SettingsKey::TimezoneLosAngeles)),
                            DropdownItem::item("asia_tokyo", t_settings(SettingsKey::TimezoneTokyo)),
                            DropdownItem::item("asia_hong_kong", t_settings(SettingsKey::TimezoneHongKong)),
                            DropdownItem::item("asia_singapore", t_settings(SettingsKey::TimezoneSingapore)),
                            DropdownItem::item("australia_sydney", t_settings(SettingsKey::TimezoneSydney)),
                        ]),
                        _ => None,
                    }
                }
                ChartSettingsTab::ScalesLines => {
                    match active_field.as_str() {
                        "crosshair_mode" => Some(vec![
                            DropdownItem::item("Normal", t_settings(SettingsKey::CrosshairNormal)),
                            DropdownItem::item("Magnet", t_settings(SettingsKey::CrosshairMagnetStrong)),
                            DropdownItem::item("MagnetOHLC", t_settings(SettingsKey::CrosshairMagnetLight)),
                            DropdownItem::item("Hidden", t_settings(SettingsKey::CrosshairHidden)),
                        ]),
                        "crosshair_line_style" => Some(vec![
                            DropdownItem::item("Solid", t_settings(SettingsKey::LineStyleSolid)),
                            DropdownItem::item("Dashed", t_settings(SettingsKey::LineStyleDashed)),
                            DropdownItem::item("Dotted", t_settings(SettingsKey::LineStyleDotted)),
                            DropdownItem::item("LargeDashed", t_settings(SettingsKey::LineStyleLargeDashed)),
                            DropdownItem::item("SparseDotted", t_settings(SettingsKey::LineStyleSparseDotted)),
                        ]),
                        "price_position" => Some(vec![
                            DropdownItem::item("left", t_settings(SettingsKey::ScalePosLeft)),
                            DropdownItem::item("right", t_settings(SettingsKey::ScalePosRight)),
                            DropdownItem::item("hidden", t_settings(SettingsKey::ScalePosHidden)),
                        ]),
                        "time_position" => Some(vec![
                            DropdownItem::item("top", t_settings(SettingsKey::ScalePosTop)),
                            DropdownItem::item("bottom", t_settings(SettingsKey::ScalePosBottom)),
                            DropdownItem::item("hidden", t_settings(SettingsKey::ScalePosHidden)),
                        ]),
                        "corner_visibility" => Some(vec![
                            DropdownItem::item("always", t_settings(SettingsKey::CornerAlways)),
                            DropdownItem::item("never", t_settings(SettingsKey::CornerNever)),
                        ]),
                        "date_format" => Some(vec![
                            DropdownItem::item("day_month_year", "ДД.ММ.ГГГГ (26.01.2025)"),
                            DropdownItem::item("month_day_year", "ММ/ДД/ГГГГ (01/26/2025)"),
                            DropdownItem::item("year_month_day", "ГГГГ-ММ-ДД (2025-01-26)"),
                            DropdownItem::item("day_month_short", "ДД МММ (26 янв)"),
                        ]),
                        _ => None,
                    }
                }
                ChartSettingsTab::StatusLine => {
                    match active_field.as_str() {
                        "legend_position" => Some(vec![
                            DropdownItem::item("top_left", t_settings(SettingsKey::LegendTopLeft)),
                            DropdownItem::item("top_right", t_settings(SettingsKey::LegendTopRight)),
                            DropdownItem::item("bottom_left", t_settings(SettingsKey::LegendBottomLeft)),
                            DropdownItem::item("bottom_right", t_settings(SettingsKey::LegendBottomRight)),
                        ]),
                        "watermark_position" => Some(vec![
                            DropdownItem::item("top_left", t_settings(SettingsKey::LegendTopLeft)),
                            DropdownItem::item("top_right", t_settings(SettingsKey::LegendTopRight)),
                            DropdownItem::item("bottom_left", t_settings(SettingsKey::LegendBottomLeft)),
                            DropdownItem::item("bottom_right", t_settings(SettingsKey::LegendBottomRight)),
                            DropdownItem::item("center", t_settings(SettingsKey::LegendCenter)),
                        ]),
                        _ => None,
                    }
                }
                _ => None,
            };

            if let Some(items) = items {
                let mut config = DropdownConfig::new(items);
                config.min_width = button_width;
                config.item_height = 28.0;

                let dropdown_result = render_dropdown(
                    ctx,
                    &config,
                    (dropdown_x, dropdown_y),
                    &dropdown_theme,
                    chart_settings_state.hovered_item_id.as_deref(),
                    |_ctx, _icon, _rect, _color| {},
                );

                for (item_id, item_rect) in &dropdown_result.item_rects {
                    result.content_items.push((
                        format!("dropdown_option:{}:{}", active_field, item_id),
                        WidgetRect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
                    ));
                }
            }
        }
    }

    // Register all content items
    for (item_id, item_rect) in &result.content_items {
        input_coordinator.register_on_layer(
            format!("chart_settings:item:{}", item_id),
            uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            &layer_id,
        );
    }

    // Register scrollbar handle (inflated ±5px X) for DRAG, track for CLICK
    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "chart_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            &layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "chart_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    input_coordinator.pop_layer(&layer_id);

    result
}

// =============================================================================
// Instrument tab
// =============================================================================

fn render_instrument_settings(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    width: f64,
    content_height: f64,
    settings: &InstrumentSettings,
    theme: &ToolbarTheme,
    chart_settings_state: &ChartSettingsState,
    result: &mut ChartSettingsModalResult,
) {
    let row_height = 32.0;
    let checkbox_size = 16.0;
    let swatch_size = 24.0;
    let swatch_gap = 8.0;
    let label_x_offset = checkbox_size + 12.0;
    let swatches_x_offset = 140.0;

    let text_color = &theme.item_text;
    let muted_color = &theme.item_text_muted;

    let scrollbar_width = 8.0;
    let viewport_height = content_height;
    let viewport_y = y;

    let section_count = 3.0;
    let item_count = 13.0;
    let gap_count = 2.0;
    let total_content_height = (section_count * row_height) + (item_count * row_height) + (gap_count * 12.0);

    result.total_content_height = total_content_height;
    result.viewport_height = viewport_height;

    let viewport_rect = WidgetRect::new(x, viewport_y, width, viewport_height);
    let scroll_config = ScrollableConfig {
        scrollbar_width,
        scrollbar_padding: 4.0,
        always_show_scrollbar: false,
    };
    let scrollable = ScrollableContainer::new(viewport_rect, &chart_settings_state.scroll, Some(scroll_config));
    scrollable.begin(ctx);

    let content_start_y = scrollable.content_y();

    let label_x = x + label_x_offset;
    let swatches_x = x + swatches_x_offset;

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    // Section: JAPANESE CANDLES
    let mut row_y = content_start_y;
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionCandles), x, row_y + row_height / 2.0);
    row_y += row_height;

    // Color swatch helper closure
    let draw_swatch = |ctx: &mut dyn RenderContext, sx: f64, sy: f64, color: &str, theme: &ToolbarTheme| -> WidgetRect {
        let swatch_rect = WidgetRect::new(sx, sy, swatch_size, swatch_size);
        ctx.set_fill_color(color);
        ctx.fill_rounded_rect(swatch_rect.x, swatch_rect.y, swatch_rect.width, swatch_rect.height, 4.0);
        ctx.set_stroke_color(&theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(swatch_rect.x, swatch_rect.y, swatch_rect.width, swatch_rect.height, 4.0);
        swatch_rect
    };

    // Row: use_prev_close checkbox
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.use_prev_close_color, theme);
    result.content_items.push(("instrument:use_prev_close".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::BodyColorPrevClose), label_x, row_y + row_height / 2.0);
    row_y += row_height + 8.0;

    // Row: body
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.body_enabled, theme);
    result.content_items.push(("instrument:body_enabled".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Body), label_x, row_y + row_height / 2.0);

    let swatch_y = row_y + (row_height - swatch_size) / 2.0;
    let body_up_rect = draw_swatch(ctx, swatches_x, swatch_y, &settings.body_up_color, theme);
    let body_down_rect = draw_swatch(ctx, swatches_x + swatch_size + swatch_gap, swatch_y, &settings.body_down_color, theme);
    result.content_items.push(("instrument:body_up_color".to_string(), body_up_rect));
    result.content_items.push(("instrument:body_down_color".to_string(), body_down_rect));
    row_y += row_height;

    // Row: border
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.border_enabled, theme);
    result.content_items.push(("instrument:border_enabled".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Borders), label_x, row_y + row_height / 2.0);

    let swatch_y = row_y + (row_height - swatch_size) / 2.0;
    let border_up_rect = draw_swatch(ctx, swatches_x, swatch_y, &settings.border_up_color, theme);
    let border_down_rect = draw_swatch(ctx, swatches_x + swatch_size + swatch_gap, swatch_y, &settings.border_down_color, theme);
    result.content_items.push(("instrument:border_up_color".to_string(), border_up_rect));
    result.content_items.push(("instrument:border_down_color".to_string(), border_down_rect));
    row_y += row_height;

    // Row: wick
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.wick_enabled, theme);
    result.content_items.push(("instrument:wick_enabled".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Wick), label_x, row_y + row_height / 2.0);

    let swatch_y = row_y + (row_height - swatch_size) / 2.0;
    let wick_up_rect = draw_swatch(ctx, swatches_x, swatch_y, &settings.wick_up_color, theme);
    let wick_down_rect = draw_swatch(ctx, swatches_x + swatch_size + swatch_gap, swatch_y, &settings.wick_down_color, theme);
    result.content_items.push(("instrument:wick_up_color".to_string(), wick_up_rect));
    result.content_items.push(("instrument:wick_down_color".to_string(), wick_down_rect));
    row_y += row_height + 16.0;

    // Section: DATA SETTINGS
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionDataConfig), x, row_y + row_height / 2.0);
    row_y += row_height;

    // Row: show_bar_countdown
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.show_bar_countdown, theme);
    result.content_items.push(("instrument:show_bar_countdown".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::CountdownToClose), x + checkbox_size + 12.0, row_y + row_height / 2.0);
    row_y += row_height;

    // Section: CURRENT PRICE TICK
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionPriceTick), x, row_y + row_height / 2.0);
    row_y += row_height;

    // Row: price_tick_extend_right
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.price_tick_extend_right, theme);
    result.content_items.push(("instrument:price_tick_extend_right".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::ExtendRight), x + checkbox_size + 12.0, row_y + row_height / 2.0);
    row_y += row_height;

    // Row: price_tick_extend_left
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.price_tick_extend_left, theme);
    result.content_items.push(("instrument:price_tick_extend_left".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::ExtendLeft), x + checkbox_size + 12.0, row_y + row_height / 2.0);
    row_y += row_height;

    // Row: price_tick_style dropdown
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::LineStyle), x, row_y + row_height / 2.0);

    let dropdown_x = x + 140.0;
    let dropdown_width = 140.0;
    let dropdown_height = 28.0;
    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    let tick_style_label = match settings.price_tick_style.as_str() {
        "dashed" => t_settings(SettingsKey::TickStyleDash),
        "solid"  => t_settings(SettingsKey::TickStyleLine),
        _        => t_settings(SettingsKey::TickStyleDots),
    };
    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, tick_style_label, row_y + row_height / 2.0, theme);
    let chevron_w = 20.0;
    let text_w = dropdown_width - chevron_w;
    result.content_items.push(("dropdown_cycle:price_tick_style".to_string(), WidgetRect::new(dropdown_x, dropdown_y, text_w, dropdown_height)));
    result.content_items.push(("dropdown_menu:price_tick_style".to_string(), WidgetRect::new(dropdown_x + text_w, dropdown_y, chevron_w, dropdown_height)));
    row_y += row_height;

    // Precision dropdown
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Precision), x, row_y + row_height / 2.0);

    let dropdown_x = x + 140.0;
    let dropdown_width = 140.0;
    let dropdown_height = 28.0;
    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;

    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, &settings.precision_label, row_y + row_height / 2.0, theme);
    let chevron_w = 20.0;
    let text_w = dropdown_width - chevron_w;
    result.content_items.push(("dropdown_cycle:precision".to_string(), WidgetRect::new(dropdown_x, dropdown_y, text_w, dropdown_height)));
    result.content_items.push(("dropdown_menu:precision".to_string(), WidgetRect::new(dropdown_x + text_w, dropdown_y, chevron_w, dropdown_height)));
    row_y += row_height;

    // Timezone dropdown
    ctx.set_text_align(TextAlign::Left);
    ctx.set_fill_color(text_color);
    ctx.fill_text(ClockKey::Timezone.get(current_language()), x, row_y + row_height / 2.0);

    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, &settings.timezone_label, row_y + row_height / 2.0, theme);
    result.content_items.push(("dropdown_cycle:timezone".to_string(), WidgetRect::new(dropdown_x, dropdown_y, text_w, dropdown_height)));
    result.content_items.push(("dropdown_menu:timezone".to_string(), WidgetRect::new(dropdown_x + text_w, dropdown_y, chevron_w, dropdown_height)));
    row_y += row_height;

    // 24-hour format checkbox
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.use_24h, theme);
    result.content_items.push(("instrument:use_24h".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ClockKey::Use24h.get(current_language()), x + checkbox_size + 12.0, row_y + row_height / 2.0);
    row_y += row_height;

    // show_utc_prefix checkbox
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.show_utc_prefix, theme);
    result.content_items.push(("instrument:show_utc_prefix".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ClockKey::ShowUtcPrefix.get(current_language()), x + checkbox_size + 12.0, row_y + row_height / 2.0);
    row_y += row_height;

    // Date format dropdown
    ctx.set_fill_color(text_color);
    ctx.fill_text(ClockKey::DateFormat.get(current_language()), x, row_y + row_height / 2.0);

    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, &settings.date_format_label, row_y + row_height / 2.0, theme);
    result.content_items.push(("instrument:date_format_cycle".to_string(), WidgetRect::new(dropdown_x, dropdown_y, text_w, dropdown_height)));
    result.content_items.push(("instrument:date_format_menu".to_string(), WidgetRect::new(dropdown_x + text_w, dropdown_y, chevron_w, dropdown_height)));
    row_y += row_height;

    // Show day of week checkbox
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.show_day_of_week, theme);
    result.content_items.push(("instrument:show_day_of_week".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(ClockKey::DayOfWeek.get(current_language()), x + checkbox_size + 12.0, row_y + row_height / 2.0);

    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
}

// =============================================================================
// Appearance tab
// =============================================================================

fn render_appearance_settings(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    width: f64,
    content_height: f64,
    theme_manager: &ThemeManager,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    chart_settings_state: &ChartSettingsState,
    result: &mut ChartSettingsModalResult,
    _current_time_ms: u64,
) {
    let row_height = 28.0;
    let section_margin = 12.0;
    let swatch_size = 18.0;

    let text_color = &toolbar_theme.item_text;
    let muted_color = &toolbar_theme.item_text_muted;
    let current_runtime = theme_manager.current();

    let scrollbar_width = 8.0;
    let viewport_height = content_height;
    let viewport_y = y;

    let mut total_items = 5.0;
    let mut total_sections = 1.0;
    for section in ThemeSettingsPanel::SECTIONS {
        total_sections += 1.0;
        total_items += section.fields.len() as f64;
    }
    let button_height = 26.0;
    let button_spacing = 4.0;
    let theme_buttons_height = 5.0 * (button_height + button_spacing);
    let style_buttons_height = 6.0 * (button_height + button_spacing);
    let slider_height = 32.0;
    let style_sliders_height = 8.0 * slider_height;
    let total_content_height =
        (total_sections * row_height) +
        row_height +
        row_height +
        theme_buttons_height +
        style_buttons_height +
        style_sliders_height +
        (total_items - 5.0) * row_height +
        (total_sections * section_margin / 2.0) +
        section_margin * 2.0;

    result.total_content_height = total_content_height;
    result.viewport_height = viewport_height;

    let viewport_rect = WidgetRect::new(x, viewport_y, width, viewport_height);
    let scroll_config = ScrollableConfig {
        scrollbar_width,
        scrollbar_padding: 4.0,
        always_show_scrollbar: false,
    };
    let scrollable = ScrollableContainer::new(viewport_rect, &chart_settings_state.scroll, Some(scroll_config));
    scrollable.begin(ctx);

    let content_start_y = scrollable.content_y();
    let swatch_x = x + width - scrollbar_width - swatch_size - 8.0;
    let mut row_y = content_start_y;

    // Theme presets
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(muted_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::SectionPresets), x, row_y + row_height / 2.0);
    row_y += row_height;

    let themes = [
        ("dark", t_settings(SettingsKey::ThemeDark), "#1e222d"),
        ("light", t_settings(SettingsKey::ThemeLight), "#ffffff"),
        ("high_contrast", t_settings(SettingsKey::ThemeHighContrast), "#000000"),
        ("high_contrast_mono", t_settings(SettingsKey::ThemeHighContrastMono), "#000000"),
        ("mascot", t_settings(SettingsKey::ThemeWizardHat), "#0a0f1a"),
    ];
    let current_theme = theme_manager.preset_name();
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

    for (id, name, preview_color) in themes.iter() {
        let button_rect = WidgetRect::new(x, row_y, width, button_height);
        let is_active = current_theme == *id;

        let config = ButtonConfig {
            text: None,
            icon: None,
            active: is_active,
            disabled: false,
            radius: 3.0,
            padding_x: 8.0,
            padding_y: 4.0,
            icon_size: 16.0,
            font_size: 12.0,
            gap: 6.0,
            active_border: false,
        };
        draw_button(ctx, &config, WidgetState::Normal, button_rect, &widget_theme, |_, _, _, _| {});

        ctx.set_fill_color(preview_color);
        ctx.fill_rect(button_rect.x + 6.0, button_rect.y + 4.0, 18.0, 18.0);
        ctx.set_stroke_color(muted_color);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(button_rect.x + 6.0, button_rect.y + 4.0, 18.0, 18.0);

        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(if is_active { &toolbar_theme.item_text_active } else { text_color });
        ctx.fill_text(name, button_rect.x + 30.0, button_rect.center_y());

        result.content_items.push((format!("appearance:theme_{}", id), button_rect));
        row_y += button_height + button_spacing;
    }

    row_y += section_margin;

    // UI Style section
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(muted_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::SectionStyle), x, row_y + row_height / 2.0);
    row_y += row_height;

    use crate::theme::UIStyle;
    let styles = UIStyle::all();
    let current_style = current_runtime.style;
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

    for style in styles {
        let button_rect = WidgetRect::new(x, row_y, width, button_height);
        let is_active = current_style == *style;

        let config = ButtonConfig::text(style.label())
            .with_active(is_active)
            .with_radius(3.0);
        draw_button(ctx, &config, WidgetState::Normal, button_rect, &widget_theme, |_, _, _, _| {});

        result.content_items.push((format!("appearance:ui_style:{}", style.index()), button_rect));
        row_y += button_height + button_spacing;
    }

    row_y += section_margin;

    // Style Parameters section
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(muted_color);
    ctx.fill_text(t_settings(SettingsKey::SectionStyleSettings), x, row_y + row_height / 2.0);
    row_y += row_height;

    let slider_height = 32.0;
    let slider_width_total = 300.0;

    let opacity_sliders = [
        (t_settings(SettingsKey::ToolbarOpacity), "toolbar_opacity", current_runtime.style_params.toolbar_bg_opacity),
        (t_settings(SettingsKey::ModalOpacity), "modal_opacity", current_runtime.style_params.modal_bg_opacity),
        (t_settings(SettingsKey::SidebarOpacity), "sidebar_opacity", current_runtime.style_params.sidebar_bg_opacity),
        (t_settings(SettingsKey::MenuOpacity), "menu_opacity", current_runtime.style_params.menu_bg_opacity),
        (t_settings(SettingsKey::ScaleOpacity), "scale_opacity", current_runtime.style_params.scale_bg_opacity),
        (t_settings(SettingsKey::HoverOpacity), "hover_opacity", current_runtime.style_params.hover_bg_opacity),
        (t_settings(SettingsKey::CrosshairLabelOpacity), "crosshair_label_opacity", current_runtime.style_params.crosshair_label_bg_opacity),
    ];

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let slider_config = SliderConfig::new(0.0, 1.0).with_step(0.01);

    for (label, field_id, value) in opacity_sliders.iter() {
        let slider_rect = WidgetRect::new(x, row_y, slider_width_total, slider_height);

        let value_field_id = format!("appearance:style_value_{}", field_id);
        let _is_editing = chart_settings_state.text_input.is_editing(&value_field_id);
        let slider_track_id = format!("appearance:style_{}", field_id);
        let hovered = chart_settings_state.slider_drag.as_ref()
            .map(|d| d.field_id == slider_track_id)
            .unwrap_or_else(|| chart_settings_state.hovered_item_id.as_deref() == Some(slider_track_id.as_str()));

        let display_value = chart_settings_state.slider_drag.as_ref()
            .filter(|d| d.field_id == slider_track_id)
            .and_then(|d| d.floating_value)
            .unwrap_or(*value as f64);

        let slider_result = render_single_slider(
            ctx,
            &slider_config,
            display_value,
            slider_rect,
            label,
            &widget_theme,
            hovered,
            None,
        );

        result.content_items.push((slider_track_id.clone(), slider_result.full_rect));
        if let Some(input_rect) = slider_result.input_rect {
            result.content_items.push((value_field_id, input_rect));
        }
        if let Some(widget_track_info) = slider_result.track_info {
            result.slider_tracks.push(SliderTrackInfo {
                field_id: slider_track_id,
                track_x: widget_track_info.track_x,
                track_width: widget_track_info.track_width,
                min_val: widget_track_info.min_val,
                max_val: widget_track_info.max_val,
            });
        }

        row_y += slider_height;
    }

    // Blur radius slider (only for Glass styles)
    if current_runtime.style.has_blur() {
        let slider_rect = WidgetRect::new(x, row_y, slider_width_total, slider_height);
        let blur_config = SliderConfig::new(0.0, 24.0).with_step(0.5);

        let value_field_id = "appearance:style_value_blur_radius";
        let slider_track_id = "appearance:style_blur_radius";
        let _is_editing = chart_settings_state.text_input.is_editing(value_field_id);
        let hovered = chart_settings_state.slider_drag.as_ref()
            .map(|d| d.field_id == slider_track_id)
            .unwrap_or_else(|| chart_settings_state.hovered_item_id.as_deref() == Some(slider_track_id));

        let blur_display = chart_settings_state.slider_drag.as_ref()
            .filter(|d| d.field_id == slider_track_id)
            .and_then(|d| d.floating_value)
            .unwrap_or(current_runtime.style_params.blur_radius as f64);

        let slider_result = render_single_slider(
            ctx,
            &blur_config,
            blur_display,
            slider_rect,
            t_settings(SettingsKey::BlurRadius),
            &widget_theme,
            hovered,
            None,
        );

        result.content_items.push((slider_track_id.to_string(), slider_result.full_rect));
        if let Some(input_rect) = slider_result.input_rect {
            result.content_items.push((value_field_id.to_string(), input_rect));
        }
        if let Some(widget_track_info) = slider_result.track_info {
            result.slider_tracks.push(SliderTrackInfo {
                field_id: slider_track_id.to_string(),
                track_x: widget_track_info.track_x,
                track_width: widget_track_info.track_width,
                min_val: widget_track_info.min_val,
                max_val: widget_track_info.max_val,
            });
        }

        row_y += slider_height;
    }

    row_y += section_margin;

    // Color sections from ThemeSettingsPanel
    for section in ThemeSettingsPanel::SECTIONS {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(muted_color);
        ctx.fill_text(&section.title.to_uppercase(), x, row_y + row_height / 2.0);
        row_y += row_height;

        ctx.set_font("12px sans-serif");
        for field in section.fields {
            let color_value = field.path.get(current_runtime);

            ctx.set_fill_color(text_color);
            ctx.fill_text(field.label, x + 4.0, row_y + row_height / 2.0);

            let swatch_y = row_y + (row_height - swatch_size) / 2.0;
            let swatch_rect = WidgetRect::new(swatch_x, swatch_y, swatch_size, swatch_size);

            // Checkerboard for transparency
            ctx.set_fill_color("#ffffff");
            ctx.fill_rect(swatch_x, swatch_y, swatch_size, swatch_size);
            ctx.set_fill_color("#cccccc");
            ctx.fill_rect(swatch_x, swatch_y, swatch_size / 2.0, swatch_size / 2.0);
            ctx.fill_rect(swatch_x + swatch_size / 2.0, swatch_y + swatch_size / 2.0, swatch_size / 2.0, swatch_size / 2.0);

            ctx.set_fill_color(color_value);
            ctx.fill_rect(swatch_x, swatch_y, swatch_size, swatch_size);

            ctx.set_stroke_color(muted_color);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rect(swatch_x, swatch_y, swatch_size, swatch_size);

            result.content_items.push((format!("appearance:{}", field.id), swatch_rect));
            row_y += row_height;
        }

        row_y += section_margin / 2.0;
    }

    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
}

// =============================================================================
// Scales & Lines tab
// =============================================================================

fn render_scales_settings(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    content_width: f64,
    content_height: f64,
    settings: &ScalesLinesSettings,
    theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    chart_settings_state: &ChartSettingsState,
    result: &mut ChartSettingsModalResult,
    _current_time_ms: u64,
) {
    let row_height = 32.0;
    let checkbox_size = 16.0;
    let label_x = x + checkbox_size + 12.0;

    let text_color = &theme.item_text;
    let muted_color = &theme.item_text_muted;

    let scrollbar_width = 8.0;
    let viewport_height = content_height;
    let viewport_y = y;

    let section_count = 8.0;
    let item_count = 22.0;
    let gap_count = 6.0;
    let total_content_height = (section_count * row_height) + (item_count * row_height) + (gap_count * 12.0);

    result.total_content_height = total_content_height;
    result.viewport_height = viewport_height;

    let viewport_rect = WidgetRect::new(x, viewport_y, content_width, viewport_height);
    let scroll_config = ScrollableConfig {
        scrollbar_width,
        scrollbar_padding: 4.0,
        always_show_scrollbar: false,
    };
    let scrollable = ScrollableContainer::new(viewport_rect, &chart_settings_state.scroll, Some(scroll_config));
    scrollable.begin(ctx);

    let scroll_offset = scrollable.scroll_offset();

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    // Section: GRID
    let mut row_y = viewport_y - scroll_offset;
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionGrid), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    // show_grid
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.show_grid, theme);
    result.content_items.push(("scales:show_grid".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::ShowGrid), label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // vert_lines
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.vert_lines, theme);
    result.content_items.push(("scales:vert_lines".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::VerticalLines), label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // horz_lines
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.horz_lines, theme);
    result.content_items.push(("scales:horz_lines".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::HorizontalLines), label_x, row_y + row_height / 2.0);
    row_y += row_height + 12.0;

    // Section: PRICE SCALE
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionPriceScale), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    // price_scale_right
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.price_scale_right, theme);
    result.content_items.push(("scales:price_scale_right".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::ShowPriceScaleRight), label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // auto_scale
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.auto_scale, theme);
    result.content_items.push(("scales:auto_scale".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::AutoScale), label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // Section: TIME SCALE
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionTimeScale), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    // time_scale_bottom
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.time_scale_bottom, theme);
    result.content_items.push(("scales:time_scale_bottom".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::ShowTimeScaleBottom), label_x, row_y + row_height / 2.0);
    row_y += row_height + 12.0;

    // Section: PRICE LINES
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionPriceLines), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    // show_prev_close
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.show_prev_close, theme);
    result.content_items.push(("scales:show_prev_close".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::PrevDayClosePrice), label_x, row_y + row_height / 2.0);
    row_y += row_height + 12.0;

    // Section: CROSSHAIR
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionCrosshair), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");

    // Crosshair mode dropdown
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::CrosshairMode), x, row_y + row_height / 2.0);

    let dropdown_x = x + 100.0;
    let dropdown_width = 120.0;
    let dropdown_height = 28.0;
    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;

    let mode_label = match settings.crosshair_mode.as_str() {
        "Normal" => t_settings(SettingsKey::CrosshairNormal),
        "Magnet" => t_settings(SettingsKey::CrosshairMagnetStrong),
        "MagnetOHLC" => t_settings(SettingsKey::CrosshairMagnetLight),
        "Hidden" => t_settings(SettingsKey::CrosshairHidden),
        _ => &settings.crosshair_mode,
    };
    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, mode_label, row_y + row_height / 2.0, theme);
    let cw = 20.0;
    let tw = dropdown_width - cw;
    result.content_items.push(("dropdown_cycle:crosshair_mode".to_string(), WidgetRect::new(dropdown_x, dropdown_y, tw, dropdown_height)));
    result.content_items.push(("dropdown_menu:crosshair_mode".to_string(), WidgetRect::new(dropdown_x + tw, dropdown_y, cw, dropdown_height)));
    row_y += row_height;

    // Crosshair line style dropdown
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::CrosshairLineStyle), x, row_y + row_height / 2.0);

    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    let style_label = match settings.crosshair_line_style.as_str() {
        "Solid" => t_settings(SettingsKey::LineStyleSolid),
        "Dashed" => t_settings(SettingsKey::LineStyleDashed),
        "Dotted" => t_settings(SettingsKey::LineStyleDotted),
        "LargeDashed" => t_settings(SettingsKey::LineStyleLargeDashed),
        "SparseDotted" => t_settings(SettingsKey::LineStyleSparseDotted),
        _ => &settings.crosshair_line_style,
    };
    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, style_label, row_y + row_height / 2.0, theme);
    result.content_items.push(("dropdown_cycle:crosshair_line_style".to_string(), WidgetRect::new(dropdown_x, dropdown_y, tw, dropdown_height)));
    result.content_items.push(("dropdown_menu:crosshair_line_style".to_string(), WidgetRect::new(dropdown_x + tw, dropdown_y, cw, dropdown_height)));
    row_y += row_height;

    // Crosshair width slider
    let slider_config = SliderConfig::new(1.0, 4.0).with_step(0.1);
    let slider_width = 200.0;
    let slider_rect = WidgetRect::new(x, row_y, slider_width, row_height);
    let widget_theme = toolbar_to_widget_theme(theme, frame_theme);

    let _is_editing_value = chart_settings_state.text_input.is_editing("scales:crosshair_line_width_value");
    let hovered = chart_settings_state.slider_drag.as_ref()
        .map(|d| d.field_id == "scales:crosshair_line_width")
        .unwrap_or_else(|| chart_settings_state.hovered_item_id.as_deref() == Some("scales:crosshair_line_width"));

    let crosshair_width_display = chart_settings_state.slider_drag.as_ref()
        .filter(|d| d.field_id == "scales:crosshair_line_width")
        .and_then(|d| d.floating_value)
        .unwrap_or(settings.crosshair_line_width);

    let slider_result = render_single_slider(ctx, &slider_config, crosshair_width_display, slider_rect, t_settings(SettingsKey::CrosshairLineWidth), &widget_theme, hovered, None);
    result.content_items.push(("scales:crosshair_line_width".to_string(), slider_result.full_rect));
    if let Some(input_rect) = slider_result.input_rect {
        result.content_items.push(("scales:crosshair_line_width_value".to_string(), input_rect));
    }
    if let Some(wi) = slider_result.track_info {
        result.slider_tracks.push(SliderTrackInfo {
            field_id: "scales:crosshair_line_width".to_string(),
            track_x: wi.track_x,
            track_width: wi.track_width,
            min_val: wi.min_val,
            max_val: wi.max_val,
        });
    }
    row_y += row_height;

    // Crosshair line color swatch
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::CrosshairLineColor), x, row_y + row_height / 2.0);

    let swatch_size = 24.0;
    let swatch_x = dropdown_x;
    let swatch_y = row_y + (row_height - swatch_size) / 2.0;

    ctx.set_fill_color(&settings.crosshair_line_color);
    ctx.fill_rounded_rect(swatch_x, swatch_y, swatch_size, swatch_size, 4.0);
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(swatch_x, swatch_y, swatch_size, swatch_size, 4.0);
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(&settings.crosshair_line_color, swatch_x + swatch_size + 8.0, row_y + row_height / 2.0);

    let color_rect = WidgetRect::new(swatch_x, swatch_y, swatch_size, swatch_size);
    result.content_items.push(("scales:crosshair_line_color".to_string(), color_rect));
    row_y += row_height + 12.0;

    // Section: SCALE POSITION
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionScalePosition), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::PriceScalePosition), x, row_y + row_height / 2.0);

    let dropdown_x2 = x + 120.0;
    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    let price_pos_label = match settings.price_scale_position.as_str() {
        "left" => t_settings(SettingsKey::ScalePosLeft),
        "right" => t_settings(SettingsKey::ScalePosRight),
        "hidden" => t_settings(SettingsKey::ScalePosHidden),
        _ => &settings.price_scale_position,
    };
    draw_split_dropdown(ctx, dropdown_x2, dropdown_y, dropdown_width, dropdown_height, price_pos_label, row_y + row_height / 2.0, theme);
    let tw2 = dropdown_width - cw;
    result.content_items.push(("dropdown_cycle:price_position".to_string(), WidgetRect::new(dropdown_x2, dropdown_y, tw2, dropdown_height)));
    result.content_items.push(("dropdown_menu:price_position".to_string(), WidgetRect::new(dropdown_x2 + tw2, dropdown_y, cw, dropdown_height)));
    row_y += row_height;

    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::TimeScalePosition), x, row_y + row_height / 2.0);

    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    let time_pos_label = match settings.time_scale_position.as_str() {
        "top" => t_settings(SettingsKey::ScalePosTop),
        "bottom" => t_settings(SettingsKey::ScalePosBottom),
        "hidden" => t_settings(SettingsKey::ScalePosHidden),
        _ => &settings.time_scale_position,
    };
    draw_split_dropdown(ctx, dropdown_x2, dropdown_y, dropdown_width, dropdown_height, time_pos_label, row_y + row_height / 2.0, theme);
    result.content_items.push(("dropdown_cycle:time_position".to_string(), WidgetRect::new(dropdown_x2, dropdown_y, tw2, dropdown_height)));
    result.content_items.push(("dropdown_menu:time_position".to_string(), WidgetRect::new(dropdown_x2 + tw2, dropdown_y, cw, dropdown_height)));
    row_y += row_height;

    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(t_settings(SettingsKey::CornerButtons), x, row_y + row_height / 2.0);

    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    let corner_vis_label = match settings.corner_visibility.as_str() {
        "always" => t_settings(SettingsKey::CornerAlways),
        "on_hover" => t_settings(SettingsKey::CornerOnHover),
        "never" => t_settings(SettingsKey::CornerNever),
        _ => &settings.corner_visibility,
    };
    draw_split_dropdown(ctx, dropdown_x2, dropdown_y, dropdown_width, dropdown_height, corner_vis_label, row_y + row_height / 2.0, theme);
    result.content_items.push(("dropdown_cycle:corner_visibility".to_string(), WidgetRect::new(dropdown_x2, dropdown_y, tw2, dropdown_height)));
    result.content_items.push(("dropdown_menu:corner_visibility".to_string(), WidgetRect::new(dropdown_x2 + tw2, dropdown_y, cw, dropdown_height)));
    row_y += row_height + 12.0;

    // Section: SCALE SIZE
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionScaleSize), x, row_y + row_height / 2.0);
    row_y += row_height;

    let slider_config = SliderConfig::new(50.0, 150.0).with_step(1.0);
    let slider_width2 = 280.0;
    let slider_rect = WidgetRect::new(x, row_y, slider_width2, row_height);
    let widget_theme = toolbar_to_widget_theme(theme, frame_theme);

    let _is_editing_price = chart_settings_state.text_input.is_editing("scales:price_width_value");
    let hovered = chart_settings_state.slider_drag.as_ref()
        .map(|d| d.field_id == "scales:price_width_slider")
        .unwrap_or_else(|| chart_settings_state.hovered_item_id.as_deref() == Some("scales:price_width_slider"));
    let price_width_display = chart_settings_state.slider_drag.as_ref()
        .filter(|d| d.field_id == "scales:price_width_slider")
        .and_then(|d| d.floating_value)
        .unwrap_or(settings.price_scale_width);

    let slider_result = render_single_slider(ctx, &slider_config, price_width_display, slider_rect, t_settings(SettingsKey::PriceScaleWidth), &widget_theme, hovered, None);
    result.content_items.push(("scales:price_width_slider".to_string(), slider_result.full_rect));
    if let Some(input_rect) = slider_result.input_rect {
        result.content_items.push(("scales:price_width_value".to_string(), input_rect));
    }
    if let Some(wi) = slider_result.track_info {
        result.slider_tracks.push(SliderTrackInfo {
            field_id: "scales:price_width_slider".to_string(),
            track_x: wi.track_x,
            track_width: wi.track_width,
            min_val: wi.min_val,
            max_val: wi.max_val,
        });
    }
    row_y += row_height;

    let time_slider_config = SliderConfig::new(20.0, 60.0).with_step(1.0);
    let slider_rect = WidgetRect::new(x, row_y, slider_width2, row_height);

    let _is_editing_time = chart_settings_state.text_input.is_editing("scales:time_height_value");
    let hovered = chart_settings_state.slider_drag.as_ref()
        .map(|d| d.field_id == "scales:time_height_slider")
        .unwrap_or_else(|| chart_settings_state.hovered_item_id.as_deref() == Some("scales:time_height_slider"));
    let time_height_display = chart_settings_state.slider_drag.as_ref()
        .filter(|d| d.field_id == "scales:time_height_slider")
        .and_then(|d| d.floating_value)
        .unwrap_or(settings.time_scale_height);

    let slider_result = render_single_slider(ctx, &time_slider_config, time_height_display, slider_rect, t_settings(SettingsKey::TimeScaleHeight), &widget_theme, hovered, None);
    result.content_items.push(("scales:time_height_slider".to_string(), slider_result.full_rect));
    if let Some(input_rect) = slider_result.input_rect {
        result.content_items.push(("scales:time_height_value".to_string(), input_rect));
    }
    if let Some(wi) = slider_result.track_info {
        result.slider_tracks.push(SliderTrackInfo {
            field_id: "scales:time_height_slider".to_string(),
            track_x: wi.track_x,
            track_width: wi.track_width,
            min_val: wi.min_val,
            max_val: wi.max_val,
        });
    }
    row_y += row_height + 12.0;

    // Section: TIME FORMAT
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionTimeFormat), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(ClockKey::DateFormat.get(current_language()), x, row_y + row_height / 2.0);

    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    let format_label = match settings.date_format.as_str() {
        "day_month_year" => "21.01.2026",
        "month_day_year" => "01/21/2026",
        "year_month_day" => "2026-01-21",
        "day_month_short" => "21 Jan",
        _ => &settings.date_format,
    };
    draw_split_dropdown(ctx, dropdown_x2, dropdown_y, dropdown_width, dropdown_height, format_label, row_y + row_height / 2.0, theme);
    result.content_items.push(("dropdown_cycle:date_format".to_string(), WidgetRect::new(dropdown_x2, dropdown_y, tw2, dropdown_height)));
    result.content_items.push(("dropdown_menu:date_format".to_string(), WidgetRect::new(dropdown_x2 + tw2, dropdown_y, cw, dropdown_height)));
    row_y += row_height;

    // 24h checkbox
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.use_24h, theme);
    result.content_items.push(("scales:use_24h".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ClockKey::Use24h.get(current_language()), x + checkbox_size + 12.0, row_y + row_height / 2.0);
    row_y += row_height;

    // show_day_of_week
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.show_day_of_week, theme);
    result.content_items.push(("scales:show_day_of_week".to_string(), check_rect));
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(ClockKey::DayOfWeek.get(current_language()), x + checkbox_size + 12.0, row_y + row_height / 2.0);

    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
}

// =============================================================================
// Status Line tab
// =============================================================================

fn render_status_line_settings(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    width: f64,
    content_height: f64,
    settings: &StatusLineSettings,
    theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    chart_settings_state: &ChartSettingsState,
    result: &mut ChartSettingsModalResult,
    _current_time_ms: u64,
) {
    let row_height = 32.0;
    let checkbox_size = 16.0;
    let label_x_offset = checkbox_size + 12.0;

    let text_color = &theme.item_text;
    let muted_color = &theme.item_text_muted;

    let scrollbar_width = 8.0;
    let viewport_height = content_height;
    let viewport_y = y;

    let section_count = 4.0;
    let item_count = 9.0;
    let gap_count = 3.0;
    let total_content_height = (section_count * row_height) + (item_count * row_height) + (gap_count * 12.0);

    result.total_content_height = total_content_height;
    result.viewport_height = viewport_height;

    let viewport_rect = WidgetRect::new(x, viewport_y, width, viewport_height);
    let scroll_config = ScrollableConfig {
        scrollbar_width,
        scrollbar_padding: 4.0,
        always_show_scrollbar: false,
    };
    let scrollable = ScrollableContainer::new(viewport_rect, &chart_settings_state.scroll, Some(scroll_config));
    scrollable.begin(ctx);

    let content_start_y = scrollable.content_y();
    let label_x = x + label_x_offset;

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let mut row_y = content_start_y;

    // Section: LEGEND
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionLegend), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Position), x, row_y + row_height / 2.0);

    let dropdown_x = x + 100.0;
    let dropdown_width = 150.0;
    let dropdown_height = 28.0;
    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;

    let legend_pos_label = match settings.legend_position.as_str() {
        "top_left" => t_settings(SettingsKey::LegendTopLeft),
        "top_right" => t_settings(SettingsKey::LegendTopRight),
        "bottom_left" => t_settings(SettingsKey::LegendBottomLeft),
        "bottom_right" => t_settings(SettingsKey::LegendBottomRight),
        _ => t_settings(SettingsKey::LegendTopLeft),
    };
    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, legend_pos_label, dropdown_y + dropdown_height / 2.0, theme);
    let cw = 20.0;
    let tw = dropdown_width - cw;
    result.content_items.push(("status:dropdown_cycle:legend_position".to_string(), WidgetRect::new(dropdown_x, dropdown_y, tw, dropdown_height)));
    result.content_items.push(("status:dropdown_menu:legend_position".to_string(), WidgetRect::new(dropdown_x + tw, dropdown_y, cw, dropdown_height)));
    row_y += row_height;

    // legend_show_ohlc
    ctx.set_font("12px sans-serif");
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.legend_show_ohlc, theme);
    result.content_items.push(("status:legend_show_ohlc".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::ShowOhlc), label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // legend_show_change
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.legend_show_change, theme);
    result.content_items.push(("status:legend_show_change".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::ShowChange), label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // legend_show_percent
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.legend_show_percent, theme);
    result.content_items.push(("status:legend_show_percent".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::ShowPercent), label_x, row_y + row_height / 2.0);
    row_y += row_height + 12.0;

    // Section: TOOLTIP
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionTooltip), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    // tooltip_visible
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.tooltip_visible, theme);
    result.content_items.push(("status:tooltip_visible".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Show), label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // tooltip_follow_cursor
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.tooltip_follow_cursor, theme);
    result.content_items.push(("status:tooltip_follow".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::FollowCursor), label_x, row_y + row_height / 2.0);
    row_y += row_height + 12.0;

    // Section: WATERMARK
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionWatermark), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    // watermark_visible
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.watermark_visible, theme);
    result.content_items.push(("status:watermark_visible".to_string(), check_rect));
    let wm_label_x = check_rect.right() + 8.0;
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Show), wm_label_x, row_y + row_height / 2.0);
    row_y += row_height;

    // watermark_position dropdown
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Position), x, row_y + row_height / 2.0);

    let dropdown_y = row_y + (row_height - dropdown_height) / 2.0;
    let watermark_pos_label = match settings.watermark_position.as_str() {
        "top_left" => t_settings(SettingsKey::LegendTopLeft),
        "top_right" => t_settings(SettingsKey::LegendTopRight),
        "bottom_left" => t_settings(SettingsKey::LegendBottomLeft),
        "bottom_right" => t_settings(SettingsKey::LegendBottomRight),
        "center" => t_settings(SettingsKey::LegendCenter),
        _ => t_settings(SettingsKey::LegendBottomLeft),
    };
    draw_split_dropdown(ctx, dropdown_x, dropdown_y, dropdown_width, dropdown_height, watermark_pos_label, dropdown_y + dropdown_height / 2.0, theme);
    result.content_items.push(("status:dropdown_cycle:watermark_position".to_string(), WidgetRect::new(dropdown_x, dropdown_y, tw, dropdown_height)));
    result.content_items.push(("status:dropdown_menu:watermark_position".to_string(), WidgetRect::new(dropdown_x + tw, dropdown_y, cw, dropdown_height)));
    row_y += row_height;

    // watermark_color swatch
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::Color), x, row_y + row_height / 2.0);

    let swatch_size = 24.0;
    let swatch_x = dropdown_x;
    let swatch_y = row_y + (row_height - swatch_size) / 2.0;

    ctx.set_fill_color(&settings.watermark_color);
    ctx.fill_rounded_rect(swatch_x, swatch_y, swatch_size, swatch_size, 3.0);
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(swatch_x, swatch_y, swatch_size, swatch_size, 3.0);

    let swatch_rect = WidgetRect::new(swatch_x, swatch_y, swatch_size, swatch_size);
    result.content_items.push(("status:watermark_color".to_string(), swatch_rect));
    row_y += row_height;

    // watermark_text input
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::Text), x, row_y + row_height / 2.0);

    let input_rect = WidgetRect::new(dropdown_x, row_y + 2.0, 200.0, row_height - 4.0);
    let is_editing = chart_settings_state.editing_text.as_ref()
        .map(|e| e.field_id == "status:watermark_text")
        .unwrap_or(false);

    let (display_text, cursor_pos, selection_start, selection_end) = if let Some(ref edit) = chart_settings_state.editing_text {
        if edit.field_id == "status:watermark_text" {
            let text: &str = if edit.text.is_empty() { "" } else { &edit.text };
            (text, edit.cursor, edit.selection_start, Some(edit.cursor))
        } else {
            (settings.watermark_text.as_str(), settings.watermark_text.len(), None, None)
        }
    } else {
        (settings.watermark_text.as_str(), settings.watermark_text.len(), None, None)
    };

    let input_config = InputConfig::new(display_text)
        .with_focused(is_editing)
        .with_type(InputType::Text)
        .with_font_size(12.0)
        .with_padding(8.0)
        .with_radius(4.0)
        .with_cursor(cursor_pos)
        .with_selection(selection_start, selection_end);

    let widget_theme = toolbar_to_widget_theme(theme, frame_theme);
    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);

    if is_editing {
        draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &theme.item_text);
        // Expose char positions for drag-to-select
        result.active_input_char_positions = input_result.char_x_positions;
        result.active_input_rect = Some(input_rect);
    }

    result.content_items.push(("status:watermark_text".to_string(), input_rect));
    row_y += row_height + 12.0;

    // Section: INDICATORS
    ctx.set_fill_color(muted_color);
    ctx.set_font("11px sans-serif");
    ctx.fill_text(t_settings(SettingsKey::SectionIndicators), x, row_y + row_height / 2.0);
    row_y += row_height;

    ctx.set_font("12px sans-serif");
    let check_y = row_y + (row_height - checkbox_size) / 2.0;
    let check_rect = WidgetRect::new(x, check_y, checkbox_size, checkbox_size);
    draw_checkbox(ctx, check_rect, settings.show_indicator_overlay, theme);
    result.content_items.push(("status:show_indicator_overlay".to_string(), check_rect));
    ctx.set_fill_color(text_color);
    ctx.fill_text(t_settings(SettingsKey::ShowIndicatorPanel), label_x, row_y + row_height / 2.0);

    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, total_content_height, &widget_theme);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
}

// =============================================================================
// Helper widgets
// =============================================================================

/// Draw a standard checkbox at the given rect with the given checked state.
fn draw_checkbox(ctx: &mut dyn RenderContext, rect: WidgetRect, checked: bool, theme: &ToolbarTheme) {
    if checked {
        ctx.set_fill_color(&theme.item_bg_active);
    } else {
        ctx.set_fill_color(&theme.background);
    }
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    if checked {
        ctx.set_stroke_color("#ffffff");
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(rect.x + 3.0, rect.center_y());
        ctx.line_to(rect.x + 6.0, rect.bottom() - 4.0);
        ctx.line_to(rect.right() - 3.0, rect.y + 4.0);
        ctx.stroke();
        ctx.set_stroke_width(1.0);
    }
}

/// Draw a split-chevron dropdown (text area + chevron area separated by a vertical line).
///
/// Does NOT register hit rects — callers must push those to `result.content_items` themselves.
fn draw_split_dropdown(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    label: &str,
    text_center_y: f64,
    theme: &ToolbarTheme,
) {
    ctx.set_fill_color(&theme.background);
    ctx.fill_rounded_rect(x, y, width, height, 4.0);
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, y, width, height, 4.0);
    ctx.set_fill_color(&theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(label, x + 8.0, text_center_y);

    let chevron_width = 20.0;
    let text_width = width - chevron_width;

    // Vertical separator
    ctx.set_stroke_color(&theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x + text_width, y);
    ctx.line_to(x + text_width, y + height);
    ctx.stroke();

    // Chevron triangle
    let arrow_x = x + text_width + chevron_width / 2.0 - 3.0;
    let arrow_y = y + height / 2.0;
    ctx.set_fill_color(&theme.item_text);
    ctx.begin_path();
    ctx.move_to(arrow_x, arrow_y - 3.0);
    ctx.line_to(arrow_x + 6.0, arrow_y - 3.0);
    ctx.line_to(arrow_x + 3.0, arrow_y + 3.0);
    ctx.close_path();
    ctx.fill();
}
