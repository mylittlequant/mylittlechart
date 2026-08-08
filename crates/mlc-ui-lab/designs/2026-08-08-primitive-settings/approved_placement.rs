//! The approved design's "Размещение" tab: the old Coordinates +
//! Visibility merged into two groups — Точки (a point/price/time table
//! plus a время/индекс бара toggle) and Виден на таймфреймах (chips per
//! class, a range row only for the enabled ones). Source of truth:
//! `crates/mlc-app-shell-web/pkg/fib_modal.html`'s "РАЗМЕЩЕНИЕ" column.
//!
//! Точки reuses baseline's own point-editing ids unchanged
//! (`coord_{i}_price` / `coord_{i}_bar` — `crate::dispatch::EditableField`,
//! not touched by this pass) and adds a real, additional read: a
//! `DD.MM HH:MM` formatting of the same millisecond value baseline already
//! calls "Bar" while editing raw milliseconds. The время/индекс бара TOGGLE
//! is real either way (both pills click, both flip `DesignUiState::
//! time_display_mode`); what is inert is the бара-индекс CONVERSION —
//! turning a millisecond timestamp into a bar index needs the chart's own
//! bar array and bar spacing, which this lab has none of
//! (`crate::render_context`'s own doc comment: no chart geometry,
//! `bar_to_x`/`price_to_y` are identity placeholders) — so that mode's
//! field honestly reads "н/д" rather than faking a number. Visible on
//! timeframes is real: `PrimitiveData::
//! timeframe_visibility` already exists and is read/written directly, no
//! external dependency — but the six range sliders in the mock are drawn
//! as a live, correct, NON-draggable readout plus two real number fields
//! (min/max) instead of a working drag, mirroring the honesty of the
//! dual-handle sliders elsewhere in this crate
//! (`crate::dispatch::on_drag_start`'s own `tf_` note: that drag was never
//! wired here either). Both simplifications are called out again in the
//! implementation report.

use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType, WidgetState, WidgetTheme};
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::TimeframeVisibilityConfig;
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use crate::design_state::{DesignUiState, TimeDisplayMode};

const ROW_H: f64 = 26.0;
const GROUP_HEADER_H: f64 = 22.0;
const GROUP_PAD_BOTTOM: f64 = 8.0;
const GROUP_GAP: f64 = 9.0;
const PILL_ROW_H: f64 = 24.0;
const RANGE_ROW_H: f64 = 24.0;

/// The six numeric timeframe classes, in the mock's own order. `ranges` is
/// boolean-only, listed separately below.
const NUMERIC_CLASSES: [(&str, &str, u32, u32); 6] = [
    ("seconds", "Секунды", 1, 59),
    ("minutes", "Минуты", 1, 59),
    ("hours", "Часы", 1, 24),
    ("days", "Дни", 1, 366),
    ("weeks", "Недели", 1, 52),
    ("months", "Месяцы", 1, 12),
];

/// The chip row (six numeric classes + "Диапазоны", every label in
/// Cyrillic) practically never fits one line at the sector widths this lab
/// draws at — `draw_timeframes_group`'s own wrap check would need the real
/// modal width to answer exactly, and that width is not settled until
/// AFTER this content height feeds the shell that computes it. Assuming
/// two lines is the safe direction: it costs a little blank space on a
/// wide window rather than clipping the last range row on a narrow one
/// (the class of bug the overflow fixes elsewhere in this crate exist to
/// close).
const CHIP_ROW_LINES_ESTIMATE: f64 = 2.0;

pub fn content_height(prim: &dyn Primitive) -> f64 {
    let n_points = prim.points().len().max(1);
    let points_h = GROUP_HEADER_H + n_points as f64 * ROW_H + PILL_ROW_H + GROUP_PAD_BOTTOM;

    let tf = prim.data().timeframe_visibility.clone().unwrap_or_else(TimeframeVisibilityConfig::all);
    let enabled_count = NUMERIC_CLASSES.iter().filter(|(id, ..)| class_enabled(&tf, id)).count();
    let chip_rows_h = CHIP_ROW_LINES_ESTIMATE * (PILL_ROW_H + 4.0);
    let tf_h = GROUP_HEADER_H + chip_rows_h + enabled_count as f64 * RANGE_ROW_H + GROUP_PAD_BOTTOM;

    points_h + GROUP_GAP + tf_h
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
    design_ui: &DesignUiState,
) {
    draw_points_group(ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, prim, design_ui);
    *row_y += GROUP_GAP;
    draw_timeframes_group(ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, prim);
}

