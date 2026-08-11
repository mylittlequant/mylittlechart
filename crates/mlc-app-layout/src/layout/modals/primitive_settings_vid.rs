//! Primitive settings modal — Appearance tab: line / fill.
//!
//! Present only when there is something to show
//! (`crate::layout::modals::primitive_settings::available_tabs`): a
//! primitive with no level ladder always has the bespoke base-stroke row
//! (below), so it always has an Appearance tab; a level-having primitive
//! only gets one if it also declares an `AppearanceLine`/`AppearanceFill`
//! property (the base stroke moves to the Levels tab's own top section for
//! those — see `primitive_settings_levels::draw_top_section`'s own doc).
//! The LABELS group that used to live in this tab (`AppearanceLabels`) is
//! its own tab now — `primitive_settings_labels` — split out in the 4-tab
//! cutover (2026-08-08); this module keeps the shared row-drawing widgets
//! (`draw_label`/`draw_bool_row`/`draw_slider_row`/`draw_split_value_control`/
//! `draw_toggle_segment`/`line_style_display_name`/`ROW_H`/
//! [`compute_label_col`]) that module and `primitive_settings_levels`/
//! `primitive_settings_placement`/`primitive_settings_property` import,
//! since it already owned them before the split.
//!
//! Generic since the 2026-08-08 declared-properties pass: this module no
//! longer names any property id or draws a group whose membership it
//! decided for itself. Every row in the LINE/FILL groups (besides the one
//! bespoke section documented below) comes from
//! `primitive_settings_property::{declared_properties, properties_in}` —
//! the primitive's own `style_properties()`, bucketed by its own declared
//! `PropertyCategory::AppearanceLine/Fill` and sorted by its own `order`,
//! rendered generically from each one's `PropertyType`. A group nobody
//! declared into draws nothing — no header, no gap, zero height.
//!
//! One section stays bespoke — a real `PrimitiveData` field, not a
//! `ConfigProperty`-shaped declaration, so there is nothing to bucket: the
//! base stroke colour/width/line-style row, universal to every primitive
//! via `PrimitiveData` (not per-type declared). It heads the LINE group
//! whenever the primitive has no level ladder; a level-having primitive
//! owns the SAME three fields on the Levels tab's own top section instead
//! (`primitive_settings_levels::draw_top_section`) — see that module's own
//! doc for why (2026-08-08 appearance-surface consolidation). This is a
//! structural fact about every primitive (has a ladder, or does not), not a
//! per-type id cherry-pick.
//!
//! "Style" (and every generic `PropertyType::LineStyle`/`Select` row —
//! `primitive_settings_property::draw_property_row` reuses the same
//! control) is the split cycle+chevron button restored in item 1,
//! 2026-08-09: the left region shows the current value as text and cycles
//! to the next option on click (unchanged id, unchanged click dispatch),
//! the right region is the chevron and opens a dropdown listing every
//! option — see [`draw_split_value_control`]'s own doc. The settings search
//! (`crate::layout::modals::primitive_settings::search_query`) filters in
//! real time against each row's OWN localized label but is not persisted
//! once you click away.
//!
//! Dropped by the move to generic per-property rendering (named, not
//! silently lost — see the 2026-08-08 declared-properties report): the
//! Extend-Left/Right two-up segmented-button pairing for DECLARED boolean
//! properties specifically (now ordinary checkbox rows).

