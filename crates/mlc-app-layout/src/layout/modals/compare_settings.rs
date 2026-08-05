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

use mlc_core::engine::render::{draw_svg_icon, RenderContext};
use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{CompareSettingsState, CompareSettingsTab, DualSliderHandle};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    begin_modal_shell, end_modal_shell, ModalFooter, ModalPlacement, ModalShellSpec, ModalTab, ModalTabs,
    ModalTheme, TabFit, FOOTER_H, HEADER_H, TAB_BAR_H,
    render_dual_slider, SliderConfig, SliderEditingInfo,
};
use crate::ui::Icon;
use mlc_core::drawing::TimeframeVisibilityConfig;
use crate::layout::render_ui::toolbar_to_widget_theme;
use uzor::input::Sense;
use uzor::types::Rect as WidgetRect;
use uzor::render::{TextAlign, TextBaseline};
use mlc_core::i18n::{SettingsKey, TextKey, ToolbarTooltipKey, t_settings, t_text, t_toolbar};

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
const MODAL_H: f64 = 422.0;
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
    templates: &[mlc_core::templates::CompareTemplate],
) -> CompareSettingsResult {
    let mut result = CompareSettingsResult::default();

    // -------------------------------------------------------------------------
    // Chrome — the shell owns frame, header, ✕, tab bar, footer and the clamp
    // -------------------------------------------------------------------------
    let tab_items: Vec<ModalTab<'_>> = CompareSettingsTab::all()
        .iter()
        .map(|tab| ModalTab::new(tab.id(), compare_tab_label(*tab)))
        .collect();
    let active_tab = CompareSettingsTab::all()
        .iter()
        .position(|t| *t == state.active_tab)
        .unwrap_or(0);
    // The size stays fixed: the shell gets the body height that leaves the
    // frame exactly MODAL_H tall with the shared chrome heights.
    let content_h = MODAL_H - HEADER_H - TAB_BAR_H - PADDING * 2.0 - FOOTER_H;

    let modal_theme = ModalTheme::from_toolbar(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let spec = ModalShellSpec {
        padding: PADDING,
        radius: RADIUS,
        placement: ModalPlacement::At(Some(state.position.unwrap_or((
            (screen_w - MODAL_W) / 2.0,
            (screen_h - MODAL_H) / 2.0,
        )))),
        ..ModalShellSpec::new("cmp_settings", t_settings(SettingsKey::Title), MODAL_W, content_h)
            // The layer keeps its old name: it is the key
            // `close_topmost_known_modal_layer` matches on.
            .layer_named("compare_settings")
            .with_tabs(ModalTabs::Horizontal {
                items: &tab_items,
                active: active_tab,
                fit: TabFit::Equal,
            })
            .with_footer(ModalFooter::Editor {
                template: Some(t_settings(SettingsKey::ButtonTemplate)),
            })
    };
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();

    // -------------------------------------------------------------------------
    // Content area
    // -------------------------------------------------------------------------
    let content_x = shell.content.x;
    let content_y = shell.content.y;
    let content_w = shell.content.width;

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
    // Footer — the bar is the shell's; only its template menu is drawn here
    // -------------------------------------------------------------------------
    if let Some(template_btn) = shell.template_btn {
        draw_template_menu(ctx, template_btn, state, toolbar_theme, input_coordinator, &layer_id, templates);
    }

    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id);

    result
}

// =============================================================================
// Private sub-renderers
// =============================================================================


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
    ctx.fill_text(t_text(TextKey::Color), content_x, row_y + ROW_H / 2.0);

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
    ctx.fill_text(t_text(TextKey::Width), content_x, row_y + ROW_H / 2.0);

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
    ctx.fill_text(t_settings(SettingsKey::LineStyle), content_x, row_y + ROW_H / 2.0);

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

    let style_label = line_style_label(state.cached_line_style.as_str());

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
            ("solid",  line_style_label("solid")),
            ("dashed", line_style_label("dashed")),
            ("dotted", line_style_label("dotted")),
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

    row_y += ROW_H;

    // --- Row 4: Chart type dropdown ---
    draw_chart_type_row(ctx, content_x, content_w, row_y, state, toolbar_theme, input_coordinator, layer_id);
}

/// Kline-renderable chart-type family available for a compare overlay
/// series — derived types (renko/footprint/etc.) need their own data
/// slots and are out of scope (see `draw_compare_overlay` dispatch).
const COMPARE_CHART_TYPES: &[&str] = &[
    "line", "step_line", "area", "candles", "bars", "hollow_candles", "heikin_ashi",
];