// =============================================================================
// Точки
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_points_group(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    row_y: &mut f64,
    prim: &dyn Primitive,
    design_ui: &DesignUiState,
) {
    let points = prim.points();
    draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, "Точки", Some(&points.len().to_string()));
    *row_y += GROUP_HEADER_H;

    let point_labels = mlc_core::drawing::get_point_labels(prim.type_id(), points.len());
    let name_w = 60.0;
    let field_w = ((x1 - x0 - name_w - 16.0) / 2.0).max(70.0);
    let price_x = x0 + name_w;
    let time_x = price_x + field_w + 8.0;

    for (i, (ms, price)) in points.iter().enumerate() {
        let y = *row_y;
        let label = point_labels.get(i).cloned().unwrap_or_else(|| format!("Точка {}", i + 1));
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, x0, y + ROW_H / 2.0);

        let price_field = format!("coord_{i}_price");
        draw_number_field(ctx, toolbar_theme, widget_theme, state, result, WidgetRect::new(price_x, y + 2.0, field_w, ROW_H - 4.0), &price_field, &format!("{price:.4}"));

        match design_ui.time_display_mode {
            TimeDisplayMode::Time => {
                let time_field = format!("coord_{i}_bar");
                let is_editing = state.active_field_id.as_deref() == Some(time_field.as_str());
                let display = if is_editing { None } else { Some(format_ms(*ms)) };
                draw_time_field(ctx, toolbar_theme, widget_theme, state, result, WidgetRect::new(time_x, y + 2.0, field_w, ROW_H - 4.0), &time_field, display.as_deref(), *ms);
            }
            TimeDisplayMode::BarIndex => {
                // The toggle itself is real (see the two pills below); what
                // is inert is the CONVERSION — this lab has no bar array,
                // no bar spacing, nothing to convert a millisecond
                // timestamp against (see the module doc). Reporting "н/д"
                // honestly is the point, not a placeholder for a future
                // fake number.
                let rect = WidgetRect::new(time_x, y + 2.0, field_w, ROW_H - 4.0);
                ctx.set_fill_color(&toolbar_theme.background);
                ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("н/д", rect.center_x(), rect.center_y());
            }
        }
        *row_y += ROW_H;
    }

    let pill_y = *row_y + 2.0;
    let time_on = design_ui.time_display_mode == TimeDisplayMode::Time;
    let time_pill = draw_pill(ctx, toolbar_theme, x0, pill_y, "время", time_on);
    let bar_pill = draw_pill(ctx, toolbar_theme, time_pill.right() + 6.0, pill_y, "индекс бара", !time_on);
    result.content_items.push(("placement_time_mode:time".to_string(), time_pill));
    result.content_items.push(("placement_time_mode:bar_index".to_string(), bar_pill));
    *row_y += PILL_ROW_H + GROUP_PAD_BOTTOM;
}

fn draw_number_field(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    field_id: &str,
    committed: &str,
) {
    let is_editing = state.active_field_id.as_deref() == Some(field_id);
    let ar = &state.active_render;
    let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
        (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
    } else {
        (committed, committed.len(), None, None)
    };
    let cfg = InputConfig::new(display)
        .with_focused(is_editing)
        .with_type(InputType::Number)
        .with_font_size(11.0)
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
}

/// Same input widget as [`draw_number_field`], but shown as a formatted
/// date/time string while closed and the raw millisecond value while
/// being typed into — real editing, friendly display.
fn draw_time_field(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    field_id: &str,
    formatted: Option<&str>,
    raw_ms: i64,
) {
    let raw_text = raw_ms.to_string();
    let committed = formatted.unwrap_or(&raw_text);
    draw_number_field(ctx, toolbar_theme, widget_theme, state, result, rect, field_id, committed);
}

