//! The approved design's "Вид" tab: the old Style + Text merged into three
//! groups — Линия (colour/width/style/extend/trend line), Заливка (fill
//! toggle/opacity), Подписи (which level labels to show, position, size,
//! custom text, bold/italic, the 3×3 alignment grid) — with a search field
//! above them. Source of truth: `crates/mlc-app-shell-web/pkg/fib_modal.html`'s
//! "ВИД" column.
//!
//! Every control here is either a base `PrimitiveData` field
//! (`stroke_color`/`stroke_width`/`line_style`, the same ids baseline's own
//! Style tab uses) or one of `Primitive::style_properties()`/
//! `data().text`'s fields — all of it already generically wired in
//! `crate::dispatch` (`style_prop:*`, `text_content`, `text_bold`,
//! `text_italic`, `text_pos_*`), unchanged. This module only lays the same
//! real controls out in the approved grouping; it adds no new dispatch.
//!
//! Two simplifications from baseline's own widgets, both real, neither
//! faked: "Стиль" and "Положение" are single cycle-on-click buttons
//! (baseline draws a second chevron zone that opens a dropdown menu;
//! every option is still reachable here, just by cycling rather than
//! picking directly) — reported in the implementation notes. The search
//! field filters in real time but is NOT persisted once you click away
//! (also noted) — it drives visibility straight from the live text buffer
//! the same way an in-progress edit already does, with no second copy of
//! the string kept anywhere.

use mlc_app_layout::layout::render_frame::{PrimitiveSettingsResult, SliderTrackInfo};
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{
    draw_input, draw_input_cursor, render_single_slider, InputConfig, InputType, SliderConfig, WidgetState,
    WidgetTheme,
};
use mlc_core::drawing::primitives_v2::config::{ConfigProperty, PropertyType};
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::{LineStyle, TextAlign as PrimTextAlign};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

const ROW_H: f64 = 26.0;
const GROUP_HEADER_H: f64 = 22.0;
const GROUP_PAD_BOTTOM: f64 = 8.0;
const GROUP_GAP: f64 = 9.0;
const SEARCH_H: f64 = 26.0;
const ALIGN_H: f64 = 72.0;
const PILL_H: f64 = 22.0;

/// Static — does not shrink when the search field filters rows out (a
/// filtered row just stops advancing `row_y` during the draw pass instead;
/// the scroll range may read slightly larger than what's visible while a
/// query is active). Noted in the implementation report.
pub fn content_height() -> f64 {
    let line_h = GROUP_HEADER_H + 5.0 * ROW_H + GROUP_PAD_BOTTOM;
    let fill_h = GROUP_HEADER_H + 2.0 * ROW_H + GROUP_PAD_BOTTOM;
    let label_h = GROUP_HEADER_H + PILL_H + 4.0 * ROW_H + ALIGN_H + GROUP_PAD_BOTTOM;
    SEARCH_H + GROUP_GAP + line_h + GROUP_GAP + fill_h + GROUP_GAP + label_h
}

/// Live search query — the field's own live buffer while it is focused,
/// nothing otherwise. See the module doc for why this is not persisted.
fn search_query(state: &PrimitiveSettingsState) -> String {
    if state.active_field_id.as_deref() == Some("vid_search") {
        state.active_render.text.to_lowercase()
    } else {
        String::new()
    }
}