/// Human-readable label for a compare chart-type id — mirrors the
/// `chart_type_selector` toolbar dropdown's own localization
/// (`mlc-core/src/toolbar.rs::chart_type_items`): every id in
/// `COMPARE_CHART_TYPES` is one of the 10 plain render styles that dropdown
/// already routes through `ToolbarMenuKey`, so this uses the SAME keys
/// (symbol-search-compare-inner arc, Wave 3 — owner spec 2026-07-16: "verify
/// the toolbar dropdown does the same and match it"). Falls back to the
/// chart-type registry's raw `label` for any id outside that localized set.
fn compare_chart_type_label(id: &str) -> &str {
    use mlc_core::i18n::{t_toolbar_menu, ToolbarMenuKey};
    match id {
        "candles" => t_toolbar_menu(ToolbarMenuKey::Candles),
        "bars" => t_toolbar_menu(ToolbarMenuKey::Bars),
        "hollow_candles" => t_toolbar_menu(ToolbarMenuKey::HollowCandles),
        "heikin_ashi" => t_toolbar_menu(ToolbarMenuKey::HeikinAshi),
        "line" => t_toolbar_menu(ToolbarMenuKey::Line),
        "step_line" => t_toolbar_menu(ToolbarMenuKey::StepLine),
        "area" => t_toolbar_menu(ToolbarMenuKey::Area),
        _ => mlc_core::chart_type::by_id(id).map(|def| def.label).unwrap_or(id),
    }
}

/// Localized label for a compare series' line-style id ("solid" / "dashed"
/// / "dotted") — reuses the SAME `SettingsKey::LineStyle*` keys the chart
/// settings modal's own line-style rows use (symbol-search-compare-inner
/// arc, Wave 3 — owner spec 2026-07-16). Unknown ids fall back to "solid".
fn line_style_label(style_id: &str) -> &'static str {
    match style_id {
        "dashed" => t_settings(SettingsKey::LineStyleDashed),
        "dotted" => t_settings(SettingsKey::LineStyleDotted),
        _ => t_settings(SettingsKey::LineStyleSolid),
    }
}

