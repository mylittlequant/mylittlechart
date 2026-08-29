//! Primitive settings modal — "Cells" tab: the Table primitive's own editable
//! cell grid. The one primitive-specific tab in the strip (2026-08-09
//! modal-surface pass) — every other tab body renders SOMETHING for every
//! primitive that reaches it; this one only ever draws for `table` (the
//! only type `Primitive::table_rows()` returns `Some` for).
//!
//! Built as a bespoke COLLECTION, the same shape the level ladder already is
//! (see `primitive_settings_levels`'s own module doc): the cell grid is
//! never declared through `style_properties()`/`text_properties()` — it is
//! read through `Primitive::table_rows()` and written through the SAME
//! generic `PropertyKind::Text` dispatch every other text-family property
//! already uses (`mlc-app`'s `input/settings.rs`, `mlc-ui-lab`'s
//! `dispatch.rs`), landing on `Table::apply_text_property`'s own
//! `"cell_{row}_{col}"`/`"rows_count"`/`"cols_count"` match arms — unchanged
//! by this pass, only no longer ALSO declared as `LabelsText` rows (that
//! flat "Header 1"/"Cell [2,1]" list is what this tab replaces).
//!
//! Each cell renders as its own single-line inline-editable text input
//! (`cell_{row}_{col}`), using the exact same shared inline-edit mechanism
//! every other field in this modal uses: `PrimitiveSettingsState::
//! active_field_id` names the field currently focused, `active_render`
//! holds its live typed buffer, and the committed value (`Primitive::
//! table_rows()`, unless actively being edited) is what every other row
//! shows otherwise — mirrored 1:1 by `primitive_settings_levels::
//! draw_level_value_field`, right down to registering
//! `result.active_input_char_positions`/`active_input_rect` only while
//! editing.
//!
//! Two labelled stepper rows (Rows / Columns) sit above the grid, each a
//! `[-]`/count/`[+]` control — `cells_{row,col}_{dec,inc}` — that resize the
//! grid by one. They read `label_col` the same way every other labelled row
//! in this modal does (caption in the label column, control after it),
//! which is why this module's `draw` takes `label_col` even though the grid
//! itself, drawn full-width below them, does not need it.

use crate::layout::render_frame::PrimitiveSettingsResult;
use crate::ui::modal_settings::PrimitiveSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType, WidgetState, WidgetTheme};
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::i18n::{current_language, ConfigKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use super::primitive_settings_vid::draw_label;

const STEPPER_H: f64 = 26.0;
const STEPPER_GAP: f64 = 10.0;
const STEP_BTN_W: f64 = 22.0;
const STEP_BTN_H: f64 = 20.0;
const STEP_COUNT_W: f64 = 28.0;
const CELL_H: f64 = 26.0;
const CELL_GAP: f64 = 3.0;
const GROUP_PAD_BOTTOM: f64 = 8.0;

/// Whether this primitive has a Cells tab at all: only `table` today
/// (`Primitive::table_rows().is_some()`) — the single predicate
/// `crate::layout::modals::primitive_settings::available_tabs` calls, kept
/// next to `content_height`/`draw` so the three can never drift apart, the
/// same discipline every other tab body's own `has_content` already
/// follows.
pub fn has_content(prim: &dyn Primitive) -> bool {
    prim.table_rows().is_some()
}

/// The row/column counts this tab is about to draw, clamped to at least one
/// each — `Table::apply_text_property`'s own `rows_count`/`cols_count`
/// clamps (`1..=20`/`1..=10`) already keep the real data from going to
/// zero; this is only a defensive floor for the layout math below, not a
/// second copy of that clamp.
fn grid_dims(rows: &[Vec<String>]) -> (usize, usize) {
    let n_rows = rows.len().max(1);
    let n_cols = rows.first().map(Vec::len).unwrap_or(1).max(1);
    (n_rows, n_cols)
}

/// Static — does not shrink for a live settings-search query (see
/// [`content_height_for_query`]), the same tradeoff every other tab body's
/// own `content_height` makes.
pub fn content_height(prim: &dyn Primitive) -> f64 {
    let Some(rows) = prim.table_rows() else { return 0.0 };
    let (n_rows, _) = grid_dims(&rows);
    grid_body_height(n_rows) + GROUP_PAD_BOTTOM
}

fn grid_body_height(n_rows: usize) -> f64 {
    2.0 * STEPPER_H + STEPPER_GAP + n_rows as f64 * CELL_H + n_rows.saturating_sub(1) as f64 * CELL_GAP
}

/// Query-aware body height for the cross-tab settings-search result view
/// (mirrors every other tab body's own `content_height_for_query`): the
/// whole tab is all-or-nothing against the query, the same "a coordinate
/// list going half-filtered would read as broken, not filtered" reasoning
/// `primitive_settings_placement`'s own points group uses — matched against
/// the tab's own name plus the "Rows"/"Columns" stepper captions, not
/// individual cell CONTENTS (those are data, not settings names).
pub fn content_height_for_query(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    let query = super::primitive_settings::search_query(state);
    if query.is_empty() {
        return content_height(prim);
    }
    if tab_matches(&query) {
        content_height(prim)
    } else {
        0.0
    }
}

fn tab_matches(query: &str) -> bool {
    let lang = current_language();
    super::primitive_settings::search_matches(
        query,
        &[ConfigKey::TabCells.get(lang), ConfigKey::Rows.get(lang), ConfigKey::Columns.get(lang)],
    )
}

/// Draws the Cells tab's own rows: the Rows/Columns stepper pair, then the
/// live editable grid. Returns `true` if it drew anything — mirrors every
/// other tab body's own convention (the caller, the cross-tab search
/// combiner in `primitive_settings.rs`, uses it to decide whether to show
/// this tab's own header there).
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
    let Some(rows) = prim.table_rows() else { return false };
    let query = super::primitive_settings::search_query(state);
    if !tab_matches(&query) {
        return false;
    }
    let lang = current_language();
    let (n_rows, n_cols) = grid_dims(&rows);

    draw_stepper_row(ctx, toolbar_theme, result, x0, label_col, *row_y, ConfigKey::Rows.get(lang), n_rows, "cells_row_dec", "cells_row_inc");
    *row_y += STEPPER_H;
    draw_stepper_row(ctx, toolbar_theme, result, x0, label_col, *row_y, ConfigKey::Columns.get(lang), n_cols, "cells_col_dec", "cells_col_inc");
    *row_y += STEPPER_H + STEPPER_GAP;

    draw_grid(ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y, &rows, n_rows, n_cols);
    *row_y += n_rows as f64 * CELL_H + n_rows.saturating_sub(1) as f64 * CELL_GAP;
    *row_y += GROUP_PAD_BOTTOM;
    true
}