fn matches(query: &str, texts: &[&str]) -> bool {
    query.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(query))
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    row_y: &mut f64,
    prim: &dyn Primitive,
) {
    let query = search_query(state);
    draw_search(ctx, toolbar_theme, state, result, x0, x1, *row_y);
    *row_y += SEARCH_H + GROUP_GAP;

    let props = prim.style_properties();
    let style = prim.data().style;
    let stroke = prim.data().color.stroke.clone();
    let width = prim.data().width;
    let text_block = prim.data().text.clone();

    // ---- Линия ----
    let line_rows: [&str; 5] = ["Цвет", "Толщина", "Стиль", "Продлить", "Линия тренда"];
    if matches(&query, &["Линия"]) || line_rows.iter().any(|r| matches(&query, &[r])) {
        let group_top = *row_y;
        draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, "Линия", None);
        *row_y += GROUP_HEADER_H;

        if matches(&query, &["Линия", "Цвет"]) {
            draw_label(ctx, toolbar_theme, x0, *row_y, "Цвет");
            let rect = WidgetRect::new(x0 + 90.0, *row_y + 3.0, 60.0, ROW_H - 6.0);
            ctx.set_fill_color(&stroke);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
            result.content_items.push(("stroke_color".to_string(), rect));
            *row_y += ROW_H;
        }

        if matches(&query, &["Линия", "Толщина"]) {
            draw_slider_row(
                ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y, "stroke_width", "Толщина",
                width, 1.0, 10.0, 0.1, true,
            );
            *row_y += ROW_H;
        }

        if matches(&query, &["Линия", "Стиль"]) {
            draw_label(ctx, toolbar_theme, x0, *row_y, "Стиль");
            let rect = WidgetRect::new(x0 + 90.0, *row_y + 2.0, 150.0, ROW_H - 4.0);
            draw_cycle_button(ctx, toolbar_theme, rect, line_style_label(style));
            result.content_items.push(("line_style".to_string(), rect));
            *row_y += ROW_H;
        }

        if matches(&query, &["Линия", "Продлить"]) {
            draw_label(ctx, toolbar_theme, x0, *row_y, "Продлить");
            let left_on = find_bool(&props, "extend_left");
            let right_on = find_bool(&props, "extend_right");
            let seg_w = 54.0;
            let left_rect = WidgetRect::new(x0 + 90.0, *row_y + 2.0, seg_w, ROW_H - 4.0);
            let right_rect = WidgetRect::new(left_rect.right() + 4.0, *row_y + 2.0, seg_w, ROW_H - 4.0);
            draw_toggle_segment(ctx, toolbar_theme, left_rect, "влево", left_on);
            draw_toggle_segment(ctx, toolbar_theme, right_rect, "вправо", right_on);
            result.content_items.push(("style_prop:extend_left".to_string(), left_rect));
            result.content_items.push(("style_prop:extend_right".to_string(), right_rect));
            *row_y += ROW_H;
        }

        if matches(&query, &["Линия", "Линия тренда"]) {
            draw_bool_row(ctx, toolbar_theme, result, x0, *row_y, "Линия тренда", find_bool(&props, "show_trend_line"), "style_prop:show_trend_line");
            *row_y += ROW_H;
        }

        if *row_y == group_top + GROUP_HEADER_H {
            // Every row in this group was filtered out — collapse the
            // header back out too, rather than leaving a floating label.
            *row_y = group_top;
        } else {
            *row_y += GROUP_PAD_BOTTOM;
        }
    }
    *row_y += GROUP_GAP;

    // ---- Заливка ----
    let fill_rows: [&str; 2] = ["Заливка", "Прозрачность"];
    if matches(&query, &["Заливка"]) || fill_rows.iter().any(|r| matches(&query, &[r])) {
        let group_top = *row_y;
        draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, "Заливка", None);
        *row_y += GROUP_HEADER_H;

        if matches(&query, &["Заливка"]) {
            draw_bool_row(ctx, toolbar_theme, result, x0, *row_y, "Заливка", find_bool(&props, "fill"), "style_prop:fill");
            *row_y += ROW_H;
        }
        if matches(&query, &["Прозрачность"]) {
            if let Some(prop) = find_prop(&props, "fill_opacity") {
                let (min, max) = number_range(prop);
                draw_slider_row(
                    ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y, "style_prop:fill_opacity",
                    "Прозрачность", prop.value.as_number().unwrap_or(0.0), min, max, 1.0, false,
                );
            }
            *row_y += ROW_H;
        }

        if *row_y == group_top + GROUP_HEADER_H {
            *row_y = group_top;
        } else {
            *row_y += GROUP_PAD_BOTTOM;
        }
    }
    *row_y += GROUP_GAP;

    // ---- Подписи ----
    {
        let group_top = *row_y;
        draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, "Подписи", Some("что показывать и чем"));
        *row_y += GROUP_HEADER_H;

        if matches(&query, &["Подписи", "цена", "уровень", "процент"]) {
            draw_pills_row(ctx, toolbar_theme, result, x0, *row_y, &[
                ("цена", find_bool(&props, "show_prices"), "style_prop:show_prices"),
                ("уровень", find_bool(&props, "show_percentages"), "style_prop:show_percentages"),
                ("процент", find_bool(&props, "show_as_percent"), "style_prop:show_as_percent"),
            ]);
            *row_y += PILL_H + 4.0;
        }

        if matches(&query, &["Подписи", "Положение"]) {
            draw_label(ctx, toolbar_theme, x0, *row_y, "Положение");
            let rect = WidgetRect::new(x0 + 90.0, *row_y + 2.0, 132.0, ROW_H - 4.0);
            let current = find_prop(&props, "label_position").and_then(|p| p.value.as_string()).unwrap_or("left");
            draw_cycle_button(ctx, toolbar_theme, rect, label_position_label(current));
            result.content_items.push(("style_prop:label_position".to_string(), rect));
            *row_y += ROW_H;
        }

        if matches(&query, &["Подписи", "Кегль"]) {
            if let Some(prop) = find_prop(&props, "label_font_size") {
                let (min, max) = number_range(prop);
                draw_slider_row(
                    ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y, "style_prop:label_font_size",
                    "Кегль", prop.value.as_number().unwrap_or(12.0), min, max, 1.0, false,
                );
            }
            *row_y += ROW_H;
        }

        if matches(&query, &["Подписи", "Свой текст"]) {
            draw_label(ctx, toolbar_theme, x0, *row_y, "Свой текст");
            let field_id = "text_content";
            let is_editing = state.active_field_id.as_deref() == Some(field_id);
            let current = text_block.as_ref().map(|t| t.content.clone()).unwrap_or_default();
            let ar = &state.active_render;
            let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
                (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
            } else {
                (current.as_str(), current.len(), None, None)
            };
            let rect = WidgetRect::new(x0 + 90.0, *row_y + 2.0, 132.0, ROW_H - 4.0);
            let cfg = InputConfig::new(display)
                .with_focused(is_editing)
                .with_type(InputType::Text)
                .with_font_size(11.5)
                .with_padding(4.0)
                .with_radius(3.0)
                .with_cursor(cursor)
                .with_selection(sel_start, sel_end);
            let res = draw_input(ctx, &cfg, WidgetState::Normal, rect, widget_theme);
            if is_editing {
                draw_input_cursor(ctx, res.cursor_x, res.cursor_y, res.cursor_height, &toolbar_theme.item_text);
                result.active_input_char_positions = res.char_x_positions;
                result.active_input_rect = Some(rect);
            }
            result.content_items.push((field_id.to_string(), rect));
            *row_y += ROW_H;
        }

        if matches(&query, &["Подписи", "Начертание"]) {
            draw_label(ctx, toolbar_theme, x0, *row_y, "Начертание");
            let bold = text_block.as_ref().map(|t| t.bold).unwrap_or(false);
            let italic = text_block.as_ref().map(|t| t.italic).unwrap_or(false);
            let seg_w = 30.0;
            let bold_rect = WidgetRect::new(x0 + 90.0, *row_y + 2.0, seg_w, ROW_H - 4.0);
            let italic_rect = WidgetRect::new(bold_rect.right() + 4.0, *row_y + 2.0, seg_w, ROW_H - 4.0);
            draw_toggle_segment(ctx, toolbar_theme, bold_rect, "Ж", bold);
            draw_toggle_segment(ctx, toolbar_theme, italic_rect, "К", italic);
            result.content_items.push(("text_bold".to_string(), bold_rect));
            result.content_items.push(("text_italic".to_string(), italic_rect));
            *row_y += ROW_H;
        }

        if matches(&query, &["Подписи", "Выравнивание"]) {
            draw_label(ctx, toolbar_theme, x0, *row_y, "Выравнивание");
            let v_align = text_block.as_ref().map(|t| t.v_align).unwrap_or(PrimTextAlign::Center);
            let h_align = text_block.as_ref().map(|t| t.h_align).unwrap_or(PrimTextAlign::Center);
            draw_align_grid(ctx, toolbar_theme, result, x0 + 90.0, *row_y, v_align, h_align);
            *row_y += ALIGN_H;
        }

        if *row_y == group_top + GROUP_HEADER_H {
            *row_y = group_top;
        } else {
            *row_y += GROUP_PAD_BOTTOM;
        }
    }
}

