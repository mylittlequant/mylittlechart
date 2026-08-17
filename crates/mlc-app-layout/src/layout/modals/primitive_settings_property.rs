//! Generic declared-property rendering, shared by every primitive-settings
//! tab body (`primitive_settings_vid`/`_levels`/`_placement`).
//!
//! This is the piece that makes the modal render **what a primitive type
//! declares** instead of a layout the tab bodies used to hardcode: a type
//! exposes its settings as `Vec<ConfigProperty>` (`Primitive::
//! style_properties()`/`text_properties()`), each one carrying a declared
//! `PropertyCategory` (which tab + group it belongs to) and `order`
//! (position within that group). A tab body buckets the primitive's own
//! declared properties by category, sorts each bucket by `order`, and
//! renders every row from its own `PropertyType` — colour swatch, slider,
//! checkbox, cycle button, segmented pills (or the same cycle button when
//! the row's own options do not fit — see the `Pills` arm), a wrapping grid
//! of glyph cells (the `Picker` arm, W4 — the ONE `PropertyType` here that is
//! not one row tall; see `primitive_settings_vid::property_row_height`),
//! single-line input — via the helpers already used for the base
//! `PrimitiveData` fields (`primitive_settings_vid::{draw_label,
//! draw_bool_row, draw_slider_row, draw_split_value_control,
//! draw_toggle_segment, line_style_display_name}`). A category nobody
//! declared into collects zero properties, so the caller's
//! `if !rows.is_empty()` naturally draws nothing for it — no header, no gap,
//! no reserved height.
//!
//! What this module deliberately does NOT decide: which categories a given
//! tab looks for (each tab body's own small fixed list — see their own
//! `content_height`/`draw`), or anything that is not `ConfigProperty`-shaped
//! at all (the level ladder, palette cards, points table, timeframe chips,
//! the universal base-stroke row, the raw-`TextBlock` content/style/position
//! block) — those stay bespoke in the tab that owns them, documented there.

use mlc_core::drawing::primitives_v2::config::{ConfigProperty, PropertyCategory, PropertyGroup, PropertyType, PropertyValue, StatRow, TableCell, Tone};
use mlc_core::drawing::primitives_v2::{Primitive, PropertyKind};
use mlc_core::drawing::LineStyle;
use mlc_core::i18n::current_language;
use mlc_core::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use crate::ui::modal_settings::PrimitiveSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType, WidgetTheme};
use crate::layout::render_frame::PrimitiveSettingsResult;

use super::primitive_settings_intent::{
    icon_picker_grid, pills_fits, pills_segments, table_grid, TableCellView, ICON_PICKER_CELL_GAP, ICON_PICKER_CELL_H,
    ICON_PICKER_CELL_W, PILLS_SEGMENT_GAP,
};
use super::primitive_settings_vid::{
    draw_bool_row, draw_label, draw_slider_row, draw_split_value_control, draw_toggle_segment,
    line_style_display_name, SplitValueKind, ROW_H,
};

/// One declared property plus which generically-dispatchable family it came
/// from (`Primitive::style_properties()` vs `text_properties()` vs
/// `level_properties()`) — the family decides the `style_prop:`/`text_prop:`/
/// `level_prop:` id prefix `apply_property` (both hosts: `mlc-app`'s
/// `input/settings.rs`, `mlc-ui-lab`'s `dispatch.rs`) already dispatches
/// generically on for ANY id.
pub(crate) type DeclaredProperty = (ConfigProperty, PropertyKind);

/// Every property a primitive declares across all three generically-
/// dispatchable families, in declaration order (NOT yet grouped or sorted —
/// callers pick their own categories out via [`properties_in`]).
///
/// `level_properties()` was folded in 2026-08-09 — it used to be excluded
/// with a doc claiming `primitive_settings_levels` read it directly for a
/// bespoke top section; that was never true (verified by reading all of
/// `primitive_settings_levels.rs`), so every override of it (Pitchfork's
/// `level_mode`) was declared but unreachable from the UI. `PropertyKind`
/// already had the third `Level` variant and `DrawingManager::apply_property`
/// already dispatched it to `apply_level_property` — only this fold-in and
/// the `level_prop:` prefix in [`field_id`] were missing.
pub(crate) fn declared_properties(prim: &dyn Primitive) -> Vec<DeclaredProperty> {
    let mut all: Vec<DeclaredProperty> =
        prim.style_properties().into_iter().map(|p| (p, PropertyKind::Style)).collect();
    if let Some(text_props) = prim.text_properties() {
        all.extend(text_props.into_iter().map(|p| (p, PropertyKind::Text)));
    }
    all.extend(prim.level_properties().into_iter().map(|p| (p, PropertyKind::Level)));
    all
}