use crate::layout::render_frame::{PrimitiveSettingsResult, ValueDropdownAnchor, ValueDropdownKind};
use crate::ui::modal_settings::PrimitiveSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{WidgetTheme, SETTINGS_BUTTON_RADIUS};
use mlc_core::drawing::primitives_v2::config::{PropertyCategory, PropertyGroup, PropertyType};
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::LineStyle;
use mlc_core::i18n::{current_language, ConfigKey, SettingsKey, StyleKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use super::primitive_settings_property::{
    declared_properties, draw_property_row, effective_header, group_runs, properties_in, DeclaredProperty,
};

pub const ROW_H: f64 = 26.0;
const GROUP_HEADER_H: f64 = 22.0;
const GROUP_PAD_BOTTOM: f64 = 8.0;
const GROUP_GAP: f64 = 9.0;

/// Breathing room `draw_label` leaves between the end of its own clipped
/// text and the live `label_col` a modal render computed
/// ([`compute_label_col`]) — the "gap" half of "label column and
/// label→control gap". Previously `draw_label` did not clip at all, so an
/// overlong label simply painted straight through the point its own column
/// was supposed to stop at.
const LABEL_CONTROL_GAP: f64 = 8.0;

/// Computes the ONE label-column width every row in this modal — checkbox,
/// colour swatch, cycle button, text input, and (via
/// [`crate::ui::widgets::SliderConfig::label_column`]) the slider track —
/// starts flush against, for THIS primitive's render. Replaces the old flat
/// `LABEL_COLUMN_W` constant (deleted, 2026-08-09 shared-label-column fix):
/// a fixed number truncated a caption as long as "Label Background Color"
/// while every slider row measured its own, different label and started its
/// own track wherever that happened to end — the same modal showing two
/// different left edges depending on which kind of control a row was.
///
/// Measured from the widest caption that can appear in THIS primitive's
/// body: every declared `ConfigProperty.name`
/// (`primitive_settings_property::declared_properties`) plus the bespoke row
/// captions the tab bodies draw themselves OUTSIDE the generic property
/// dispatch — the base-stroke colour/width/style row (`draw_base_stroke_rows`
/// below, and `primitive_settings_levels`'s own Levels/Base fallback, byte-
/// for-byte the same three captions) and the text-content block
/// (`primitive_settings_labels::draw_text_block_rows`). A caption that
/// renders through [`draw_slider_row`] is measured with the trailing `":"`
/// that function appends to its own label, so the column is never one glyph
/// short for exactly those rows. Clamped to `90.0..=220.0`: neither an
/// unusually short caption set (nothing declared, base row only) nor an
/// unusually long one can push a checkbox off the edge of a narrow modal or
/// eat most of a wide one.
pub fn compute_label_col(ctx: &mut dyn RenderContext, prim: &dyn Primitive) -> f64 {
    let lang = current_language();
    ctx.set_font("12px sans-serif");

    let mut captions: Vec<String> = declared_properties(prim)
        .into_iter()
        .map(|(p, _)| {
            if matches!(p.prop_type, PropertyType::Number { .. } | PropertyType::Integer { .. }) {
                format!("{}:", p.name)
            } else {
                p.name
            }
        })
        .collect();

    // Bespoke base-stroke row (`draw_base_stroke_rows` below, and
    // `primitive_settings_levels`'s own Levels/Base fallback).
    captions.push(SettingsKey::Color.get(lang).to_string());
    captions.push(format!("{}:", ConfigKey::WidthLabel.get(lang)));
    captions.push(ConfigKey::Style.get(lang).to_string());

    // Bespoke text-content block (`primitive_settings_labels::draw_text_block_rows`).
    captions.push(ConfigKey::Content.get(lang).to_string());
    captions.push(ConfigKey::TextStyleLabel.get(lang).to_string());
    captions.push(ConfigKey::PositionLabel.get(lang).to_string());

    let max_w = captions.iter().map(|c| ctx.measure_text(c)).fold(0.0_f64, f64::max);
    (max_w + LABEL_CONTROL_GAP).clamp(90.0, 220.0)
}

/// Height of the bespoke base-stroke row (colour + width + line style) —
/// see this module's own doc for why it is not `ConfigProperty`-driven.
const BASE_STROKE_H: f64 = 3.0 * ROW_H;

/// True height the LINE/FILL groups need, given `prim`'s own declared
/// properties. Shared by `content_height` (query-independent) and
/// `content_height_for_query` (via `group_heights_for_query`, below) so the
/// two can never drift apart.
struct GroupHeights {
    line: f64,
    fill: f64,
    /// Position's trailing read-only Risk/Target/R:R group (2026-08-09) —
    /// `PropertyCategory::AppearanceStats`. Zero for every other primitive
    /// today (nothing else declares into that category).
    stats: f64,
}

impl GroupHeights {
    fn total(&self) -> f64 {
        let mut h = 0.0;
        let mut any = false;
        for g in [self.line, self.fill, self.stats] {
            if g > 0.0 {
                if any {
                    h += GROUP_GAP;
                }
                h += g;
                any = true;
            }
        }
        h
    }
}

/// Height for the LINE group's own already-filtered/`order`-sorted rows,
/// honouring a declared `PropertyGroup` sub-split (W2, `mlc_core`'s
/// `PropertyGroup` doc): the default (`group: None`) run keeps the bespoke
/// base-stroke row (`draw_base_stroke`, present whenever `!has_levels`) and
/// this category's own fixed header (suppressed for a level-having
/// primitive, exactly as before this split existed); every other declared
/// `PropertyGroup` gets its own `GROUP_HEADER_H` run, unconditionally
/// headered, `GROUP_GAP`-separated from its neighbours. Shared by
/// `group_heights`/`query_heights` (via their own filtered `line_props`)
/// and mirrored row-for-row by `draw`'s own LINE section.
fn line_group_height(line_props: &[&DeclaredProperty], has_levels: bool, draw_base_stroke: bool) -> f64 {
    let mut h = 0.0;
    let mut any = false;
    for (group, rows) in group_runs(line_props) {
        let is_default = group.is_none();
        let base_h = if is_default && !has_levels && draw_base_stroke { BASE_STROKE_H } else { 0.0 };
        let inner = base_h + rows.len() as f64 * ROW_H;
        if inner <= 0.0 {
            continue;
        }
        let header_h = if is_default { if has_levels { 0.0 } else { GROUP_HEADER_H } } else { GROUP_HEADER_H };
        if any {
            h += GROUP_GAP;
        }
        h += header_h + inner + GROUP_PAD_BOTTOM;
        any = true;
    }
    h
}

/// Height for a category whose own default run is UNCONDITIONALLY headered
/// when non-empty and carries no bespoke row (`AppearanceFill`,
/// `AppearanceStats`) — every [`PropertyGroup`] run (including the default
/// one) shares the exact same shape, `GROUP_GAP`-separated. Shared by
/// `group_heights`/`query_heights` and mirrored by `draw`'s own FILL/RESULTS
/// sections.
fn headered_group_height(props: &[&DeclaredProperty]) -> f64 {
    let mut h = 0.0;
    let mut any = false;
    for (_, rows) in group_runs(props) {
        if rows.is_empty() {
            continue;
        }
        if any {
            h += GROUP_GAP;
        }
        h += GROUP_HEADER_H + rows.len() as f64 * ROW_H + GROUP_PAD_BOTTOM;
        any = true;
    }
    h
}

fn group_heights(prim: &dyn Primitive) -> GroupHeights {
    let has_levels = prim.level_configs().is_some();
    let all = declared_properties(prim);

    let line_props = properties_in(&all, PropertyCategory::AppearanceLine);
    let line = line_group_height(&line_props, has_levels, true);

    let fill_props = properties_in(&all, PropertyCategory::AppearanceFill);
    let fill = headered_group_height(&fill_props);

    let stats_props = properties_in(&all, PropertyCategory::AppearanceStats);
    let stats = headered_group_height(&stats_props);

    GroupHeights { line, fill, stats }
}

/// Whether this primitive has anything at all for the Appearance tab: the
/// bespoke base-stroke row (present whenever it has no level ladder — a
/// level-having primitive owns the same three fields on the Levels tab
/// instead), or a declared `AppearanceLine`/`AppearanceFill` property. The
/// single predicate `crate::layout::modals::primitive_settings::
/// available_tabs` calls to decide whether this tab exists for a given
/// primitive — kept next to `content_height`/`draw` so the three can never
/// drift apart.
pub fn has_content(prim: &dyn Primitive) -> bool {
    group_heights(prim).total() > 0.0
}

/// Static — does not shrink when the search field filters rows out (a
/// filtered row just stops advancing `row_y` during the draw pass instead;
/// the scroll range may read slightly larger than what's visible while a
/// query is active).
pub fn content_height(prim: &dyn Primitive) -> f64 {
    group_heights(prim).total()
}

/// Query-aware body height for the cross-tab settings-search result view
/// (item 8, 2026-08-08 follow-up). Unlike `content_height` (static, sized
/// for everything this tab could ever show), this shrinks to exactly what
/// `draw` will paint for a live `query`.
pub fn content_height_for_query(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    let query = super::primitive_settings::search_query(state);
    query_heights(prim, &query).total()
}

fn query_heights(prim: &dyn Primitive, query: &str) -> GroupHeights {
    if query.is_empty() {
        return group_heights(prim);
    }
    let lang = current_language();
    let matches = super::primitive_settings::search_matches;
    let has_levels = prim.level_configs().is_some();
    let all = declared_properties(prim);

    let group_line = ConfigKey::GroupLine.get(lang);
    let base_stroke_matches = matches(query, &[group_line, SettingsKey::Color.get(lang), ConfigKey::WidthLabel.get(lang), ConfigKey::Style.get(lang)]);
    let line_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::AppearanceLine)
        .into_iter()
        .filter(|(p, _)| matches(query, &[effective_header(p, group_line), p.name.as_str()]))
        .collect();
    let line = line_group_height(&line_props, has_levels, base_stroke_matches);

    let group_fill = ConfigKey::Fill.get(lang);
    let fill_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::AppearanceFill)
        .into_iter()
        .filter(|(p, _)| matches(query, &[effective_header(p, group_fill), p.name.as_str()]))
        .collect();
    let fill = headered_group_height(&fill_props);

    let group_results = ConfigKey::GroupResults.get(lang);
    let stats_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::AppearanceStats)
        .into_iter()
        .filter(|(p, _)| matches(query, &[effective_header(p, group_results), p.name.as_str()]))
        .collect();
    let stats = headered_group_height(&stats_props);

    GroupHeights { line, fill, stats }
}