fn draw_pill(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x: f64, y: f64, label: &str, on: bool) -> WidgetRect {
    ctx.set_font("11.5px sans-serif");
    let w = ctx.measure_text(label) + 20.0;
    let rect = WidgetRect::new(x, y, w, PILL_ROW_H);
    ctx.set_fill_color(if on { &toolbar_theme.accent } else { &toolbar_theme.background });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, PILL_ROW_H / 2.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, PILL_ROW_H / 2.0);
    ctx.set_fill_color(if on { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    rect
}


/// `DD.MM HH:MM`, local time, from a millisecond epoch value — pure
/// arithmetic + `js_sys::Date` (this crate is wasm32-only, see
/// `crate::lib`), no chart/bar dependency.
fn format_ms(ms: i64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    format!("{:02}.{:02} {:02}:{:02}", d.get_date(), d.get_month() + 1, d.get_hours(), d.get_minutes())
}

// =============================================================================
// Виден на таймфреймах
// =============================================================================

fn class_enabled(tf: &TimeframeVisibilityConfig, id: &str) -> bool {
    match id {
        "seconds" => tf.seconds.is_some(),
        "minutes" => tf.minutes.is_some(),
        "hours" => tf.hours.is_some(),
        "days" => tf.days.is_some(),
        "weeks" => tf.weeks.is_some(),
        "months" => tf.months.is_some(),
        _ => false,
    }
}

fn class_range(tf: &TimeframeVisibilityConfig, id: &str) -> Option<(u32, u32)> {
    match id {
        "seconds" => tf.seconds,
        "minutes" => tf.minutes,
        "hours" => tf.hours,
        "days" => tf.days,
        "weeks" => tf.weeks,
        "months" => tf.months,
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_timeframes_group(
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
    let tf = prim.data().timeframe_visibility.clone().unwrap_or_else(TimeframeVisibilityConfig::all);

    draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, "Виден на таймфреймах", None);
    *row_y += GROUP_HEADER_H;

    let mut chip_x = x0;
    let mut chip_y = *row_y;
    let mut chip_labels: Vec<(String, &str, bool)> =
        NUMERIC_CLASSES.iter().map(|(id, label, ..)| (format!("tf_{id}_toggle"), *label, class_enabled(&tf, id))).collect();
    chip_labels.push(("tf_ranges_toggle".to_string(), "Диапазоны", tf.ranges));
    for (field_id, label, on) in chip_labels {
        ctx.set_font("11.5px sans-serif");
        let w = ctx.measure_text(label) + 20.0;
        if chip_x + w > x1 && chip_x > x0 {
            chip_x = x0;
            chip_y += PILL_ROW_H + 4.0;
        }
        let rect = draw_pill(ctx, toolbar_theme, chip_x, chip_y, label, on);
        result.content_items.push((field_id, rect));
        chip_x = rect.right() + 6.0;
    }
    *row_y = chip_y + PILL_ROW_H + 6.0;

    for (id, label, lo, hi) in NUMERIC_CLASSES {
        let Some((cmin, cmax)) = class_range(&tf, id) else { continue };
        let y = *row_y;
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, x0, y + RANGE_ROW_H / 2.0);

        let min_field = format!("tf_{id}_min");
        let max_field = format!("tf_{id}_max");
        let field_w = 40.0;
        let min_rect = WidgetRect::new(x0 + 62.0, y + 2.0, field_w, RANGE_ROW_H - 4.0);
        let max_rect = WidgetRect::new(x1 - field_w, y + 2.0, field_w, RANGE_ROW_H - 4.0);

        draw_number_field(ctx, toolbar_theme, widget_theme, state, result, min_rect, &min_field, &cmin.to_string());
        draw_number_field(ctx, toolbar_theme, widget_theme, state, result, max_rect, &max_field, &cmax.to_string());

        // Live, correct, non-draggable readout between the two fields —
        // see the module doc for why dragging is not wired.
        let track_x0 = min_rect.right() + 6.0;
        let track_x1 = max_rect.x - 6.0;
        let track_w = (track_x1 - track_x0).max(0.0);
        let track_y = y + RANGE_ROW_H / 2.0 - 1.0;
        ctx.set_fill_color(&toolbar_theme.separator);
        ctx.fill_rounded_rect(track_x0, track_y, track_w, 2.0, 1.0);
        let span = (hi - lo).max(1) as f64;
        let fx0 = track_x0 + track_w * ((cmin.saturating_sub(lo)) as f64 / span);
        let fx1 = track_x0 + track_w * ((cmax.saturating_sub(lo)) as f64 / span);
        ctx.set_fill_color(&toolbar_theme.accent);
        ctx.fill_rounded_rect(fx0, track_y, (fx1 - fx0).max(2.0), 2.0, 1.0);

        *row_y += RANGE_ROW_H;
    }
    *row_y += GROUP_PAD_BOTTOM;
}

fn draw_group_header(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x0: f64, x1: f64, y: f64, title: &str, sub: Option<&str>) {
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