/// The properties declared into `category`, sorted by their own `order` —
/// exactly what one group in one tab renders. Empty means the group emits
/// nothing (no header, no gap, zero height) — the caller decides that by
/// checking `.is_empty()`, this function never does.
pub(crate) fn properties_in(all: &[DeclaredProperty], category: PropertyCategory) -> Vec<&DeclaredProperty> {
    let mut found: Vec<&DeclaredProperty> = all.iter().filter(|(p, _)| p.category == category).collect();
    found.sort_by_key(|(p, _)| p.order);
    found
}

/// Buckets one category's own already-`order`-sorted rows
/// (`properties_in`'s output, typically filtered by a live search query
/// first) by their declared [`PropertyGroup`] — the W2 group model
/// (`mlc_core`'s `PropertyGroup` doc,
/// `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`). The
/// `None` bucket — the category's own DEFAULT run, whatever header (or lack
/// of one) that category already drew before `group` existed — is always
/// FIRST, even when empty, so a caller can always match on it without an
/// `Option` dance; every other value follows, in the order its first row
/// appears among `props` — the convention every migrated declaration
/// follows: a property meant to stay in the default run keeps a LOWER
/// `order` than the ones pulled into a named group, so "first row declares
/// the order" and "default run first" agree in practice.
pub(crate) fn group_runs<'a>(props: &[&'a DeclaredProperty]) -> Vec<(Option<PropertyGroup>, Vec<&'a DeclaredProperty>)> {
    bucket_by_group(props, |dp| dp.0.group).into_iter().map(|(g, rows)| (g, rows.into_iter().copied().collect())).collect()
}

/// The generic bucketing [`group_runs`] wraps for `&DeclaredProperty` pairs
/// — shared with `primitive_settings_levels`'s own `BaseGroupRows::Declared`
/// (`Vec<ConfigProperty>`, not a `DeclaredProperty` pair), which calls this
/// directly rather than through a second copy of the same loop. `group_of`
/// extracts whichever item's own declared `PropertyGroup`; the `None`
/// bucket is always first, even when empty, same contract as `group_runs`.
pub(crate) fn bucket_by_group<'a, T>(
    items: &'a [T],
    group_of: impl Fn(&T) -> Option<PropertyGroup>,
) -> Vec<(Option<PropertyGroup>, Vec<&'a T>)> {
    let mut order: Vec<Option<PropertyGroup>> = vec![None];
    let mut buckets: std::collections::HashMap<Option<PropertyGroup>, Vec<&'a T>> = std::collections::HashMap::new();
    buckets.insert(None, Vec::new());
    for item in items {
        let g = group_of(item);
        if g.is_some() && !buckets.contains_key(&g) {
            order.push(g);
        }
        buckets.entry(g).or_default().push(item);
    }
    order.into_iter().map(|g| { let rows = buckets.remove(&g).unwrap_or_default(); (g, rows) }).collect()
}

