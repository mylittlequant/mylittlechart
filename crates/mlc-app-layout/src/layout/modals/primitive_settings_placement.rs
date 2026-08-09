//! Primitive settings modal — "Размещение" tab: points and timeframe
//! visibility (the old Coordinates + Visibility tabs merged). Part of the
//! approved 3-tab redesign ported from the `mlc-ui-lab` prototype (owner
//! direction, 2026-08-07..08 — `crates/mlc-app-shell-web/pkg/fib_modal.html`'s
//! "РАЗМЕЩЕНИЕ" column).
//!
//! Точки reuses the pre-port point-editing ids unchanged (`coord_{i}_price`
//! / `coord_{i}_bar` — `crate::layout::modals::primitive_settings_intent`
//! does not touch them, `mlc-app/src/input/settings.rs` keeps its existing
//! handling) and adds a real, additional read: a `DD.MM HH:MM` formatting
//! of the same millisecond value the price/bar fields already edit as raw
//! milliseconds — via `mlc_core::chart::types::format_time_full`, the same
//! cross-platform (no wasm-only `Date`) formatter the crosshair label
//! already uses, not a parallel one. The время/индекс бара TOGGLE is real
//! either way (both pills click, both flip
//! `PrimitiveSettingsState::time_display_mode`); what is inert is the
//! бара-индекс CONVERSION — turning a millisecond timestamp into a bar
//! index needs the chart's own bar array and bar spacing, which this
//! render call has no access to (chrome, not chart geometry) — so that
//! mode's field honestly reads "н/д" rather than faking a number. Visible
//! on timeframes is real: `PrimitiveData::timeframe_visibility` already
//! exists and is read/written directly — but the six range sliders in the
//! mock are drawn as a live, correct, NON-draggable readout plus two real
//! number fields (min/max) instead of a working drag, same simplification
//! the pre-port Visibility tab's own dual-slider drag never had wired for
//! this modal. Both simplifications are called out again in the port
//! report.
//!
//! Both groups' own tables/chips stay bespoke under the 2026-08-08
//! declared-properties pass (`primitive_settings_vid`'s own doc has the
//! full report) — the vocabulary's own doc
//! (`mlc_core::drawing::primitives_v2::config::PropertyCategory`) names
//! "the points table" and "the timeframe chips" as its own bespoke
//! examples, and this module never cherry-picked a property by id either
//! (its `coord_{i}_*`/`tf_{class}_*` ids key a COLLECTION item — a point
//! index, a timeframe class — not a declared property). What DOES change:
//! each group now appends any properties a primitive declares into
//! `PropertyCategory::PlacementPoints`/`PlacementTimeframes` — rendered
//! generically after the bespoke table/chips, sorted by their own `order` —
//! so a type that wants a placement-adjacent setting (e.g. a per-primitive
//! "lock points" toggle) has somewhere to declare it without this module
//! changing. No primitive declares either today, so this renders nothing
//! for every existing type — the mechanism is proven wired the same way
//! `primitive_settings_vid`'s LINE/FILL groups are, not by inspection.

use crate::layout::render_frame::PrimitiveSettingsResult;
use crate::layout::modals::primitive_settings_property::{declared_properties, draw_property_row, properties_in};
use crate::ui::modal_settings::{PrimitiveSettingsState, TimeDisplayMode};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType, WidgetState, WidgetTheme, SETTINGS_BUTTON_RADIUS};
use mlc_core::chart::types::format_time_full;
use mlc_core::drawing::primitives_v2::config::PropertyCategory;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::TimeframeVisibilityConfig;
use mlc_core::i18n::{current_language, ConfigKey, Language, SettingsKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

const ROW_H: f64 = 26.0;
const GROUP_HEADER_H: f64 = 22.0;
const GROUP_PAD_BOTTOM: f64 = 8.0;
const GROUP_GAP: f64 = 9.0;
const PILL_ROW_H: f64 = 24.0;
const RANGE_ROW_H: f64 = 24.0;

/// The six numeric timeframe classes, in the mock's own order. `ranges` is
/// boolean-only, listed separately below.
const NUMERIC_CLASSES: [(&str, u32, u32); 6] = [
    ("seconds", 1, 59),
    ("minutes", 1, 59),
    ("hours", 1, 24),
    ("days", 1, 366),
    ("weeks", 1, 52),
    ("months", 1, 12),
];

fn class_label(id: &str, lang: Language) -> &'static str {
    match id {
        "seconds" => SettingsKey::TfSeconds.get(lang),
        "minutes" => SettingsKey::TfMinutes.get(lang),
        "hours" => SettingsKey::TfHours.get(lang),
        "days" => SettingsKey::TfDays.get(lang),
        "weeks" => SettingsKey::TfWeeks.get(lang),
        "months" => SettingsKey::TfMonths.get(lang),
        _ => "",
    }
}