// =============================================================================
// Rows / Columns steppers
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_stepper_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    label_col: f64,
    y: f64,
    label: &str,
    count: usize,
    dec_id: &str,
    inc_id: &str,
) {
    draw_label(ctx, toolbar_theme, x0, label_col, y, label);
    let control_x = x0 + label_col;

    let dec_rect = WidgetRect::new(control_x, y + (STEPPER_H - STEP_BTN_H) / 2.0, STEP_BTN_W, STEP_BTN_H);
    draw_step_button(ctx, toolbar_theme, dec_rect, "\u{2212}");
    result.content_items.push((dec_id.to_string(), dec_rect));

    let count_rect = WidgetRect::new(dec_rect.right() + 4.0, y, STEP_COUNT_W, STEPPER_H);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&count.to_string(), count_rect.center_x(), count_rect.center_y());

    let inc_rect = WidgetRect::new(count_rect.right() + 4.0, y + (STEPPER_H - STEP_BTN_H) / 2.0, STEP_BTN_W, STEP_BTN_H);
    draw_step_button(ctx, toolbar_theme, inc_rect, "+");
    result.content_items.push((inc_id.to_string(), inc_rect));

    // Restore left alignment for whatever draws next — the grid's own cell
    // inputs never set their own alignment before drawing (`draw_input`
    // reads the theme's default), so leaving `Center` behind here would
    // misalign the very first cell drawn after this row.
    ctx.set_text_align(TextAlign::Left);
}

fn draw_step_button(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, rect: WidgetRect, glyph: &str) {
    crate::ui::overlay_fill::fill_modal_control(
        ctx,
        &toolbar_theme.dropdown_bg,
        &toolbar_theme.background,
        rect.x, rect.y, rect.width, rect.height,
        3.0,
    );
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
// The grid
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_grid(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    row_y: f64,
    rows: &[Vec<String>],
    n_rows: usize,
    n_cols: usize,
) {
    let cell_w = ((x1 - x0) - n_cols.saturating_sub(1) as f64 * CELL_GAP) / n_cols as f64;
    for r in 0..n_rows {
        let y = row_y + r as f64 * (CELL_H + CELL_GAP);
        let row = rows.get(r);
        for c in 0..n_cols {
            let cell_x = x0 + c as f64 * (cell_w + CELL_GAP);
            let rect = WidgetRect::new(cell_x, y, cell_w, CELL_H);
            let value = row.and_then(|r| r.get(c)).map(String::as_str).unwrap_or("");
            draw_cell_field(ctx, toolbar_theme, widget_theme, state, result, rect, r, c, value);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_cell_field(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    row: usize,
    col: usize,
    value: &str,
) {
    let field_id = format!("cell_{row}_{col}");
    let is_editing = state.active_field_id.as_deref() == Some(field_id.as_str());
    let ar = &state.active_render;
    let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
        (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
    } else {
        (value, value.len(), None, None)
    };
    let cfg = InputConfig::new(display)
        .with_focused(is_editing)
        .with_type(InputType::Text)
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
    result.content_items.push((field_id, rect));
}