/// A declared property's own effective header text for cross-tab settings
/// search: its named [`PropertyGroup`]'s label when it has one, otherwise
/// `default_header` (whatever fixed caption the category's own default run
/// shows, or would show if it drew a header at all). Every tab body's own
/// search-match filter uses this instead of always matching against
/// `default_header`, so a query naming a row's ACTUAL on-screen header
/// (named group or default) finds it — never the wrong one.
pub(crate) fn effective_header(prop: &ConfigProperty, default_header: &'static str) -> &'static str {
    prop.group.map(|g| g.label()).unwrap_or(default_header)
}

/// The `content_items` field id a declared property's control registers
/// under. The three base `PrimitiveData` fields keep the bare id the click
/// dispatch already special-cases for them regardless of which tab drew the
/// row (`mlc-app`'s `input/settings.rs`, pre-existing, not introduced by
/// this module); every other declared property goes through the generic
/// `style_prop:{id}`/`text_prop:{id}` dispatch, keyed by which family it
/// came from.
fn field_id(kind: PropertyKind, prop_id: &str) -> String {
    if matches!(prop_id, "stroke_color" | "stroke_width" | "line_style") {
        return prop_id.to_string();
    }
    match kind {
        PropertyKind::Text => format!("text_prop:{prop_id}"),
        PropertyKind::Style => format!("style_prop:{prop_id}"),
        PropertyKind::Level => format!("level_prop:{prop_id}"),
    }
}

/// Draw one declared property as a single row at `y`, dispatched by its own
/// `PropertyType`. Does not advance `y` itself — every caller advances by
/// the same `ROW_H` every other row in these tabs uses (except `Stat`, see
/// [`crate::layout::modals::primitive_settings_vid::property_row_height`]).
///
/// `PropertyType::Stat` (W4, a computed-stat BLOCK — e.g. Position's own
/// Risk/Target/R:R) is checked FIRST, ahead of the generic `readonly`
/// branch below: its own arm never pushes into `result.content_items`, by
/// construction — inertness is a property of THIS arm's own code, not of
/// the `readonly` flag a caller happened to set. See
/// `mlc_core::drawing::primitives_v2::config::PropertyType::Stat`'s own doc.
///
/// A `readonly` property that is NOT `Stat` (`ConfigProperty::readonly`,
/// today none — every readonly declarer in this crate is `Stat`) still
/// short-circuits every other `PropertyType` arm below: it renders as a
/// plain caption + value row, muted text, no control, nothing pushed into
/// `result.content_items` either — kept as a second, generic fallback for
/// any future readonly scalar property that is not a `Stat` block.
///
/// `PropertyType::FibLevels`/`Coordinate`/`TimeframeVisibility` are not
/// produced by any `style_properties()`/`text_properties()` implementation
/// today (verified against the whole crate) — this draws nothing for them
/// rather than invent a control shape this modal has no widget for yet. A
/// caller still reserves the row's `ROW_H` for it, so a future one that
/// DOES appear is not silently clipped — only unstyled until this module
/// grows a real arm for it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_property_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    kind: PropertyKind,
    prop: &ConfigProperty,
) {
    let lang = current_language();
    let fid = field_id(kind, &prop.id);

    if let PropertyType::Stat { rows } = &prop.prop_type {
        draw_stat_block_row(ctx, toolbar_theme, x0, x1, label_col, y, rows);
        return;
    }

    // `PropertyType::Table` (W4) is checked BEFORE the generic `readonly`
    // branch below — same reasoning as `Stat`'s own early check: this row
    // IS interactive (some of its own cells write back through
    // `result.content_items`), so it must never fall into the plain
    // caption+value row `readonly` produces. `ConfigProperty::table` marks
    // the WHOLE property `readonly` anyway (excludes it from template
    // capture, see that constructor's own doc) — this early return is what
    // keeps that flag from also disabling the row's own interactivity.
    if let PropertyType::Table { head, rows } = &prop.prop_type {
        draw_table_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, y, &fid, head, rows);
        return;
    }

    if prop.readonly {
        draw_readonly_row(ctx, toolbar_theme, x0, x1, label_col, y, &prop.name, &readonly_value_text(&prop.value));
        return;
    }

    match &prop.prop_type {
        PropertyType::Color => {
            draw_label(ctx, toolbar_theme, x0, label_col, y, &prop.name);
            let swatch_w = (x1 - (x0 + label_col)).clamp(40.0, 60.0);
            let rect = WidgetRect::new(x0 + label_col, y + 3.0, swatch_w, ROW_H - 6.0);
            let color = prop.value.as_color().unwrap_or("#888888");
            ctx.set_fill_color(color);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
            result.content_items.push((fid, rect));
        }
        PropertyType::Number { min, max, .. } => {
            let value = prop.value.as_number().unwrap_or(0.0);
            let lo = min.unwrap_or(0.0);
            let hi = max.unwrap_or(100.0);
            draw_number_slider(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, y, &fid, &prop.name, value, lo, hi);
        }
        // `PropertyType::Integer` is never constructed by any
        // `style_properties()`/`text_properties()` implementation today
        // (verified against the whole crate) — handled the same as
        // `Number` rather than silently dropped, in case a future type
        // does declare one.
        PropertyType::Integer { min, max } => {
            let value = match &prop.value {
                mlc_core::drawing::primitives_v2::config::PropertyValue::Integer(i) => *i as f64,
                other => other.as_number().unwrap_or(0.0),
            };
            let lo = min.map(|v| v as f64).unwrap_or(0.0);
            let hi = max.map(|v| v as f64).unwrap_or(100.0);
            draw_number_slider(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, y, &fid, &prop.name, value, lo, hi);
        }
        PropertyType::Boolean => {
            let checked = prop.value.as_bool().unwrap_or(false);
            draw_bool_row(ctx, toolbar_theme, result, x0, label_col, y, &prop.name, checked, &fid);
        }
        PropertyType::LineStyle => {
            draw_label(ctx, toolbar_theme, x0, label_col, y, &prop.name);
            let value = prop.value.as_string().unwrap_or("solid");
            let style = LineStyle::from_str(value);
            let display = line_style_display_name(style, lang);
            let control_w = (x1 - (x0 + label_col)).max(100.0);
            let rect = WidgetRect::new(x0 + label_col, y + 2.0, control_w, ROW_H - 4.0);
            draw_split_value_control(ctx, toolbar_theme, state, result, rect, &fid, display, SplitValueKind::LineStyle(style));
        }
        PropertyType::Select { options } => {
            draw_label(ctx, toolbar_theme, x0, label_col, y, &prop.name);
            let current = prop.value.as_string().unwrap_or("").to_string();
            let display = options.iter().find(|o| o.value == current).map(|o| o.label.clone()).unwrap_or_else(|| current.clone());
            let control_w = (x1 - (x0 + label_col)).max(100.0);
            let rect = WidgetRect::new(x0 + label_col, y + 2.0, control_w, ROW_H - 4.0);
            let opts: Vec<(String, String)> = options.iter().map(|o| (o.value.clone(), o.label.clone())).collect();
            draw_split_value_control(
                ctx, toolbar_theme, state, result, rect, &fid, &display,
                SplitValueKind::Select { options: opts, current },
            );
        }
        // A `Pills` row draws its own options as adjacent segments when
        // they fit the row's own width (`pills_fits`), and degrades to the
        // exact same control a `Select` row draws otherwise (W4's own
        // "fall back honestly" rule — never overflow the modal). Either
        // way the click id a segment or dropdown option registers is
        // `{fid}::opt:{value}` — the SAME id a `Select` row's own dropdown
        // option already registers (`draw_value_dropdown`), so both shapes
        // reach the existing `"::opt:"` apply path with zero new dispatch
        // code (see `pills_segments`'s own doc).
        PropertyType::Pills { options } => {
            draw_label(ctx, toolbar_theme, x0, label_col, y, &prop.name);
            let current = prop.value.as_string().unwrap_or("").to_string();
            let control_w = (x1 - (x0 + label_col)).max(100.0);
            if pills_fits(options.len(), control_w) {
                let n = options.len() as f64;
                let gap_total = PILLS_SEGMENT_GAP * (n - 1.0).max(0.0);
                let seg_w = ((control_w - gap_total) / n).max(0.0);
                let mut seg_x = x0 + label_col;
                for seg in pills_segments(&fid, options, &current) {
                    let rect = WidgetRect::new(seg_x, y + 2.0, seg_w, ROW_H - 4.0);
                    draw_toggle_segment(ctx, toolbar_theme, rect, &seg.label, seg.active);
                    result.content_items.push((seg.click_id, rect));
                    seg_x += seg_w + PILLS_SEGMENT_GAP;
                }
            } else {
                let display = options.iter().find(|o| o.value == current).map(|o| o.label.clone()).unwrap_or_else(|| current.clone());
                let rect = WidgetRect::new(x0 + label_col, y + 2.0, control_w, ROW_H - 4.0);
                let opts: Vec<(String, String)> = options.iter().map(|o| (o.value.clone(), o.label.clone())).collect();
                draw_split_value_control(
                    ctx, toolbar_theme, state, result, rect, &fid, &display,
                    SplitValueKind::Select { options: opts, current },
                );
            }
        }
        // A `Picker` row draws its own catalogue as a grid of glyph cells
        // spanning the FULL row width (`x0..x1`, not offset by `label_col`
        // — there is no per-row caption, the group header above it already
        // names the row, same as the ladder's own palette cards). Wraps to
        // as many columns as `icon_picker_grid` fits `x1 - x0` and grows
        // downward — the caller (`primitive_settings_vid`'s `draw_line_run`)
        // advances `row_y` by [`super::primitive_settings_vid::property_row_height`]
        // rather than the flat `ROW_H` every other `PropertyType` needs,
        // computed from the SAME `icon_picker_grid` so the space reserved
        // for this row and what is actually painted here can never
        // disagree (see that function's own doc).
        PropertyType::Picker { options } => {
            let current = prop.value.as_string().unwrap_or("").to_string();
            let control_w = (x1 - x0).max(0.0);
            let grid = icon_picker_grid(&fid, options, &current, control_w);
            let columns = grid.columns.max(1);
            let cell_step_x = ICON_PICKER_CELL_W + ICON_PICKER_CELL_GAP;
            let cell_step_y = ICON_PICKER_CELL_H + ICON_PICKER_CELL_GAP;
            for (i, cell) in grid.cells.iter().enumerate() {
                let col = i % columns;
                let row = i / columns;
                let rect = WidgetRect::new(
                    x0 + col as f64 * cell_step_x,
                    y + row as f64 * cell_step_y,
                    ICON_PICKER_CELL_W,
                    ICON_PICKER_CELL_H,
                );
                ctx.set_fill_color(if cell.active { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
                ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
                let tint = if cell.active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted };
                let glyph = 16.0;
                let gx = rect.x + (rect.width - glyph) / 2.0;
                let gy = rect.y + (rect.height - glyph) / 2.0;
                draw_svg_icon(ctx, &cell.svg, gx, gy, glyph, glyph, tint);
                result.content_items.push((cell.click_id.clone(), rect));
            }
        }
        PropertyType::Text { .. } => {
            draw_label(ctx, toolbar_theme, x0, label_col, y, &prop.name);
            let is_editing = state.active_field_id.as_deref() == Some(fid.as_str());
            let committed = prop.value.as_string().unwrap_or("").to_string();
            let ar = &state.active_render;
            let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
                (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
            } else {
                (committed.as_str(), committed.len(), None, None)
            };
            let control_w = (x1 - (x0 + label_col)).max(100.0);
            let rect = WidgetRect::new(x0 + label_col, y + 2.0, control_w, ROW_H - 4.0);
            let cfg = InputConfig::new(display)
                .with_focused(is_editing)
                .with_type(InputType::Text)
                .with_font_size(11.5)
                .with_padding(4.0)
                .with_radius(3.0)
                .with_cursor(cursor)
                .with_selection(sel_start, sel_end);
            let res = draw_input(ctx, &cfg, crate::ui::widgets::WidgetState::Normal, rect, widget_theme);
            if is_editing {
                draw_input_cursor(ctx, res.cursor_x, res.cursor_y, res.cursor_height, &toolbar_theme.item_text);
                result.active_input_char_positions = res.char_x_positions;
                result.active_input_rect = Some(rect);
            }
            result.content_items.push((fid, rect));
        }
        // `Stat`/`Table` are handled by their own early `if let`s above and
        // can never reach here — listed for exhaustiveness only, same
        // treatment as the three genuinely-unused types beside them.
        PropertyType::Stat { .. }
        | PropertyType::Table { .. }
        | PropertyType::FibLevels
        | PropertyType::Coordinate
        | PropertyType::TimeframeVisibility => {}
    }
}

