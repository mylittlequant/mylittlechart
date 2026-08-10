//! Concept-modal renderer — the eight `Row` variants that are collections or
//! composite controls rather than one plain labelled field: [`Row::Ladder`],
//! [`Row::Ramps`], [`Row::FillMaster`], [`Row::Table`], [`Row::Cells`],
//! [`Row::Icons`], [`Row::Stepper`], [`Row::Range`] (see
//! [`crate::concept::spec::Row`]). Split out of `render.rs` (which was
//! already at its own soft line-count ceiling) rather than grown in place.
//!
//! Same ground rules as `render.rs`: real app widgets where one applies and
//! is actually callable against concept data (`draw_checkbox`/
//! `draw_level_bar` from `primitive_settings_levels`, `palette_color_at`/
//! `PALETTES` from `primitive_settings_palettes`, `draw_number_field` from
//! `primitive_settings_placement`, `draw_toggle_segment`/`draw_label` from
//! `primitive_settings_vid`, `draw_input`/`render_single_slider` from
//! `ui::widgets`); a small local reimplementation where the real widget's
//! own signature needs something concept data does not have (see
//! [`draw_ladder_row`]'s own doc for the one case that applies here —
//! `primitive_settings_levels::draw_level_value_field` takes `&dyn
//! Primitive` to format its own label, and a concept archetype has no live
//! `Primitive`, only a [`crate::concept::spec::LadderUnit`]).
//!
//! Every `*_height` function here is the single source of truth `render.rs`'s
//! own `row_height` calls into — never duplicated inline in a `draw_*`
//! function, the same discipline `render.rs` itself already documents for
//! its own `content_height_for_tab`/`draw_tab_body` pairing.