/// Draws the Appearance tab's own rows. Returns `true` if at least one interactive
/// row was drawn — the caller (the cross-tab search combiner in
/// `primitive_settings.rs`) uses this to decide whether to show this tab's
/// own header; group headers alone never register a `content_items` entry,
/// so this can be read straight off whether that list grew.
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

    let has_levels = prim.level_configs().is_some();
    let all = declared_properties(prim);
    let mut any_drawn = false;

    // ---- Line ----
    // Headerless for a level-capable primitive's own DEFAULT run (2026-08-08
    // appearance-surface consolidation): the base stroke row lives on the
    // Levels tab instead, so the only rows that can land in the default run
    // here are declared AppearanceLine properties (extend/trend line/…) —
    // the "LINE" caption above them would now be captioning unrelated
    // toggles with nothing to introduce, so it is skipped. A NAMED
    // `PropertyGroup` run (W2) is never skipped this way — see
    // `line_group_height`'s own doc.
    let group_line = ConfigKey::GroupLine.get(lang);
    let draw_base_stroke = !has_levels
        && matches(&query, &[group_line, SettingsKey::Color.get(lang), ConfigKey::WidthLabel.get(lang), ConfigKey::Style.get(lang)]);
    let line_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::AppearanceLine)
        .into_iter()
        .filter(|(p, _)| matches(&query, &[effective_header(p, group_line), p.name.as_str()]))
        .collect();
    for (group, rows) in group_runs(&line_props) {
        draw_line_run(
            ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, row_y, &mut any_drawn,
            prim, group, &rows, has_levels, draw_base_stroke, group_line,
        );
    }

    // ---- Fill ----
    let group_fill = ConfigKey::Fill.get(lang);
    let fill_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::AppearanceFill)
        .into_iter()
        .filter(|(p, _)| matches(&query, &[effective_header(p, group_fill), p.name.as_str()]))
        .collect();
    for (group, rows) in group_runs(&fill_props) {
        draw_headered_run(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, row_y, &mut any_drawn, group, &rows, group_fill);
    }

    // ---- Results (read-only computed stats, e.g. Position's own
    // Risk/Target/R:R — `PropertyCategory::AppearanceStats`) — always the
    // LAST group(s), below the editable Line/Fill rows above. ----
    let group_results = ConfigKey::GroupResults.get(lang);
    let stats_props: Vec<&DeclaredProperty> = properties_in(&all, PropertyCategory::AppearanceStats)
        .into_iter()
        .filter(|(p, _)| matches(&query, &[effective_header(p, group_results), p.name.as_str()]))
        .collect();
    for (group, rows) in group_runs(&stats_props) {
        draw_headered_run(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, row_y, &mut any_drawn, group, &rows, group_results);
    }

    result.content_items.len() > items_before
}