/// A `readonly` property's own row shape: the caption in the label column
/// (same [`draw_label`] every other row uses), the value right-aligned
/// against `x1` in the muted text colour — no control, nothing pushed into
/// `result.content_items` (see [`draw_property_row`]'s own doc for why:
/// there is nothing here to click, hover or drag).
fn draw_readonly_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    label: &str,
    value_text: &str,
) {
    draw_label(ctx, toolbar_theme, x0, label_col, y, label);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Right);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(value_text, x1, y + ROW_H / 2.0);
    // Restore left alignment — every other row in this modal assumes it,
    // the same discipline `primitive_settings_levels::draw_ramps_row`
    // documents for its own `Center` alignment.
    ctx.set_text_align(TextAlign::Left);
}

/// A [`PropertyType::Stat`] block's own row shape (W4) — one line per
/// [`StatRow`], stacked downward from `y`: the row's own label in the
/// label column (same [`draw_label`] every other row uses), its value
/// right-aligned against `x1`, coloured by `tone` — the theme's own
/// `success`/`danger` fields for `Good`/`Bad` (the SAME colours a stop/
/// target zone already uses elsewhere in this modal — no new palette),
/// `item_text` for `Neutral`/untoned (a plain reading, not muted — this is
/// live information, not a placeholder). Pushes nothing into
/// `result.content_items`: there is no click id for a value that cannot be
/// applied — see [`draw_property_row`]'s own doc.
fn draw_stat_block_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    rows: &[StatRow],
) {
    for (i, row) in rows.iter().enumerate() {
        let row_y = y + i as f64 * ROW_H;
        draw_label(ctx, toolbar_theme, x0, label_col, row_y, &row.label);
        let color = match row.tone {
            Some(Tone::Good) => &toolbar_theme.success,
            Some(Tone::Bad) => &toolbar_theme.danger,
            Some(Tone::Neutral) | None => &toolbar_theme.item_text,
        };
        ctx.set_font("11.5px sans-serif");
        ctx.set_fill_color(color);
        ctx.set_text_align(TextAlign::Right);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&row.value, x1, row_y + ROW_H / 2.0);
        ctx.set_text_align(TextAlign::Left);
    }
}

