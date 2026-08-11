//! Primitive settings modal — Labels tab: content / font / style / position
//! for the primitive's own text/label surface.
//!
//! Split out of the Appearance tab (`primitive_settings_vid`) in the 4-tab
//! cutover (2026-08-08) — Labels used to be one GROUP inside that tab
//! (`PropertyCategory::AppearanceLabels`, now `LabelsText`), sharing it with
//! the LINE and FILL groups. It is a tab of its own now, present or absent
//! per [`has_content`] exactly like every other tab
//! (`crate::layout::modals::primitive_settings::available_tabs` calls it) —
//! a primitive that declares nothing into `PropertyCategory::LabelsText`
//! AND overrides `Primitive::text_properties()` to `Some(vec)` (its own doc
//! on the trait: that return means "use custom text properties, hide the
//! position grid") shows no Labels tab at all.
//!
//! Generic since the 2026-08-08 declared-properties pass, same contract as
//! `primitive_settings_vid`'s own LINE/FILL groups: every row besides the
//! one bespoke block below comes from `primitive_settings_property::
//! {declared_properties, properties_in}`, bucketed by
//! `PropertyCategory::LabelsText` and sorted by its own `order`.
//!
//! One section stays bespoke — the content/bold-italic/position-grid block,
//! driven by `PrimitiveData.text` directly. Drawn only when
//! `Primitive::text_properties()` returns `None`. Ported unchanged from
//! `primitive_settings_vid::draw_text_block_rows` (pre-split) — see that
//! module's former doc (2026-08-08 declared-properties pass) for the full
//! history of this block.

use crate::layout::render_frame::PrimitiveSettingsResult;
use crate::ui::modal_settings::PrimitiveSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType, WidgetState, WidgetTheme};
use mlc_core::drawing::primitives_v2::config::PropertyCategory;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::TextAlign as PrimTextAlign;
use mlc_core::i18n::{current_language, ConfigKey};
use mlc_core::render::RenderContext;
use uzor::types::Rect as WidgetRect;

use super::primitive_settings_property::{
    declared_properties, draw_property_row, effective_header, group_runs, properties_in, DeclaredProperty,
};
use super::primitive_settings_vid::{draw_group_header, draw_label, draw_toggle_segment, ROW_H};

const GROUP_PAD_BOTTOM: f64 = 8.0;
const ALIGN_H: f64 = 72.0;
/// Mirrors `primitive_settings_vid::GROUP_HEADER_H` — this tab has no
/// header of its own for the DEFAULT run (unchanged, see [`has_named_group`]
/// and this module's own doc), only a named [`mlc_core::drawing::primitives_v2::config::PropertyGroup`]
/// run draws one, at the same height every other tab's own group header
/// uses.
const GROUP_HEADER_H: f64 = 22.0;
/// Mirrors `primitive_settings_vid::GROUP_GAP` — spacing between two
/// consecutive runs on this tab, used only once a primitive actually
/// declares more than one (a primitive with zero named groups, today's
/// status quo for most types, draws byte-identical to before this constant
/// existed: see [`has_named_group`]).
const GROUP_GAP: f64 = 9.0;

/// Whether `props` uses the W2 group model at all — `false` for every
/// primitive that has not migrated any `LabelsText` property into a named
/// [`mlc_core::drawing::primitives_v2::config::PropertyGroup`] yet, which is
/// what keeps this tab's height/spacing byte-identical to before named
/// groups existed for those types (no header, no extra gap before the
/// bespoke text block).
fn has_named_group(props: &[&DeclaredProperty]) -> bool {
    props.iter().any(|(p, _)| p.group.is_some())
}

/// Height for this tab's own declared-property runs (W2 group model): the
/// default (`group: None`) run keeps this tab's own pre-existing
/// convention — no header — every named `PropertyGroup` gets its own
/// `GROUP_HEADER_H` run, `GROUP_GAP`-separated from its neighbours. Mirrors
/// `draw`'s own shape.
fn labels_group_height(props: &[&DeclaredProperty]) -> f64 {
    let mut h = 0.0;
    let mut any = false;
    for (group, rows) in group_runs(props) {
        if rows.is_empty() {
            continue;
        }
        if any {
            h += GROUP_GAP;
        }
        let header_h = if group.is_none() { 0.0 } else { GROUP_HEADER_H };
        h += header_h + rows.len() as f64 * ROW_H;
        any = true;
    }
    h
}