/// Draws one LINE-category run — the default (`group: None`) run keeps the
/// bespoke base-stroke row and this category's own conditionally-suppressed
/// header; every named [`PropertyGroup`] run is unconditionally headered.
/// Mirrors [`line_group_height`]'s own shape row for row. Does nothing (not
/// even the `GROUP_GAP`) when this run has nothing to draw.
#[allow(clippy::too_many_arguments)]
fn draw_line_run(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    row_y: &mut f64,
    any_drawn: &mut bool,
    prim: &dyn Primitive,
    group: Option<PropertyGroup>,
    rows: &[&DeclaredProperty],
    has_levels: bool,
    draw_base_stroke: bool,
    group_line: &str,
) {
    let is_default = group.is_none();
    let will_draw_base = is_default && !has_levels && draw_base_stroke;
    if !will_draw_base && rows.is_empty() {
        return;
    }
    if *any_drawn {
        *row_y += GROUP_GAP;
    }
    // `will_draw_base || !rows.is_empty()` is guaranteed by the early return
    // above — this run always paints at least one row from here on, so
    // unlike a category's own top-level `if` (before this split existed)
    // there is no "reserved a header, drew nothing" case left to unwind.
    let show_header = !is_default || !has_levels;
    if show_header {
        let title = group.map(|g| g.label()).unwrap_or(group_line);
        draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, title, None);
        *row_y += GROUP_HEADER_H;
    }
    if will_draw_base {
        draw_base_stroke_rows(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, row_y, prim);
    }
    for (prop, kind) in rows {
        draw_property_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, *kind, prop);
        *row_y += ROW_H;
    }
    *row_y += GROUP_PAD_BOTTOM;
    *any_drawn = true;
}