use mlc_app_layout::layout::modals::primitive_settings_levels::{draw_checkbox, draw_level_bar};
use mlc_app_layout::layout::modals::primitive_settings_palettes::{palette_color_at, PALETTES};
use mlc_app_layout::layout::modals::primitive_settings_placement::{draw_number_field, RANGE_ROW_H};
use mlc_app_layout::layout::modals::primitive_settings_vid::{draw_label, draw_toggle_segment, ROW_H};
use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{
    draw_input, render_single_slider, InputConfig, InputType, SliderConfig, WidgetState, WidgetTheme,
};
use mlc_core::drawing::primitives_v2::gann::gann_fan::gann_angle_label;
use mlc_core::i18n::{current_language, ConfigKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use crate::concept::spec::{Cell, LadderUnit, Tone};
use crate::concept::state::ConceptModalState;

// =============================================================================
// Ladder — Row::Ladder
// =============================================================================

const LADDER_CHECK: f64 = 16.0;
const LADDER_COL_GAP: f64 = 7.0;
const LADDER_VALUE_W: f64 = 52.0;
const LADDER_BAR_H: f64 = 14.0;
const LADDER_DEL_W: f64 = 14.0;
const LADDER_ADD_ROW_H: f64 = 34.0;

pub(super) fn ladder_height(n_values: usize) -> f64 {
    n_values.max(1) as f64 * ROW_H + LADDER_ADD_ROW_H
}

/// [`super::spec::Row::Ladder`] — one row per level (checkbox, value field,
/// colour/gradient bar, delete affordance) plus the "add level" affordance
/// underneath, mirroring `primitive_settings_levels::draw_ladder`'s own
/// shape minus the parts that need data this archetype does not carry (the
/// colour swatch/fill chips and the row's own inspector both key off a live
/// `FibLevelConfig`; nothing in `spec.rs` supplies one — see this crate's
/// top-of-file doc, "known simplifications", in `render.rs`).
///
/// `draw_checkbox`/`draw_level_bar` are the real app widgets, called
/// directly. The value field is NOT drawn via the real
/// `draw_level_value_field` — that function's own label comes from
/// `Primitive::level_value_label(level)`, which needs a live `&dyn
/// Primitive` this crate never has (a concept archetype's ladder value
/// format is decided by [`LadderUnit`] instead, per the task brief: "do NOT
/// reformat it yourself"). [`ladder_value_label`] below reproduces the exact
/// same two formats `level_value_label`'s own real implementations use
/// (`Primitive`'s own default `"{level:.3}"` for [`LadderUnit::Ratio`],
/// `gann_fan::gann_angle_label` — already `pub` in `mlc-core` — for
/// [`LadderUnit::Gann`]), then this function paints it with the same
/// `draw_input` call `draw_level_value_field` itself makes, just without the
/// edit-cursor branch (inert here, same simplification `render.rs`'s own
/// `draw_num_row`/`draw_area_row` already make for every other field).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_ladder_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y0: f64,
    state: &ConceptModalState,
    key: &str,
    values: &[f64],
    unit: LadderUnit,
) {
    let value_x = x0 + LADDER_CHECK + LADDER_COL_GAP;
    let del_x = x1 - LADDER_DEL_W;
    let bar_x = value_x + LADDER_VALUE_W + LADDER_COL_GAP;
    let bar_w = (del_x - LADDER_COL_GAP - bar_x).max(0.0);
    let denom = values.len().saturating_sub(1).max(1) as f64;

    for (i, &value) in values.iter().enumerate() {
        let y = y0 + i as f64 * ROW_H;
        if i > 0 {
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(x0, y);
            ctx.line_to(x1, y);
            ctx.stroke();
        }

        // Interactive (task requirement): the checkbox reads/would-mutate
        // through the same `ConceptModalState` keyed map every other
        // interactive row already uses — sub-keyed by level INDEX (stable
        // for a fixed spec list, unlike the real app's own value-keyed
        // `vkey`, which exists there to survive a resort this concept
        // ladder never does).
        let level_key = format!("{key}::level:{i}");
        let checked = state.chk(&level_key, true);
        let check_rect = WidgetRect::new(x0, y + (ROW_H - LADDER_CHECK) / 2.0, LADDER_CHECK, LADDER_CHECK);
        draw_checkbox(ctx, check_rect, checked, toolbar_theme, &toolbar_theme.background.clone());
        result.content_items.push((level_key, check_rect));

        let label = ladder_value_label(value, unit);
        let value_rect = WidgetRect::new(value_x, y + 2.0, LADDER_VALUE_W, ROW_H - 4.0);
        let input_cfg = InputConfig::new(&label).with_type(InputType::Text).with_font_size(11.5).with_padding(4.0).with_radius(3.0);
        draw_input(ctx, &input_cfg, WidgetState::Normal, value_rect, widget_theme);

        // Colour/gradient bar: `spec.rs` carries no per-level colour (unlike
        // the real `FibLevelConfig::color`), so this reads the Classic
        // palette's own interpolation across the ladder's own row index —
        // a real gradient, just not one wired to the Ramps row's own
        // selection (two independent `Row` entries, no data channel
        // between them; named again in the task report).
        let this_color = palette_color_at(&PALETTES[0].colors, i as f64 / denom);
        let next_color = values.get(i + 1).map(|_| palette_color_at(&PALETTES[0].colors, (i + 1) as f64 / denom));
        let bar_rect = WidgetRect::new(bar_x, y + (ROW_H - LADDER_BAR_H) / 2.0, bar_w, LADDER_BAR_H);
        draw_level_bar(ctx, bar_rect, &this_color, next_color.as_deref(), checked);

        let del_rect = WidgetRect::new(del_x, y + (ROW_H - LADDER_BAR_H) / 2.0, LADDER_DEL_W, LADDER_BAR_H);
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("\u{d7}", del_rect.center_x(), del_rect.center_y());
    }

    let add_y = y0 + values.len() as f64 * ROW_H;
    let mid_y = add_y + LADDER_ADD_ROW_H / 2.0;
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.accent);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("+", x0, mid_y);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.fill_text(ConfigKey::AddLevel.get(current_language()), x0 + 14.0, mid_y);
}

/// Mirrors the two real `Primitive::level_value_label` implementations this
/// crate cannot call directly (see [`draw_ladder_row`]'s own doc): the
/// trait's own default (`"{level:.3}"`, used by every Ratio-unit type —
/// Fib Retracement and Pitchfork neither override it) for
/// [`LadderUnit::Ratio`], and `gann_fan::gann_angle_label` — already `pub`
/// in `mlc-core`, called directly rather than reimplemented — for
/// [`LadderUnit::Gann`].
fn ladder_value_label(value: f64, unit: LadderUnit) -> String {
    match unit {
        LadderUnit::Ratio => format!("{value:.3}"),
        LadderUnit::Gann => gann_angle_label(value),
    }
}