// =============================================================================
// Small building blocks
// =============================================================================

fn find_prop<'a>(props: &'a [ConfigProperty], id: &str) -> Option<&'a ConfigProperty> {
    props.iter().find(|p| p.id == id)
}

fn find_bool(props: &[ConfigProperty], id: &str) -> bool {
    find_prop(props, id).and_then(|p| p.value.as_bool()).unwrap_or(false)
}

fn number_range(prop: &ConfigProperty) -> (f64, f64) {
    match prop.prop_type {
        PropertyType::Number { min, max, .. } => (min.unwrap_or(0.0), max.unwrap_or(100.0)),
        _ => (0.0, 100.0),
    }
}

fn line_style_label(style: LineStyle) -> String {
    match style {
        LineStyle::Solid => "\u{2014}\u{2014} Сплошная",
        LineStyle::Dashed => "- - Пунктир",
        LineStyle::Dotted => "\u{b7}\u{b7} Точки",
        LineStyle::LargeDashed => "\u{2014} \u{2014} Штрих",
        LineStyle::SparseDotted => "\u{b7} \u{b7} Редкие точки",
    }
    .to_string()
}

fn label_position_label(id: &str) -> String {
    match id {
        "center" => "По центру",
        "right" => "Справа",
        _ => "Слева",
    }
    .to_string()
}