/// Draws one always-headered run — shared by the FILL and RESULTS sections
/// (no bespoke row, unlike LINE). Mirrors [`headered_group_height`]'s own
/// shape. Does nothing when `rows` is empty.
#[allow(clippy::too_many_arguments)]
fn draw_headered_run(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    row_y: &mut f64,
    any_drawn: &mut bool,
    group: Option<PropertyGroup>,
    rows: &[&DeclaredProperty],
    default_title: &str,
) {
    if rows.is_empty() {
        return;
    }
    if *any_drawn {
        *row_y += GROUP_GAP;
    }
    let title = group.map(|g| g.label()).unwrap_or(default_title);
    draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, title, None);
    *row_y += GROUP_HEADER_H;
    for (prop, kind) in rows {
        draw_property_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, *kind, prop);
        *row_y += ROW_H;
    }
    *row_y += GROUP_PAD_BOTTOM;
    *any_drawn = true;
}

// =============================================================================
// Bespoke: base stroke colour/width/line style (universal `PrimitiveData`
// fields — see this module's own doc for why they are not declared
// properties).
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_base_stroke_rows(
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
    let stroke = prim.data().color.stroke.clone();
    let width = prim.data().width;
    let style = prim.data().style;

    draw_label(ctx, toolbar_theme, x0, label_col, *row_y, SettingsKey::Color.get(lang));
    let swatch_w = (x1 - (x0 + label_col)).clamp(40.0, 60.0);
    let rect = WidgetRect::new(x0 + label_col, *row_y + 3.0, swatch_w, ROW_H - 6.0);
    ctx.set_fill_color(&stroke);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    result.content_items.push(("stroke_color".to_string(), rect));
    *row_y += ROW_H;

    draw_slider_row(
        ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, "stroke_width", ConfigKey::WidthLabel.get(lang),
        width, 1.0, 10.0, 0.1, true,
    );
    *row_y += ROW_H;

    draw_label(ctx, toolbar_theme, x0, label_col, *row_y, ConfigKey::Style.get(lang));
    let control_w = (x1 - (x0 + label_col)).max(100.0);
    let rect = WidgetRect::new(x0 + label_col, *row_y + 2.0, control_w, ROW_H - 4.0);
    draw_split_value_control(
        ctx, toolbar_theme, state, result, rect, "line_style", line_style_display_name(style, lang),
        SplitValueKind::LineStyle(style),
    );
    *row_y += ROW_H;
}

// =============================================================================
// Small building blocks — shared with `primitive_settings_levels` (base
// stroke row reuses the same widgets), `primitive_settings_labels` (its own
// bespoke text-content row reuses `draw_label`/`draw_toggle_segment`) and
// `primitive_settings_property` (generic per-`PropertyType` dispatch reuses
// the same widgets too).
// =============================================================================