/// A [`PropertyType::Table`] row's own shape (W4) — a header line of column
/// captions, followed by one line per data row, each cell dispatched by its
/// own [`TableCell`] variant via [`table_grid`]'s own pure decision. Grows
/// downward exactly like [`draw_stat_block_row`]: `(rows.len() + 1) *
/// ROW_H`, reserved by `primitive_settings_vid::property_row_height`'s own
/// `Table` arm off the SAME `rows` this draws, so the two can never
/// disagree. Columns split the row's own full width evenly (`x0..x1`, not
/// offset by `label_col` — like `Picker`, there is no per-row caption, the
/// group header above it already names the table).
#[allow(clippy::too_many_arguments)]
fn draw_table_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    fid: &str,
    head: &[String],
    rows: &[Vec<TableCell>],
) {
    let n_cols = head.len().max(1);
    let col_w = ((x1 - x0) / n_cols as f64).max(1.0);

    ctx.set_font("10.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    for (c, label) in head.iter().enumerate() {
        ctx.fill_text(label, x0 + c as f64 * col_w + 4.0, y + ROW_H / 2.0);
    }

    for (r, row) in table_grid(fid, rows).into_iter().enumerate() {
        let row_y = y + (r + 1) as f64 * ROW_H;
        for (c, view) in row.into_iter().enumerate() {
            let cx = x0 + c as f64 * col_w;
            let rect = WidgetRect::new(cx + 2.0, row_y + 2.0, (col_w - 4.0).max(0.0), ROW_H - 4.0);
            draw_table_cell(ctx, toolbar_theme, widget_theme, state, result, rect, &view);
        }
    }
}