fn draw_group_header(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    x0: f64,
    x1: f64,
    y: f64,
    title: &str,
    sub: Option<&str>,
) {
    let rect = WidgetRect::new(x0, y, (x1 - x0).max(0.0), GROUP_HEADER_H);
    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.separator, 0.4));
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);
    ctx.set_font("10.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&title.to_uppercase(), rect.x + 6.0, rect.center_y());
    if let Some(s) = sub {
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text_muted, 0.7));
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(s, rect.right() - 6.0, rect.center_y());
    }
}

fn draw_label(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x: f64, y: f64, text: &str) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, x, y + ROW_H / 2.0);
}

fn draw_bool_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    y: f64,
    label: &str,
    checked: bool,
    field_id: &str,
) {
    draw_label(ctx, toolbar_theme, x0, y, label);
    let rect = WidgetRect::new(x0 + 90.0, y + (ROW_H - 16.0) / 2.0, 16.0, 16.0);
    ctx.set_fill_color(if checked { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    if checked {
        ctx.set_stroke_color(&toolbar_theme.item_text_active);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(rect.x + 3.0, rect.y + rect.height * 0.55);
        ctx.line_to(rect.x + rect.width * 0.42, rect.bottom() - 3.0);
        ctx.line_to(rect.right() - 3.0, rect.y + 3.0);
        ctx.stroke();
    }
    result.content_items.push((field_id.to_string(), rect));
}

fn draw_pills_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    y: f64,
    pills: &[(&str, bool, &str)],
) {
    let mut x = x0;
    for (label, on, field_id) in pills {
        ctx.set_font("11.5px sans-serif");
        let w = ctx.measure_text(label) + 20.0;
        let rect = WidgetRect::new(x, y, w, PILL_H);
        ctx.set_fill_color(if *on { &toolbar_theme.accent } else { &toolbar_theme.background });
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, PILL_H / 2.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, PILL_H / 2.0);
        ctx.set_fill_color(if *on { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, rect.center_x(), rect.center_y());
        result.content_items.push(((*field_id).to_string(), rect));
        x += w + 6.0;
    }
}

fn draw_toggle_segment(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, rect: WidgetRect, label: &str, on: bool) {
    ctx.set_fill_color(if on { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    ctx.set_fill_color(if on { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted });
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
}

fn draw_cycle_button(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, rect: WidgetRect, label: String) {
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width - 6.0, rect.height);
    ctx.clip();
    ctx.fill_text(&label, rect.x + 8.0, rect.center_y());
    ctx.restore();
}

#[allow(clippy::too_many_arguments)]
fn draw_slider_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    field_id: &str,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    with_input: bool,
) {
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let mut cfg = SliderConfig::new(min, max).with_step(step);
    if !with_input {
        cfg = cfg.without_input();
    }
    let hovered = state.slider_drag.as_ref().map(|d| d.field_id == field_id).unwrap_or(false);
    let display = state
        .slider_drag
        .as_ref()
        .filter(|d| d.field_id == field_id)
        .and_then(|d| d.floating_value)
        .unwrap_or(value);
    let value_field = format!("{field_id}_value");
    let ar = &state.active_render;
    let editing = (with_input && state.active_field_id.as_deref() == Some(value_field.as_str()))
        .then(|| mlc_app_layout::ui::widgets::SliderEditingInfo { text: ar.text.as_str(), cursor: ar.cursor, selection_start: ar.selection_start });
    let rect = WidgetRect::new(x0, y, (x1 - x0).max(80.0), ROW_H);
    let res = render_single_slider(ctx, &cfg, display, rect, &format!("{label}:"), widget_theme, hovered, editing);
    if !with_input {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&format!("{display:.0}"), res.track_rect.right() + 8.0, y + ROW_H / 2.0);
    }
    result.content_items.push((field_id.to_string(), res.full_rect));
    if with_input {
        if let Some(input_rect) = res.input_rect {
            result.content_items.push((value_field.clone(), input_rect));
            if state.active_field_id.as_deref() == Some(value_field.as_str()) {
                result.active_input_rect = Some(input_rect);
                if let Some(ref ir) = res.input_result {
                    result.active_input_char_positions = ir.char_x_positions.clone();
                }
            }
        }
    }
    if let Some(t) = res.track_info {
        result.slider_tracks.push(SliderTrackInfo { field_id: field_id.to_string(), track_x: t.track_x, track_width: t.track_width, min_val: min, max_val: max });
    }
}