pub fn line_style_display_name(style: LineStyle, lang: mlc_core::i18n::Language) -> &'static str {
    match style {
        LineStyle::Solid => StyleKey::Solid.get(lang),
        LineStyle::Dashed => StyleKey::Dashed.get(lang),
        LineStyle::Dotted => StyleKey::Dotted.get(lang),
        LineStyle::LargeDashed => StyleKey::LargeDashed.get(lang),
        LineStyle::SparseDotted => StyleKey::SparseDotted.get(lang),
    }
}

pub fn draw_group_header(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    x0: f64,
    x1: f64,
    y: f64,
    title: &str,
    sub: Option<&str>,
) {
    // No contrasting bar (item 7, 2026-08-08 review): the header background
    // is the modal's own background colour, same as the Placement tab's
    // group headers — the two must agree.
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

// The row builders below (`draw_label`/`draw_bool_row`/`draw_slider_row`/
// `draw_value_dropdown`/`draw_split_value_control`/`draw_toggle_segment`,
// plus `ROW_H`/`SplitValueKind`/`line_style_display_name`) are `pub`, not
// `pub(crate)`, since 2026-08-10: `mlc-ui-lab` draws the APPROVED but
// not-yet-implemented archetype modals with them. A design prototype that
// hand-rolls its own checkbox is a prototype of a different product — the
// whole point of hosting the lab against this crate is that a row in the
// lab is the same pixels as a row in the app. Nothing about their
// behaviour changed; only who may call them.

/// Draws `text` left-aligned in the row's label column, clipped to
/// `label_col` minus [`LABEL_CONTROL_GAP`] so it can never run into the
/// control that starts at `x + label_col` — see [`compute_label_col`]'s own
/// doc for how that width is chosen (row-spacing fix, 2026-08-08; per-
/// primitive measured column, 2026-08-09).
pub fn draw_label(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x: f64, label_col: f64, y: f64, text: &str) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.save();
    ctx.begin_path();
    ctx.rect(x, y, (label_col - LABEL_CONTROL_GAP).max(0.0), ROW_H);
    ctx.clip();
    ctx.fill_text(text, x, y + ROW_H / 2.0);
    ctx.restore();
}