/// Height of the bespoke text block (content + bold/italic + position
/// grid), drawn only when `text_properties()` is `None` — see this
/// module's own doc.
const TEXT_BLOCK_H: f64 = 2.0 * ROW_H + ALIGN_H;

/// Whether this primitive has anything at all for the Labels tab: any
/// declared `PropertyCategory::LabelsText` property, or the bespoke text
/// block (shown whenever `text_properties()` is `None`). The single
/// predicate `crate::layout::modals::primitive_settings::available_tabs`
/// calls to decide whether the Labels tab exists for a given primitive —
/// kept next to `content_height`/`draw` so the three can never drift apart.
pub fn has_content(prim: &dyn Primitive) -> bool {
    !properties_in(&declared_properties(prim), PropertyCategory::LabelsText).is_empty()
        || prim.text_properties().is_none()
}

/// Static — does not shrink when the search field filters rows out, same
/// tradeoff every other tab body's own `content_height` makes (a filtered
/// row just stops advancing `row_y` during the draw pass instead).
pub fn content_height(prim: &dyn Primitive) -> f64 {
    let all = declared_properties(prim);
    let labels_props = properties_in(&all, PropertyCategory::LabelsText);
    let has_custom_text = prim.text_properties().is_some();
    let props_h = labels_group_height(&labels_props);
    let text_block_h = if has_custom_text { 0.0 } else { TEXT_BLOCK_H };
    // The extra gap before the bespoke text block only exists once this
    // primitive actually uses a named group — see `has_named_group`'s own
    // doc: zero named groups reproduces the pre-W2 shape exactly (rows then
    // block, no gap).
    let gap = if props_h > 0.0 && text_block_h > 0.0 && has_named_group(&labels_props) { GROUP_GAP } else { 0.0 };
    let inner = props_h + gap + text_block_h;
    if inner > 0.0 { inner + GROUP_PAD_BOTTOM } else { 0.0 }
}

/// Query-aware body height for the cross-tab settings-search result view
/// (item 8, 2026-08-08 follow-up). Unlike `content_height` (static, sized
/// for everything this tab could ever show), this shrinks to exactly what
/// `draw` will paint for a live `query`.
pub fn content_height_for_query(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    let query = super::primitive_settings::search_query(state);
    query_height(prim, &query)
}

fn query_height(prim: &dyn Primitive, query: &str) -> f64 {
    if query.is_empty() {
        return content_height(prim);
    }
    let lang = current_language();
    let matches = super::primitive_settings::search_matches;
    let all = declared_properties(prim);

    let labels_header = ConfigKey::Labels.get(lang);
    let labels_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::LabelsText)
        .into_iter()
        .filter(|(p, _)| matches(query, &[effective_header(p, labels_header), p.name.as_str()]))
        .collect();
    let has_custom_text = prim.text_properties().is_some();
    let text_block_matches = !has_custom_text
        && matches(query, &[labels_header, ConfigKey::Content.get(lang), ConfigKey::TextStyleLabel.get(lang), ConfigKey::PositionLabel.get(lang)]);
    let props_h = labels_group_height(&labels_props);
    let text_block_h = if text_block_matches { TEXT_BLOCK_H } else { 0.0 };
    let gap = if props_h > 0.0 && text_block_h > 0.0 && has_named_group(&labels_props) { GROUP_GAP } else { 0.0 };
    let inner = props_h + gap + text_block_h;
    if inner > 0.0 { inner + GROUP_PAD_BOTTOM } else { 0.0 }
}