/// One [`TableCellView`]'s own drawn shape — dispatched by its own
/// [`TableCell`] variant. Only `EditableText`/`EditableSwatch`/
/// `EditableCheck` ever push into `result.content_items` (via
/// `view.click_id`, computed once by [`table_grid`]) — the other four
/// variants never do, structurally, the same discipline the `Stat` block's
/// own arm follows.
fn draw_table_cell(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    view: &TableCellView,
) {
    match &view.cell {
        TableCell::Text(s) => draw_table_text(ctx, rect, s, &toolbar_theme.item_text),
        TableCell::Toned(s, tone) => {
            let color = match tone {
                Tone::Good => &toolbar_theme.success,
                Tone::Bad => &toolbar_theme.danger,
                Tone::Neutral => &toolbar_theme.item_text,
            };
            draw_table_text(ctx, rect, s, color);
        }
        TableCell::EditableText(committed) => {
            let is_editing = view.click_id.is_some() && state.active_field_id.as_deref() == view.click_id.as_deref();
            let ar = &state.active_render;
            let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
                (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
            } else {
                (committed.as_str(), committed.len(), None, None)
            };
            let cfg = InputConfig::new(display)
                .with_focused(is_editing)
                .with_type(InputType::Text)
                .with_font_size(11.0)
                .with_padding(3.0)
                .with_radius(3.0)
                .with_cursor(cursor)
                .with_selection(sel_start, sel_end);
            let res = draw_input(ctx, &cfg, crate::ui::widgets::WidgetState::Normal, rect, widget_theme);
            if is_editing {
                draw_input_cursor(ctx, res.cursor_x, res.cursor_y, res.cursor_height, &toolbar_theme.item_text);
                result.active_input_char_positions = res.char_x_positions;
                result.active_input_rect = Some(rect);
            }
            if let Some(id) = &view.click_id {
                result.content_items.push((id.clone(), rect));
            }
        }
        TableCell::Swatch(color) | TableCell::EditableSwatch(color) => {
            let side = rect.height.min(rect.width);
            let swatch = WidgetRect::new(rect.x, rect.y, side, side);
            ctx.set_fill_color(color);
            ctx.fill_rounded_rect(swatch.x, swatch.y, swatch.width, swatch.height, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(swatch.x, swatch.y, swatch.width, swatch.height, 4.0);
            if let Some(id) = &view.click_id {
                result.content_items.push((id.clone(), swatch));
            }
        }
        TableCell::Check(checked) | TableCell::EditableCheck(checked) => {
            let box_rect = WidgetRect::new(rect.x, rect.y + (rect.height - 16.0) / 2.0, 16.0, 16.0);
            ctx.set_fill_color(if *checked { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
            ctx.fill_rounded_rect(box_rect.x, box_rect.y, box_rect.width, box_rect.height, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(box_rect.x, box_rect.y, box_rect.width, box_rect.height, 3.0);
            if *checked {
                ctx.set_stroke_color(&toolbar_theme.item_text_active);
                ctx.set_stroke_width(2.0);
                ctx.begin_path();
                ctx.move_to(box_rect.x + 3.0, box_rect.y + box_rect.height * 0.55);
                ctx.line_to(box_rect.x + box_rect.width * 0.42, box_rect.bottom() - 3.0);
                ctx.line_to(box_rect.right() - 3.0, box_rect.y + 3.0);
                ctx.stroke();
            }
            if let Some(id) = &view.click_id {
                result.content_items.push((id.clone(), box_rect));
            }
        }
    }
}

/// A plain, clipped text draw shared by [`TableCell::Text`]/`Toned` — same
/// clip discipline [`draw_label`] uses, so a string wider than its own
/// column is cut off rather than spilling into the next one.
fn draw_table_text(ctx: &mut dyn RenderContext, rect: WidgetRect, text: &str, color: &str) {
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.clip();
    ctx.fill_text(text, rect.x, rect.center_y());
    ctx.restore();
}

/// Render any [`PropertyValue`] as plain text for a `readonly` row —
/// generic over every variant, not just the `String` shape a scalar
/// readonly property (a hypothetical future one — every readonly declarer
/// in this crate today is `PropertyType::Stat`/`PropertyType::Table`, drawn
/// by [`draw_stat_block_row`]/[`draw_table_row`] instead) would use, so one
/// still shows something instead of an empty row.
fn readonly_value_text(value: &PropertyValue) -> String {
    match value {
        PropertyValue::String(s) | PropertyValue::Color(s) | PropertyValue::LineStyle(s) => s.clone(),
        PropertyValue::Number(n) => format!("{n}"),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::Coordinate { bar, price } => format!("{bar:.1} / {price:.2}"),
        // Neither `Stat` nor `Table` ever reaches here (drawn by their own
        // early `if let`s in `draw_property_row` before this function could
        // ever be called on one) — listed for exhaustiveness only, same
        // treatment as the two genuinely-unused types beside them.
        PropertyValue::Stat(_) | PropertyValue::Table(_) | PropertyValue::FibLevels(_) | PropertyValue::TimeframeVisibility(_) => {
            String::new()
        }
    }
}

/// Shared body for `Number`/`Integer` — a slider with an editable input,
/// step heuristically derived from the range (see [`draw_property_row`]'s
/// own `Number` arm doc: `PropertyType::Number.step` is always `None` on
/// every constructor call site in the codebase today).
#[allow(clippy::too_many_arguments)]
fn draw_number_slider(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    fid: &str,
    name: &str,
    value: f64,
    lo: f64,
    hi: f64,
) {
    let step = if (hi - lo) <= 20.0 { 0.1 } else { 1.0 };
    draw_slider_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, y, fid, name, value, lo, hi, step, true);
}