pub fn draw_bool_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    label_col: f64,
    y: f64,
    label: &str,
    checked: bool,
    field_id: &str,
) {
    draw_label(ctx, toolbar_theme, x0, label_col, y, label);
    let rect = WidgetRect::new(x0 + label_col, y + (ROW_H - 16.0) / 2.0, 16.0, 16.0);
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

pub fn draw_toggle_segment(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, rect: WidgetRect, label: &str, on: bool) {
    ctx.set_fill_color(if on { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_fill_color(if on { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted });
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    // Clip so a label wider than the segment (e.g. "Extend Left" at a
    // narrow modal width) is cut off rather than spilling outside the
    // button shape (item 1, 2026-08-08 review).
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    ctx.clip();
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    ctx.restore();
}

/// Every `LineStyle`/`Select` row's own control shape (item 1, 2026-08-09
/// restore): a single button split into two hit regions — passed to
/// [`draw_split_value_control`] so it knows which dropdown content to
/// anchor when this row's own menu is open.
pub enum SplitValueKind {
    /// A line-style row — carries the current style so the dropdown (if
    /// opened) can draw the fixed 5-variant catalogue as graphic previews.
    LineStyle(LineStyle),
    /// An ordinary `Select` row — its own `(value, label)` options plus
    /// which one is current.
    Select { options: Vec<(String, String)>, current: String },
}

/// The split cycle+chevron control every `PropertyType::LineStyle`/`Select`
/// row draws (item 1, 2026-08-09): the control this modal had before the
/// 2026-08-08 generic-property rewrite collapsed it into a single
/// cycle-on-click button with a decorative chevron
/// (`git log -p` on this file finds the pre-rewrite shape, registered as
/// `line_style`/`line_style_menu`).
///
/// The LEFT region shows the current value as text and cycles to the next
/// option on click — registered under `field_id` UNCHANGED, so every
/// existing click dispatch for it (`mlc-app`'s `input/settings.rs` line-
/// style/select-cycle handling) keeps working without modification. The
/// RIGHT region is the chevron and opens a dropdown listing every option —
/// registered under `{field_id}::menu`. Inside the button itself the value
/// always stays text regardless of `kind` — only the DROPDOWN's own rows
/// get the graphic-preview treatment for a line style (see
/// [`draw_value_dropdown`]).
///
/// This function does not paint the dropdown itself: it only records the
/// anchor (`PrimitiveSettingsResult::value_dropdown_anchor`) when
/// `state.open_value_dropdown` names THIS row's `field_id`, for the outer
/// pass (`primitive_settings::render_primitive_settings_modal`) to paint
/// and register OUTSIDE whichever tab's own scroll clip — the same
/// treatment `add_chip_dropdown_anchor`/`template_dropdown_menu` already
/// get. Painting it here would be cut by that clip (the ladder's, or
/// whichever tab's `ScrollableContainer` is currently open). At most one
/// row's anchor is ever set per frame: `open_value_dropdown` is a single
/// `Option<String>`, so opening a different row's menu always REPLACES the
/// previous value — the one-open-at-a-time guarantee lives there, not here.
#[allow(clippy::too_many_arguments)]
pub fn draw_split_value_control(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    field_id: &str,
    display_label: &str,
    kind: SplitValueKind,
) {
    const CHEVRON_W: f64 = 22.0;

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);

    let text_w = (rect.width - CHEVRON_W).max(0.0);
    let sep_x = rect.x + text_w;

    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.save();
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, text_w, rect.height);
    ctx.clip();
    ctx.fill_text(display_label, rect.x + 8.0, rect.center_y());
    ctx.restore();

    // Separator between the two hit regions.
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(sep_x, rect.y);
    ctx.line_to(sep_x, rect.bottom());
    ctx.stroke();

    // Chevron.
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("\u{25be}", sep_x + CHEVRON_W / 2.0, rect.center_y());

    let cycle_rect = WidgetRect::new(rect.x, rect.y, text_w, rect.height);
    let menu_rect = WidgetRect::new(sep_x, rect.y, CHEVRON_W, rect.height);
    result.content_items.push((field_id.to_string(), cycle_rect));
    result.content_items.push((format!("{field_id}::menu"), menu_rect));

    if state.open_value_dropdown.as_deref() == Some(field_id) {
        let (dd_kind, current_value) = match kind {
            SplitValueKind::LineStyle(style) => (ValueDropdownKind::LineStyle, style.as_str().to_string()),
            SplitValueKind::Select { options, current } => (ValueDropdownKind::Select(options), current),
        };
        result.value_dropdown_anchor = Some(ValueDropdownAnchor {
            button_rect: rect,
            field_id: field_id.to_string(),
            current_value,
            kind: dd_kind,
        });
    }
}

/// The fixed 5-variant catalogue every line-style dropdown lists, in the
/// order the owner specified: solid / dashed / dotted / large-dashed /
/// sparse-dotted.
const LINE_STYLE_CATALOGUE: [LineStyle; 5] =
    [LineStyle::Solid, LineStyle::Dashed, LineStyle::Dotted, LineStyle::LargeDashed, LineStyle::SparseDotted];

/// Paint + register the split control's dropdown from the anchor
/// `draw_split_value_control` set
/// (`PrimitiveSettingsResult::value_dropdown_anchor`). Called by the outer
/// pass (`primitive_settings::render_primitive_settings_modal`), after
/// `scrollable.end()` closes whichever tab's own scroll clip and after
/// `body_items_end` is captured — see that field's own doc for why. Hover
/// is not hand-drawn here: the generic hover-highlight loop further down in
/// the outer pass already covers every `content_items` entry this pushes,
/// the same as `draw_add_chip_dropdown`'s own rows.
pub fn draw_value_dropdown(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    anchor: &ValueDropdownAnchor,
) {
    const DD_ROW_H: f64 = 26.0;

    let menu_w = anchor.button_rect.width.max(120.0);
    let menu_x = anchor.button_rect.x;
    let menu_y = anchor.button_rect.bottom() + 2.0;
    let row_count = match &anchor.kind {
        ValueDropdownKind::LineStyle => LINE_STYLE_CATALOGUE.len(),
        ValueDropdownKind::Select(options) => options.len(),
    };
    let total_h = DD_ROW_H * row_count as f64 + 6.0;

    ctx.set_fill_color(&toolbar_theme.dropdown_bg);
    ctx.fill_rounded_rect(menu_x, menu_y, menu_w, total_h, SETTINGS_BUTTON_RADIUS);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(menu_x, menu_y, menu_w, total_h, SETTINGS_BUTTON_RADIUS);

    let mut row_y = menu_y + 3.0;
    match &anchor.kind {
        ValueDropdownKind::LineStyle => {
            for style in LINE_STYLE_CATALOGUE {
                let value = style.as_str();
                let row_rect = WidgetRect::new(menu_x, row_y, menu_w, DD_ROW_H);
                draw_dropdown_row_bg(ctx, toolbar_theme, row_rect, anchor.current_value == value);
                draw_line_style_preview(ctx, toolbar_theme, row_rect, style);
                result.content_items.push((format!("{}::opt:{}", anchor.field_id, value), row_rect));
                row_y += DD_ROW_H;
            }
        }
        ValueDropdownKind::Select(options) => {
            for (value, label) in options {
                let row_rect = WidgetRect::new(menu_x, row_y, menu_w, DD_ROW_H);
                let is_current = anchor.current_value == *value;
                draw_dropdown_row_bg(ctx, toolbar_theme, row_rect, is_current);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(if is_current { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, row_rect.x + 10.0, row_rect.center_y());
                result.content_items.push((format!("{}::opt:{}", anchor.field_id, value), row_rect));
                row_y += DD_ROW_H;
            }
        }
    }
}

fn draw_dropdown_row_bg(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, rect: WidgetRect, is_current: bool) {
    if is_current {
        ctx.set_fill_color(&toolbar_theme.item_bg_active);
        ctx.fill_rounded_rect(rect.x + 1.0, rect.y, rect.width - 2.0, rect.height, 3.0);
    }
}

/// One line-style dropdown row's own graphic preview — an actual
/// dash-patterned line sample instead of the style's name (item 1,
/// 2026-08-09: "not the style's name"). Dash/gap lengths are chosen to read
/// as visibly distinct at this row height, not lifted from
/// `LineStyle::dash_pattern` (that scales with stroke width, which this
/// preview has no notion of).
fn draw_line_style_preview(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, rect: WidgetRect, style: LineStyle) {
    let mid_y = rect.center_y();
    let sx0 = rect.x + 12.0;
    let sx1 = rect.right() - 12.0;
    ctx.set_stroke_color(&toolbar_theme.item_text);
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_stroke_width(1.5);
    match style {
        LineStyle::Solid => {
            ctx.begin_path();
            ctx.move_to(sx0, mid_y);
            ctx.line_to(sx1, mid_y);
            ctx.stroke();
        }
        LineStyle::Dashed => draw_dash_run(ctx, sx0, sx1, mid_y, 7.0, 4.0),
        LineStyle::LargeDashed => draw_dash_run(ctx, sx0, sx1, mid_y, 13.0, 5.0),
        LineStyle::Dotted => draw_dot_run(ctx, sx0, sx1, mid_y, 5.0),
        LineStyle::SparseDotted => draw_dot_run(ctx, sx0, sx1, mid_y, 9.0),
    }
}

fn draw_dash_run(ctx: &mut dyn RenderContext, x0: f64, x1: f64, y: f64, dash: f64, gap: f64) {
    let mut x = x0;
    while x < x1 {
        ctx.begin_path();
        ctx.move_to(x, y);
        ctx.line_to((x + dash).min(x1), y);
        ctx.stroke();
        x += dash + gap;
    }
}

fn draw_dot_run(ctx: &mut dyn RenderContext, x0: f64, x1: f64, y: f64, gap: f64) {
    let mut x = x0;
    while x <= x1 {
        ctx.begin_path();
        ctx.arc(x, y, 1.2, 0.0, std::f64::consts::TAU);
        ctx.fill();
        x += gap;
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_slider_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    field_id: &str,
    label: &str,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    with_input: bool,
) {
    use crate::ui::widgets::{render_single_slider, SliderConfig};

    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let mut cfg = SliderConfig::new(min, max).with_step(step).with_label_column(label_col);
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
        .then(|| crate::ui::widgets::SliderEditingInfo { text: ar.text.as_str(), cursor: ar.cursor, selection_start: ar.selection_start });
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
        result.slider_tracks.push(crate::layout::render_frame::SliderTrackInfo {
            field_id: field_id.to_string(),
            track_x: t.track_x,
            track_width: t.track_width,
            min_val: min,
            max_val: max,
        });
    }
}