// =============================================================================
// Ramps — Row::Ramps
// =============================================================================

const RAMP_CARD_H: f64 = 46.0;

pub(super) fn ramps_height() -> f64 {
    RAMP_CARD_H
}

/// [`super::spec::Row::Ramps`] — the four palette cards, drawn the same way
/// `primitive_settings_levels::draw_ramps_row` draws them (that function
/// itself is private, so this is a from-scratch redraw against the same
/// `PALETTES`/[`palette_color_at`] data, not a copy of unreachable code).
/// Static (task brief: "Everything else may be static") — the first card
/// reads permanently selected; no `ConceptModalState` key, since nothing
/// downstream (the Ladder row's own gradient) reads a live selection either.
pub(super) fn draw_ramps_row(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x0: f64, x1: f64, y: f64) {
    let gap = 7.0;
    let card_w = (((x1 - x0) - gap * 3.0) / 4.0).max(0.0);
    for (i, palette) in PALETTES.iter().enumerate() {
        let rect = WidgetRect::new(x0 + i as f64 * (card_w + gap), y, card_w, RAMP_CARD_H);
        let selected = i == 0;
        ctx.set_stroke_color(if selected { &toolbar_theme.accent } else { &toolbar_theme.separator });
        ctx.set_stroke_width(if selected { 2.0 } else { 1.0 });
        ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);

        let strip_h = 22.0;
        let strip_y = rect.y + 5.0;
        let seg_w = (rect.width - 10.0).max(0.0) / palette.colors.len() as f64;
        for (j, c) in palette.colors.iter().enumerate() {
            ctx.set_fill_color(c);
            ctx.fill_rect(rect.x + 5.0 + seg_w * j as f64, strip_y, seg_w, strip_h);
        }

        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(palette.name_key.get(current_language()), rect.center_x(), strip_y + strip_h + 10.0);
    }
    ctx.set_text_align(TextAlign::Left);
}

// =============================================================================
// FillMaster — Row::FillMaster
// =============================================================================

const FILLMASTER_CHECK: f64 = 16.0;
const FILLMASTER_GAP: f64 = 8.0;
const FILLMASTER_PCT_W: f64 = 40.0;

pub(super) fn fillmaster_height() -> f64 {
    ROW_H
}

/// [`super::spec::Row::FillMaster`] — the ladder tab's own "fill to next
/// level" master row: checkbox + opacity slider + value, in the same
/// right-anchored shape `primitive_settings_levels::draw_global_opacity_row`
/// draws (toggle chip flush to `x1`, percent readout just left of it,
/// slider taking the rest). Static (task brief: "Everything else may be
/// static") — `on`/`pct`/`value` all read straight off the spec row, no
/// `ConceptModalState` key.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_fillmaster_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    x0: f64,
    x1: f64,
    y: f64,
    label: &str,
    pct: u8,
    display_value: &str,
    on: bool,
) {
    let toggle_rect = WidgetRect::new(x1 - FILLMASTER_CHECK, y + (ROW_H - FILLMASTER_CHECK) / 2.0, FILLMASTER_CHECK, FILLMASTER_CHECK);
    let slider_right = toggle_rect.x - FILLMASTER_GAP - FILLMASTER_PCT_W - FILLMASTER_GAP;

    let cfg = SliderConfig::new(0.0, 100.0).with_step(1.0).without_input();
    let rect = WidgetRect::new(x0, y, (slider_right - x0).max(80.0), ROW_H);
    let res = render_single_slider(ctx, &cfg, pct as f64, rect, label, widget_theme, false, None);

    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(display_value, res.track_rect.right() + FILLMASTER_GAP, y + ROW_H / 2.0);

    draw_checkbox(ctx, toggle_rect, on, toolbar_theme, &toolbar_theme.background.clone());
}

// =============================================================================
// Table — Row::Table
// =============================================================================

pub(super) fn table_height(n_data_rows: usize) -> f64 {
    (n_data_rows + 1) as f64 * ROW_H
}