/// The chip row (six numeric classes + "Диапазоны", every label
/// language-dependent) practically never fits one line at typical modal
/// widths — `draw_timeframes_group`'s own wrap check would need the real
/// modal width to answer exactly, and that width is not settled until
/// AFTER this content height feeds the shell that computes it. Assuming
/// two lines is the safe direction: it costs a little blank space on a
/// wide window rather than clipping the last range row on a narrow one.
const CHIP_ROW_LINES_ESTIMATE: f64 = 2.0;

/// Any properties the primitive declares into `PropertyCategory::PlacementPoints`
/// — appended after the bespoke points table, sorted by their own `order`.
/// No primitive declares any today; see this module's own doc.
fn declared_points_rows(prim: &dyn Primitive) -> Vec<mlc_core::drawing::primitives_v2::config::ConfigProperty> {
    properties_in(&declared_properties(prim), PropertyCategory::PlacementPoints).into_iter().map(|(p, _)| p.clone()).collect()
}

/// Same as [`declared_points_rows`] for `PropertyCategory::PlacementTimeframes`.
fn declared_timeframes_rows(prim: &dyn Primitive) -> Vec<mlc_core::drawing::primitives_v2::config::ConfigProperty> {
    properties_in(&declared_properties(prim), PropertyCategory::PlacementTimeframes).into_iter().map(|(p, _)| p.clone()).collect()
}

/// Height of the "Точки" group alone — shared by `content_height` (always
/// counts it) and `content_height_for_query` (counts it only when
/// `group_points_matches` says the live query keeps it in the result).
fn points_group_height(prim: &dyn Primitive) -> f64 {
    let n_points = prim.points().len().max(1);
    let declared_h = declared_points_rows(prim).len() as f64 * ROW_H;
    GROUP_HEADER_H + n_points as f64 * ROW_H + PILL_ROW_H + declared_h + GROUP_PAD_BOTTOM
}

/// Height of the "Виден на таймфреймах" group alone — same sharing as
/// [`points_group_height`], gated by `group_timeframes_matches`.
fn timeframes_group_height(prim: &dyn Primitive) -> f64 {
    let tf = prim.data().timeframe_visibility.clone().unwrap_or_else(TimeframeVisibilityConfig::all);
    let enabled_count = NUMERIC_CLASSES.iter().filter(|(id, ..)| class_enabled(&tf, id)).count();
    let chip_rows_h = CHIP_ROW_LINES_ESTIMATE * (PILL_ROW_H + 4.0);
    let declared_h = declared_timeframes_rows(prim).len() as f64 * ROW_H;
    GROUP_HEADER_H + chip_rows_h + enabled_count as f64 * RANGE_ROW_H + declared_h + GROUP_PAD_BOTTOM
}

pub fn content_height(prim: &dyn Primitive) -> f64 {
    points_group_height(prim) + GROUP_GAP + timeframes_group_height(prim)
}

/// Query-aware body height for the cross-tab settings-search result view
/// (item 8, 2026-08-08 follow-up) — unlike `content_height` (static, always
/// reserves both groups), this counts only the group(s) `draw` will
/// actually paint for the live query, using the SAME
/// `group_timeframes_matches`/`group_points_matches` predicates `draw`
/// gates on, so the two can never drift apart.
pub fn content_height_for_query(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    let query = super::primitive_settings::search_query(state);
    let show_tf = group_timeframes_matches(&query);
    let show_points = group_points_matches(prim, &query);
    let mut h = 0.0;
    if show_tf {
        h += timeframes_group_height(prim);
    }
    if show_points {
        if show_tf {
            h += GROUP_GAP;
        }
        h += points_group_height(prim);
    }
    h
}