fn draw_align_grid(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    y0: f64,
    v_align: PrimTextAlign,
    h_align: PrimTextAlign,
) {
    let cell = 19.0;
    let gap = 3.0;
    let vs = [PrimTextAlign::Start, PrimTextAlign::Center, PrimTextAlign::End];
    let hs = [PrimTextAlign::Start, PrimTextAlign::Center, PrimTextAlign::End];
    for (vi, v) in vs.iter().enumerate() {
        for (hi, h) in hs.iter().enumerate() {
            let rect = WidgetRect::new(x0 + hi as f64 * (cell + gap), y0 + vi as f64 * (cell + gap), cell, cell);
            let on = *v == v_align && *h == h_align;
            ctx.set_fill_color(if on { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
            let v_id = align_id(*v);
            let h_id = align_id(*h);
            result.content_items.push((format!("text_pos_{v_id}_{h_id}"), rect));
        }
    }
}

fn align_id(a: PrimTextAlign) -> &'static str {
    match a {
        PrimTextAlign::Start => "start",
        PrimTextAlign::Center => "center",
        PrimTextAlign::End => "end",
    }
}

fn draw_search(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
) {
    let field_id = "vid_search";
    let is_editing = state.active_field_id.as_deref() == Some(field_id);
    let rect = WidgetRect::new(x0, y, (x1 - x0).max(0.0), SEARCH_H);
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    ctx.set_stroke_color(if is_editing { &toolbar_theme.accent } else { &toolbar_theme.separator });
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);

    let ar = &state.active_render;
    let placeholder = "поиск по настройкам\u{2026}";
    ctx.set_font("11.5px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    if is_editing {
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.fill_text(&ar.text, rect.x + 9.0, rect.center_y());
        result.active_input_rect = Some(rect);
    } else {
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.fill_text(placeholder, rect.x + 9.0, rect.center_y());
    }
    result.content_items.push((field_id.to_string(), rect));
}