/// Localized label for a `CompareSettingsTab` (Style / Visibility / Info) —
/// symbol-search-compare-inner arc, Wave 3 (owner spec 2026-07-16): the
/// modal's tab bar previously used `CompareSettingsTab::label()`, which
/// returns a hardcoded Russian literal. Routed through `SettingsKey` (the
/// same table `compare_settings.rs`'s footer buttons already use).
fn compare_tab_label(tab: CompareSettingsTab) -> &'static str {
    match tab {
        CompareSettingsTab::Style => t_settings(SettingsKey::CompareTabStyle),
        CompareSettingsTab::Visibility => t_settings(SettingsKey::CompareTabVisibility),
        CompareSettingsTab::Info => t_settings(SettingsKey::CompareTabInfo),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_chart_type_row(
    ctx: &mut dyn RenderContext,
    content_x: f64,
    content_w: f64,
    row_y: f64,
    state: &CompareSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_toolbar(ToolbarTooltipKey::ChartType), content_x, row_y + ROW_H / 2.0);

    let dd_x = content_x + LABEL_W;
    let dd_w = content_w - LABEL_W;
    let dd_h = 22.0;
    let dd_y = row_y + (ROW_H - dd_h) / 2.0;

    let is_hovered = state.hovered_item_id.as_deref() == Some("chart_type_dd");
    ctx.set_fill_color(if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg });
    ctx.fill_rounded_rect(dd_x, dd_y, dd_w, dd_h, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(dd_x, dd_y, dd_w, dd_h, 3.0);

    let label = compare_chart_type_label(state.cached_chart_type.as_str());
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(label, dd_x + 8.0, dd_y + dd_h / 2.0);

    let arrow_size = 12.0;
    let arrow_x = dd_x + dd_w - arrow_size - 4.0;
    let arrow_y = dd_y + (dd_h - arrow_size) / 2.0;
    draw_svg_icon(ctx, Icon::ChevronDown.svg(), arrow_x, arrow_y, arrow_size, arrow_size, &toolbar_theme.item_text_muted);

    input_coordinator.register_on_layer(
        "cmp_settings:chart_type_dd",
        WidgetRect::new(dd_x, dd_y, dd_w, dd_h),
        Sense::click(),
        layer_id,
    );

    // Chart type dropdown options (shown below the dropdown if open)
    if state.chart_type_dropdown_open {
        let opt_h = 24.0;
        let menu_w = dd_w;
        let menu_x = dd_x;
        let menu_y = dd_y + dd_h;
        let menu_total_h = opt_h * COMPARE_CHART_TYPES.len() as f64;

        // Background
        ctx.set_fill_color(&toolbar_theme.dropdown_bg);
        ctx.fill_rounded_rect(menu_x, menu_y, menu_w, menu_total_h, 3.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(menu_x, menu_y, menu_w, menu_total_h, 3.0);

        for (i, ct_id) in COMPARE_CHART_TYPES.iter().enumerate() {
            let opt_y = menu_y + i as f64 * opt_h;
            let widget_id = format!("cmp_settings:chart_type_option:{}", ct_id);

            let is_opt_hovered = state.hovered_item_id.as_deref()
                == Some(widget_id.strip_prefix("cmp_settings:").unwrap_or(""));
            let is_selected = state.cached_chart_type.as_str() == *ct_id;

            if is_opt_hovered || is_selected {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rect(menu_x, opt_y, menu_w, opt_h);
            }

            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(compare_chart_type_label(ct_id), menu_x + 8.0, opt_y + opt_h / 2.0);

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
        (t_settings(SettingsKey::TfTicks),   true,  tf_config.ticks,             None,                  0, 0),
        (t_settings(SettingsKey::TfSeconds), false, tf_config.seconds.is_some(), tf_config.seconds,     1, 59),
        (t_settings(SettingsKey::TfMinutes), false, tf_config.minutes.is_some(), tf_config.minutes,     1, 59),
        (t_settings(SettingsKey::TfHours),   false, tf_config.hours.is_some(),   tf_config.hours,       1, 24),
        (t_settings(SettingsKey::TfDays),    false, tf_config.days.is_some(),    tf_config.days,        1, 366),
        (t_settings(SettingsKey::TfWeeks),   false, tf_config.weeks.is_some(),   tf_config.weeks,       1, 52),
        (t_settings(SettingsKey::TfMonths),  false, tf_config.months.is_some(),  tf_config.months,      1, 12),
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
            ctx.set_stroke_color(&toolbar_theme.item_text_active);
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

            let ar = &state.active_render;
            let editing_min_info = if state.active_field_id.as_deref() == Some(min_field_id.as_str()) {
                Some(SliderEditingInfo { text: ar.text.as_str(), cursor: ar.cursor, selection_start: ar.selection_start })
            } else {
                None
            };

            let editing_max_info = if state.active_field_id.as_deref() == Some(max_field_id.as_str()) {
                Some(SliderEditingInfo { text: ar.text.as_str(), cursor: ar.cursor, selection_start: ar.selection_start })
            } else {
                None
            };

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
                if state.active_field_id.as_deref() == Some(min_field_id.as_str()) {
                    result.tf_active_input_rect = Some(min_input_rect);
                    if let Some(ref ir) = slider_result.min_input_result {
                        result.tf_active_input_char_positions = ir.char_x_positions.clone();
                    }
                }
            }
            if let Some(max_input_rect) = slider_result.max_input_rect {
                result.tf_content_items.push((max_field_id.clone(), max_input_rect));
                if state.active_field_id.as_deref() == Some(max_field_id.as_str()) {
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
        (t_settings(SettingsKey::CompareInfoSymbol), state.cached_symbol.as_str()),
        (t_settings(SettingsKey::CompareInfoName),   state.cached_name.as_str()),
        (t_settings(SettingsKey::CompareInfoBars),   bars_str.as_str()),
        (t_settings(SettingsKey::CompareInfoBase),   base_str.as_str()),
    ];

    for (i, (label, value)) in rows.iter().enumerate() {
        let row_y = content_y + 8.0 + i as f64 * ROW_H;
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.fill_text(label, content_x, row_y + ROW_H / 2.0);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.fill_text(value, content_x + LABEL_W, row_y + ROW_H / 2.0);
    }
}

/// The menu that hangs off the footer's Template button. The bar itself — the
/// separator, that button and OK — belongs to the modal shell; this places the
/// menu from the rect the shell reported rather than re-deriving it.
#[allow(clippy::too_many_arguments)]
fn draw_template_menu(
    ctx: &mut dyn RenderContext,
    template_btn: WidgetRect,
    state: &CompareSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    templates: &[mlc_core::templates::CompareTemplate],
) {
    let button_height = template_btn.height;
    let button_y = template_btn.y;
    let button_padding = 12.0;
    let template_btn_width = template_btn.width;
    let template_btn_x = template_btn.x;

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
        ctx.fill_text(t_settings(SettingsKey::SaveAsTemplate), dd_x + 8.0, row_y + opt_h / 2.0);
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
        ctx.fill_text(t_settings(SettingsKey::ApplyDefault), dd_x + 8.0, row_y + opt_h / 2.0);
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
            ctx.fill_text(t_settings(SettingsKey::NoTemplates), dd_x + 8.0, row_y + opt_h / 2.0);
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
                let del_color = if is_del_hovered { &toolbar_theme.danger } else { &toolbar_theme.item_text_muted };
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

    let _ = button_padding;
}