/// Draws the Размещение tab's own rows, group-gated by a live
/// settings-search query exactly as before the cross-tab search follow-up
/// (an empty query keeps both groups, matching `content_height` above).
/// Returns `true` if at least one group was drawn — the caller (the
/// cross-tab search combiner in `primitive_settings.rs`) uses this to
/// decide whether to show this tab's own header.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    row_y: &mut f64,
    prim: &dyn Primitive,
) -> bool {
    // Delta 1 (2026-08-08 port): timeframes group ABOVE the points group —
    // the owner has asked twice for this order; the archive/prototype drew
    // points first.
    //
    // Cross-tab settings search (item 8, 2026-08-08 review): filtering is
    // GROUP-level here, not per-row — "Точки" is a coordinate list, hiding
    // one point but not another would read as broken, not filtered.
    let query = super::primitive_settings::search_query(state);
    let show_tf = group_timeframes_matches(&query);
    let show_points = group_points_matches(prim, &query);

    if show_tf {
        draw_timeframes_group(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, row_y, prim);
        if show_points {
            *row_y += GROUP_GAP;
        }
    }
    if show_points {
        draw_points_group(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, row_y, prim);
    }
    show_tf || show_points
}

/// Whether the "Точки" group has anything a live settings-search query
/// matches — its own header title or any point's own label.
fn group_points_matches(prim: &dyn Primitive, query: &str) -> bool {
    let lang = current_language();
    if super::primitive_settings::search_matches(query, &[ConfigKey::GroupPoints.get(lang)]) {
        return true;
    }
    let points = prim.points();
    let labels = mlc_core::drawing::get_point_labels(prim.type_id(), points.len());
    labels.iter().any(|l| super::primitive_settings::search_matches(query, &[l.as_str()]))
}