/// [`super::spec::Row::Table`] — header row + data rows, columns split
/// evenly (the spec carries no per-column width). [`Cell::Check`] is
/// interactive (task requirement): reads/registers through
/// `ConceptModalState::chk`, sub-keyed by `(row, col)` — the same
/// compound-key pattern `render.rs`'s own `draw_chk3_row` already uses for
/// its own per-item sub-keys.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_table_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y0: f64,
    state: &ConceptModalState,
    key: &str,
    head: &[String],
    rows: &[Vec<Cell>],
) {
    let n_cols = head.len().max(1);
    let col_w = (x1 - x0) / n_cols as f64;

    ctx.set_font("10.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    for (c, h) in head.iter().enumerate() {
        ctx.fill_text(h, x0 + c as f64 * col_w + 4.0, y0 + ROW_H / 2.0);
    }
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x0, y0 + ROW_H);
    ctx.line_to(x1, y0 + ROW_H);
    ctx.stroke();

    for (r, row) in rows.iter().enumerate() {
        let y = y0 + (r + 1) as f64 * ROW_H;
        for (c, cell) in row.iter().enumerate() {
            let rect = WidgetRect::new(x0 + c as f64 * col_w, y, col_w, ROW_H);
            draw_table_cell(ctx, toolbar_theme, result, rect, state, key, r, c, cell);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_table_cell(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    state: &ConceptModalState,
    key: &str,
    row: usize,
    col: usize,
    cell: &Cell,
) {
    match cell {
        Cell::Text(s) => {
            ctx.set_font("11.5px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(s, rect.x + 4.0, rect.center_y());
        }
        Cell::Toned(s, tone) => {
            let color = match tone {
                Tone::Good => toolbar_theme.success.clone(),
                Tone::Bad => toolbar_theme.danger.clone(),
                Tone::Neutral => toolbar_theme.item_text.clone(),
            };
            ctx.set_font("11.5px sans-serif");
            ctx.set_fill_color(&color);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(s, rect.x + 4.0, rect.center_y());
        }
        Cell::Swatch(theme_color) => {
            let hex = super::theme_color(toolbar_theme, *theme_color);
            let chip = WidgetRect::new(rect.x + 4.0, rect.y + (rect.height - 14.0) / 2.0, 14.0, 14.0);
            ctx.set_fill_color(hex);
            ctx.fill_rounded_rect(chip.x, chip.y, chip.width, chip.height, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(chip.x, chip.y, chip.width, chip.height, 3.0);
        }
        Cell::Check(default_on) => {
            let cell_key = format!("{key}::cell:{row}:{col}");
            let checked = state.chk(&cell_key, *default_on);
            let box_rect = WidgetRect::new(rect.x + (rect.width - 16.0) / 2.0, rect.y + (rect.height - 16.0) / 2.0, 16.0, 16.0);
            draw_checkbox(ctx, box_rect, checked, toolbar_theme, &toolbar_theme.background.clone());
            result.content_items.push((cell_key, box_rect));
        }
    }
}

// =============================================================================
// Cells — Row::Cells (the Table archetype's own editable grid)
// =============================================================================

const CELLS_ROW_H: f64 = 26.0;
const CELLS_GAP: f64 = 3.0;

pub(super) fn cells_height(n_rows: usize) -> f64 {
    n_rows.max(1) as f64 * CELLS_ROW_H + n_rows.saturating_sub(1) as f64 * CELLS_GAP
}

/// [`super::spec::Row::Cells`] — every cell (including the first row, which
/// happens to hold header-looking text) is its own inline-editable-looking
/// text field, exactly as `primitive_settings_cells::draw_grid` draws the
/// Table primitive's own cell grid — no header/data distinction in that
/// widget, and none here either. Static (not in the task's interactivity
/// list) — the app's own version wires live edits through
/// `PrimitiveSettingsState::active_field_id`, which a concept archetype has
/// no equivalent of.
pub(super) fn draw_cells_row(ctx: &mut dyn RenderContext, widget_theme: &WidgetTheme, x0: f64, x1: f64, y0: f64, cols: usize, rows: &[Vec<String>]) {
    let n_cols = cols.max(1);
    let cell_w = (((x1 - x0) - n_cols.saturating_sub(1) as f64 * CELLS_GAP) / n_cols as f64).max(0.0);
    for (r, row) in rows.iter().enumerate() {
        let y = y0 + r as f64 * (CELLS_ROW_H + CELLS_GAP);
        for c in 0..n_cols {
            let rect = WidgetRect::new(x0 + c as f64 * (cell_w + CELLS_GAP), y, cell_w, CELLS_ROW_H);
            let value = row.get(c).map(String::as_str).unwrap_or("");
            let cfg = InputConfig::new(value).with_type(InputType::Text).with_font_size(11.0).with_padding(4.0).with_radius(3.0);
            draw_input(ctx, &cfg, WidgetState::Normal, rect, widget_theme);
        }
    }
}

// =============================================================================
// Icons — Row::Icons
// =============================================================================

const ICON_CHIP_W: f64 = 30.0;
const ICON_CHIP_H: f64 = 26.0;
const ICON_CHIP_GAP: f64 = 4.0;
/// Columns-per-line assumed for the height reservation pass (before the
/// modal's real content width is settled) — same "safe direction" tradeoff
/// `primitive_settings_placement`'s own `CHIP_ROW_LINES_ESTIMATE` accepts
/// for the identical chicken-and-egg problem (wrap width needed before wrap
/// width is known). A narrower assumption than the min resizable content
/// width actually allows, so the reserved height is never short.
const ICONS_ESTIMATE_COLS: usize = 8;

pub(super) fn icons_height(n_options: usize) -> f64 {
    let cols = ICONS_ESTIMATE_COLS.max(1);
    let lines = n_options.max(1).div_ceil(cols).max(1) as f64;
    lines * (ICON_CHIP_H + ICON_CHIP_GAP) - ICON_CHIP_GAP
}

/// [`super::spec::Row::Icons`] — a wrapping grid of glyph chips, using the
/// real [`draw_toggle_segment`] widget per chip (same pill/segment styling
/// `Row::Mode`/`Row::Pills` already draw with in `render.rs`) laid out with
/// a manual wrap loop, the same technique
/// `primitive_settings_placement::draw_timeframes_group` already uses for
/// its own chip row (`draw_pill` there, `draw_toggle_segment` here — no
/// dedicated WRAPPING chip-grid widget exists in the app to call instead).
/// Interactive (task requirement): reads through
/// `ConceptModalState::segment`, the same accessor `Row::Mode`/`Row::Pills`
/// already use — an icon grid is "one of N, by index" too, despite drawing
/// a grid instead of a single line.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_icons_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y0: f64,
    state: &ConceptModalState,
    key: &str,
    options: &[String],
    default_selected: usize,
) {
    let selected = state.segment(key, default_selected);
    let mut cx = x0;
    let mut cy = y0;
    for (i, opt) in options.iter().enumerate() {
        if cx + ICON_CHIP_W > x1 && cx > x0 {
            cx = x0;
            cy += ICON_CHIP_H + ICON_CHIP_GAP;
        }
        let rect = WidgetRect::new(cx, cy, ICON_CHIP_W, ICON_CHIP_H);
        draw_toggle_segment(ctx, toolbar_theme, rect, opt, i == selected);
        result.content_items.push((format!("{key}::icons:{i}"), rect));
        cx += ICON_CHIP_W + ICON_CHIP_GAP;
    }
}

// =============================================================================
// Stepper — Row::Stepper
// =============================================================================

const STEP_BTN_W: f64 = 22.0;
const STEP_BTN_H: f64 = 20.0;
const STEP_COUNT_W: f64 = 32.0;

pub(super) fn stepper_height() -> f64 {
    ROW_H
}

/// [`super::spec::Row::Stepper`] — label, `-` button, value, `+` button, the
/// same shape `primitive_settings_cells::draw_stepper_row` draws (that
/// function is private, so this is a from-scratch redraw, not a copy of
/// unreachable code — the button chrome (`draw_step_button` below) mirrors
/// its own private twin byte-for-byte). Interactive (task requirement): the
/// live value reads through `ConceptModalState::stepper`; the two buttons
/// register under `{key}::stepper:dec`/`{key}::stepper:inc`, ready for a
/// future click dispatcher to call `ConceptModalState::bump_stepper` on
/// (wiring the actual `InputCoordinator` click response into a mutator call
/// is out of scope here — see `state.rs`'s own module doc: that step is
/// true for every interactive row in this file, not particular to this one).
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_stepper_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    label_col: f64,
    y: f64,
    state: &ConceptModalState,
    key: &str,
    label: &str,
    default_value: i32,
) {
    draw_label(ctx, toolbar_theme, x0, label_col, y, label);
    let value = state.stepper(key, default_value);
    let control_x = x0 + label_col;

    let dec_rect = WidgetRect::new(control_x, y + (ROW_H - STEP_BTN_H) / 2.0, STEP_BTN_W, STEP_BTN_H);
    draw_step_button(ctx, toolbar_theme, dec_rect, "\u{2212}");
    result.content_items.push((format!("{key}::stepper:dec"), dec_rect));

    let count_rect = WidgetRect::new(dec_rect.right() + 4.0, y, STEP_COUNT_W, ROW_H);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&value.to_string(), count_rect.center_x(), count_rect.center_y());

    let inc_rect = WidgetRect::new(count_rect.right() + 4.0, y + (ROW_H - STEP_BTN_H) / 2.0, STEP_BTN_W, STEP_BTN_H);
    draw_step_button(ctx, toolbar_theme, inc_rect, "+");
    result.content_items.push((format!("{key}::stepper:inc"), inc_rect));

    ctx.set_text_align(TextAlign::Left);
}

fn draw_step_button(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, rect: WidgetRect, glyph: &str) {
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(glyph, rect.center_x(), rect.center_y());
}

// =============================================================================
// Range — Row::Range
// =============================================================================

pub(super) fn range_height() -> f64 {
    RANGE_ROW_H
}

/// [`super::spec::Row::Range`] — one timeframe-visibility range row: class
/// label, min number field, a live (non-draggable — the real widget's own
/// simplification too, see its own doc) track with the selected span
/// painted in the accent colour, max number field. Exactly the shape
/// `primitive_settings_placement::draw_timeframes_group` draws for one of
/// its own numeric classes, using the SAME real field widget
/// ([`draw_number_field`]) and the SAME magic offsets (label reserves 62px,
/// fields are 40px) for visual parity. The domain is always `0..100` —
/// every `Row::Range` in `spec.rs` is built by `range(label, 0, 100, cap)`,
/// so this never needs a per-row domain the way the real widget's own
/// `NUMERIC_CLASSES` table carries one.
///
/// `cap` (e.g. `"1с–30с"`) is deliberately not drawn: the real widget this
/// mirrors has no such caption either — it is not a fifth element the task
/// brief's own enumeration ("class label, min number field, live track …,
/// max number field") asked for.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw_range_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    dummy_state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    key: &str,
    label: &str,
    from: i32,
    to: i32,
) {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x0, y + RANGE_ROW_H / 2.0);

    let field_w = 40.0;
    let min_rect = WidgetRect::new(x0 + 62.0, y + 2.0, field_w, RANGE_ROW_H - 4.0);
    let max_rect = WidgetRect::new(x1 - field_w, y + 2.0, field_w, RANGE_ROW_H - 4.0);
    draw_number_field(ctx, toolbar_theme, widget_theme, dummy_state, result, min_rect, &format!("{key}::range:min"), &from.to_string());
    draw_number_field(ctx, toolbar_theme, widget_theme, dummy_state, result, max_rect, &format!("{key}::range:max"), &to.to_string());

    let track_x0 = min_rect.right() + 6.0;
    let track_x1 = max_rect.x - 6.0;
    let track_w = (track_x1 - track_x0).max(0.0);
    let track_y = y + RANGE_ROW_H / 2.0 - 1.0;
    ctx.set_fill_color(&toolbar_theme.separator);
    ctx.fill_rounded_rect(track_x0, track_y, track_w, 2.0, 1.0);

    let span = 100.0_f64;
    let fx0 = track_x0 + track_w * (f64::from(from.clamp(0, 100)) / span);
    let fx1 = track_x0 + track_w * (f64::from(to.clamp(0, 100)) / span);
    ctx.set_fill_color(&toolbar_theme.accent);
    ctx.fill_rounded_rect(fx0, track_y, (fx1 - fx0).max(2.0), 2.0, 1.0);
}