/// Draws the Labels tab's own rows. Returns `true` if at least one
/// interactive row was drawn — mirrors every other tab body's own
/// convention (`primitive_settings_vid::draw`, `_placement::draw`); the
/// caller (the cross-tab search combiner in `primitive_settings.rs`) uses
/// it to decide whether to show this tab's own header.
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
    let lang = current_language();
    let query = super::primitive_settings::search_query(state);
    let matches = super::primitive_settings::search_matches;
    let items_before = result.content_items.len();

    let all = declared_properties(prim);
    let labels_header = ConfigKey::Labels.get(lang);
    let labels_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::LabelsText)
        .into_iter()
        .filter(|(p, _)| matches(&query, &[effective_header(p, labels_header), p.name.as_str()]))
        .collect();
    let has_custom_text = prim.text_properties().is_some();
    let draw_text_block = !has_custom_text
        && matches(&query, &[labels_header, ConfigKey::Content.get(lang), ConfigKey::TextStyleLabel.get(lang), ConfigKey::PositionLabel.get(lang)]);
    let named_group_in_use = has_named_group(&labels_props);

    if !labels_props.is_empty() || draw_text_block {
        let mut any_drawn = false;
        for (group, rows) in group_runs(&labels_props) {
            if rows.is_empty() {
                continue;
            }
            if any_drawn {
                *row_y += GROUP_GAP;
            }
            // The default run keeps this tab's own pre-existing
            // convention — no header at all; only a named group draws one.
            if let Some(g) = group {
                draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, g.label(), None);
                *row_y += GROUP_HEADER_H;
            }
            for (prop, kind) in &rows {
                draw_property_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, *kind, prop);
                *row_y += ROW_H;
            }
            any_drawn = true;
        }

        if draw_text_block {
            if any_drawn && named_group_in_use {
                *row_y += GROUP_GAP;
            }
            draw_text_block_rows(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, row_y, prim);
        }

        *row_y += GROUP_PAD_BOTTOM;
    }

    result.content_items.len() > items_before
}

// =============================================================================
// Bespoke: content / bold-italic / position grid — driven by
// `PrimitiveData.text` directly, drawn only when `text_properties()` is
// `None` (see this module's own doc).
// =============================================================================

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn draw_text_block_rows(
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
    let text_block = prim.data().text.clone();

    draw_label(ctx, toolbar_theme, x0, label_col, *row_y, ConfigKey::Content.get(lang));
    let field_id = "text_content";
    let is_editing = state.active_field_id.as_deref() == Some(field_id);
    let current = text_block.as_ref().map(|t| t.content.clone()).unwrap_or_default();
    let ar = &state.active_render;
    let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
        (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
    } else {
        (current.as_str(), current.len(), None, None)
    };
    let control_w = (x1 - (x0 + label_col)).max(100.0);
    let rect = WidgetRect::new(x0 + label_col, *row_y + 2.0, control_w, ROW_H - 4.0);
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

    draw_label(ctx, toolbar_theme, x0, label_col, *row_y, ConfigKey::TextStyleLabel.get(lang));
    let bold = text_block.as_ref().map(|t| t.bold).unwrap_or(false);
    let italic = text_block.as_ref().map(|t| t.italic).unwrap_or(false);
    let seg_w = 30.0;
    let bold_rect = WidgetRect::new(x0 + label_col, *row_y + 2.0, seg_w, ROW_H - 4.0);
    let italic_rect = WidgetRect::new(bold_rect.right() + 4.0, *row_y + 2.0, seg_w, ROW_H - 4.0);
    draw_toggle_segment(ctx, toolbar_theme, bold_rect, ConfigKey::BoldAbbrev.get(lang), bold);
    draw_toggle_segment(ctx, toolbar_theme, italic_rect, ConfigKey::ItalicAbbrev.get(lang), italic);
    result.content_items.push(("text_bold".to_string(), bold_rect));
    result.content_items.push(("text_italic".to_string(), italic_rect));
    *row_y += ROW_H;

    draw_label(ctx, toolbar_theme, x0, label_col, *row_y, ConfigKey::PositionLabel.get(lang));
    let v_align = text_block.as_ref().map(|t| t.v_align).unwrap_or(PrimTextAlign::Center);
    let h_align = text_block.as_ref().map(|t| t.h_align).unwrap_or(PrimTextAlign::Center);
    draw_align_grid(ctx, toolbar_theme, result, x0 + label_col, *row_y, v_align, h_align);
    *row_y += ALIGN_H;
}

fn align_id(a: PrimTextAlign) -> &'static str {
    match a {
        PrimTextAlign::Start => "start",
        PrimTextAlign::Center => "center",
        PrimTextAlign::End => "end",
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