/// Whether the "Виден на таймфреймах" group has anything a live
/// settings-search query matches — its own header title or any of the
/// numeric class labels / "Диапазоны".
fn group_timeframes_matches(query: &str) -> bool {
    let lang = current_language();
    if super::primitive_settings::search_matches(query, &[ConfigKey::GroupVisibleOnTimeframes.get(lang)]) {
        return true;
    }
    if NUMERIC_CLASSES.iter().any(|(id, ..)| super::primitive_settings::search_matches(query, &[class_label(id, lang)])) {
        return true;
    }
    super::primitive_settings::search_matches(query, &[SettingsKey::TfRanges.get(lang)])
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
    label_col: f64,
    row_y: &mut f64,
    prim: &dyn Primitive,
) {
    let lang = current_language();
    let points = prim.points();
    draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, ConfigKey::GroupPoints.get(lang), Some(&points.len().to_string()));
    *row_y += GROUP_HEADER_H;

    let point_labels = mlc_core::drawing::get_point_labels(prim.type_id(), points.len());
    let point_prefix = ConfigKey::PointLabel.get(lang);
    let name_w = 60.0;
    let field_w = ((x1 - x0 - name_w - 16.0) / 2.0).max(70.0);
    let price_x = x0 + name_w;
    let time_x = price_x + field_w + 8.0;

    for (i, (ms, price)) in points.iter().enumerate() {
        let y = *row_y;
        let label = point_labels.get(i).cloned().unwrap_or_else(|| format!("{point_prefix} {}", i + 1));
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, x0, y + ROW_H / 2.0);

        let price_field = format!("coord_{i}_price");
        draw_number_field(ctx, toolbar_theme, widget_theme, state, result, WidgetRect::new(price_x, y + 2.0, field_w, ROW_H - 4.0), &price_field, &format!("{price:.4}"));

        match state.time_display_mode {
            TimeDisplayMode::Time => {
                let time_field = format!("coord_{i}_bar");
                let is_editing = state.active_field_id.as_deref() == Some(time_field.as_str());
                let display = if is_editing { None } else { Some(format_time_full(ms.div_euclid(1000))) };
                draw_time_field(ctx, toolbar_theme, widget_theme, state, result, WidgetRect::new(time_x, y + 2.0, field_w, ROW_H - 4.0), &time_field, display.as_deref(), *ms);
            }
            TimeDisplayMode::BarIndex => {
                // The toggle itself is real (see the two pills below); what
                // is inert is the CONVERSION — this render call has no bar
                // array, no bar spacing, nothing to convert a millisecond
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
                ctx.fill_text(ConfigKey::NotAvailable.get(lang), rect.center_x(), rect.center_y());
            }
        }
        *row_y += ROW_H;
    }

    let pill_y = *row_y + 2.0;
    let time_on = state.time_display_mode == TimeDisplayMode::Time;
    let time_pill = draw_pill(ctx, toolbar_theme, x0, pill_y, ConfigKey::TimeModeTime.get(lang), time_on);
    let bar_pill = draw_pill(ctx, toolbar_theme, time_pill.right() + 6.0, pill_y, ConfigKey::TimeModeBarIndex.get(lang), !time_on);
    result.content_items.push(("placement_time_mode:time".to_string(), time_pill));
    result.content_items.push(("placement_time_mode:bar_index".to_string(), bar_pill));
    *row_y += PILL_ROW_H;

    // Any declared `PropertyCategory::PlacementPoints` properties — see
    // this module's own doc. No primitive declares any today.
    for prop in &declared_points_rows(prim) {
        draw_property_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, mlc_core::drawing::primitives_v2::PropertyKind::Style, prop);
        *row_y += ROW_H;
    }

    *row_y += GROUP_PAD_BOTTOM;
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
    // One button shape everywhere in this modal: a rectangle at
    // `SETTINGS_BUTTON_RADIUS`, not a fully-rounded pill (item 1,
    // 2026-08-08 review) — matches the generic hover highlight's own
    // radius so the two never mismatch.
    ctx.set_fill_color(if on { &toolbar_theme.accent } else { &toolbar_theme.background });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_fill_color(if on { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.clip();
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    ctx.restore();
    rect
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
    label_col: f64,
    row_y: &mut f64,
    prim: &dyn Primitive,
) {
    let lang = current_language();
    let tf = prim.data().timeframe_visibility.clone().unwrap_or_else(TimeframeVisibilityConfig::all);

    draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, ConfigKey::GroupVisibleOnTimeframes.get(lang), None);
    *row_y += GROUP_HEADER_H;

    let mut chip_x = x0;
    let mut chip_y = *row_y;
    let mut chip_labels: Vec<(String, &str, bool)> =
        NUMERIC_CLASSES.iter().map(|(id, ..)| (format!("tf_{id}_toggle"), class_label(id, lang), class_enabled(&tf, id))).collect();
    chip_labels.push(("tf_ranges_toggle".to_string(), SettingsKey::TfRanges.get(lang), tf.ranges));
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

    for (id, lo, hi) in NUMERIC_CLASSES {
        let Some((cmin, cmax)) = class_range(&tf, id) else { continue };
        let y = *row_y;
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(class_label(id, lang), x0, y + RANGE_ROW_H / 2.0);

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

    // Any declared `PropertyCategory::PlacementTimeframes` properties — see
    // this module's own doc. No primitive declares any today.
    for prop in &declared_timeframes_rows(prim) {
        draw_property_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, mlc_core::drawing::primitives_v2::PropertyKind::Style, prop);
        *row_y += ROW_H;
    }

    *row_y += GROUP_PAD_BOTTOM;
}

fn draw_group_header(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x0: f64, x1: f64, y: f64, title: &str, sub: Option<&str>) {
    // No contrasting bar (item 7, 2026-08-08 review): the header background
    // is the modal's own background colour, matching the Вид tab's own
    // group headers.
    let rect = WidgetRect::new(x0, y, (x1 - x0).max(0.0), GROUP_HEADER_H);
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);
    ctx.set_font("10.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&title.to_uppercase(), rect.center_x(), rect.center_y());
    if let Some(s) = sub {
        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text_muted, 0.7));
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(s, rect.right() - 6.0, rect.center_y());
    }
}
