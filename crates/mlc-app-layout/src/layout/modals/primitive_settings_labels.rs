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

use super::primitive_settings_property::{declared_properties, draw_property_row, properties_in, DeclaredProperty};
use super::primitive_settings_vid::{draw_label, draw_toggle_segment, ROW_H};

const GROUP_PAD_BOTTOM: f64 = 8.0;
const ALIGN_H: f64 = 72.0;

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
    let inner = labels_props.len() as f64 * ROW_H + if has_custom_text { 0.0 } else { TEXT_BLOCK_H };
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
        .filter(|(p, _)| matches(query, &[labels_header, p.name.as_str()]))
        .collect();
    let has_custom_text = prim.text_properties().is_some();
    let text_block_matches = !has_custom_text
        && matches(query, &[labels_header, ConfigKey::Content.get(lang), ConfigKey::TextStyleLabel.get(lang), ConfigKey::PositionLabel.get(lang)]);
    let inner = labels_props.len() as f64 * ROW_H + if text_block_matches { TEXT_BLOCK_H } else { 0.0 };
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
        .filter(|(p, _)| matches(&query, &[labels_header, p.name.as_str()]))
        .collect();
    let has_custom_text = prim.text_properties().is_some();
    let draw_text_block = !has_custom_text
        && matches(&query, &[labels_header, ConfigKey::Content.get(lang), ConfigKey::TextStyleLabel.get(lang), ConfigKey::PositionLabel.get(lang)]);

    if !labels_props.is_empty() || draw_text_block {
        for (prop, kind) in &labels_props {
            draw_property_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, *kind, prop);
            *row_y += ROW_H;
        }

        if draw_text_block {
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
