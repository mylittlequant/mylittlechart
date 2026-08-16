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
//! the row's own options do not fit — see the `Pills` arm), single-line
//! input — via the helpers already used for the base `PrimitiveData` fields
//! (`primitive_settings_vid::{draw_label, draw_bool_row, draw_slider_row,
//! draw_split_value_control, draw_toggle_segment, line_style_display_name}`).
//! A category nobody declared into collects zero properties, so the caller's
//! `if !rows.is_empty()` naturally draws nothing for it — no header, no gap,
//! no reserved height.
//!
//! What this module deliberately does NOT decide: which categories a given
//! tab looks for (each tab body's own small fixed list — see their own
//! `content_height`/`draw`), or anything that is not `ConfigProperty`-shaped
//! at all (the level ladder, palette cards, points table, timeframe chips,
//! the universal base-stroke row, the raw-`TextBlock` content/style/position
//! block) — those stay bespoke in the tab that owns them, documented there.

use mlc_core::drawing::primitives_v2::config::{ConfigProperty, PropertyCategory, PropertyGroup, PropertyType, PropertyValue};
use mlc_core::drawing::primitives_v2::{Primitive, PropertyKind};
use mlc_core::drawing::LineStyle;
use mlc_core::i18n::current_language;
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use crate::ui::modal_settings::PrimitiveSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType, WidgetTheme};
use crate::layout::render_frame::PrimitiveSettingsResult;

use super::primitive_settings_intent::{pills_fits, pills_segments, PILLS_SEGMENT_GAP};
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
/// the same `ROW_H` every other row in these tabs uses, since every
/// `PropertyType` this module knows how to draw renders as one row.
///
/// A `readonly` property (`ConfigProperty::readonly`, e.g. Position's
/// computed Risk/Target/R:R — `PropertyCategory::AppearanceStats`) short-
/// circuits every `PropertyType` arm below: it always renders as a plain
/// caption + value row, muted text, no control, nothing pushed into
/// `result.content_items` — there is nothing here to click or drag.
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
        PropertyType::FibLevels | PropertyType::Coordinate | PropertyType::TimeframeVisibility => {}
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

/// Render any [`PropertyValue`] as plain text for a `readonly` row —
/// generic over every variant, not just the `String` shape
/// `ConfigProperty::text`-based readonly properties (Position's Risk/
/// Target/R:R, today's only declarers) actually use, so a future readonly
/// `Number`/`Boolean`/`Color` property still shows something instead of an
/// empty row.
fn readonly_value_text(value: &PropertyValue) -> String {
    match value {
        PropertyValue::String(s) | PropertyValue::Color(s) | PropertyValue::LineStyle(s) => s.clone(),
        PropertyValue::Number(n) => format!("{n}"),
        PropertyValue::Integer(i) => i.to_string(),
        PropertyValue::Boolean(b) => b.to_string(),
        PropertyValue::Coordinate { bar, price } => format!("{bar:.1} / {price:.2}"),
        PropertyValue::FibLevels(_) | PropertyValue::TimeframeVisibility(_) => String::new(),
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
