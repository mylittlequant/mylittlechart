//! Primitive settings modal — "Уровни" tab: the four palette cards, one
//! global fill-opacity slider, the ladder (checkbox, editable value,
//! preview bar, colour/fill chips, delete), the selected row's inspector,
//! and "+ добавить уровень" at the bottom. Part of the approved 3-tab
//! redesign, transcribed line-for-line from the `mlc-ui-lab` prototype's
//! approved design archive
//! (`crates/mlc-ui-lab/designs/2026-08-08-primitive-settings/approved_levels.rs`,
//! itself sourced from `crates/mlc-app-shell-web/pkg/fib_modal.html`'s
//! "УРОВНИ" column).
//!
//! The standalone "Набор" row and its own dropdown (`draw_set_row` /
//! `draw_set_dropdown`) were deleted 2026-08-08 (owner review, live) — the
//! level-set choice moved into the footer Template menu, which already
//! existed for saved templates
//! (`crate::layout::modals::primitive_settings::render_primitive_settings_modal`).
//! `draw_ramps_row` (the four palettes) stays where it was.
//!
//! `PrimitiveSettingsState::{level_set, palette}` are what the prototype's
//! own `DesignUiState` fields of the same name folded into on the app side
//! (see their own doc comments on `PrimitiveSettingsState`); `composition()`
//! below is the app's value-excluded-list view standing in for the
//! prototype's own positive `composition: Vec<f64>` list — see its own doc
//! comment for why that state-model difference exists. Every mutation
//! (набор swap, add, delete, colour/width/style edits, palette recolour)
//! happens in `crate::layout::modals::primitive_settings_intent` in
//! response to the widget ids this module registers, mirrored 1:1 by
//! `mlc-app/src/input/settings.rs` for the app-owned persistence half.

use crate::layout::render_frame::{PrimitiveSettingsResult, SliderTrackInfo};
use crate::layout::modals::primitive_settings_intent::is_palette_mode;
use crate::layout::modals::primitive_settings_palettes::PALETTES;
use crate::layout::modals::primitive_settings_property::{bucket_by_group, declared_properties, draw_property_row, properties_in};
use crate::layout::modals::primitive_settings_vid::{
    draw_group_header, draw_label, draw_slider_row, draw_split_value_control, draw_toggle_segment,
    line_style_display_name, SplitValueKind,
};
use crate::ui::modal_settings::PrimitiveSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    draw_input, draw_input_cursor, render_single_slider, InputConfig, InputType, SliderConfig, WidgetState,
    WidgetTheme, SETTINGS_BUTTON_RADIUS,
};
use mlc_core::drawing::primitives_v2::config::{ConfigProperty, FibLevelConfig, PropertyCategory};
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::i18n::{current_language, ConfigKey, SettingsKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

const ROW_H: f64 = 26.0;
const CHECK: f64 = 16.0;
const CHIP: f64 = 14.0;
const BAR_H: f64 = 14.0;
const DEL_W: f64 = 14.0;
const COL_GAP: f64 = 7.0;
const RAMP_CARD_H: f64 = 46.0;
const GLOBAL_ROW_H: f64 = 26.0;
const ADD_ROW_H: f64 = 34.0;
/// Height of one row in the add-level quick-value dropdown (item 10,
/// 2026-08-08 review) — shown under the add-level input only while it has
/// focus, instead of sitting permanently in the row.
const ADD_CHIP_ROW_H: f64 = 22.0;
/// The quick-add values offered under the add-level input while it has
/// focus (item 10, 2026-08-08 review). Module-level so both the anchor
/// computation (`draw_add_row`, which needs the menu's own height to decide
/// whether it fits below the input) and the paint pass
/// (`draw_add_chip_dropdown`) read the identical list.
const ADD_CHIP_VALUES: [&str; 3] = ["1.272", "1.618", "2.618"];
const BLOCK_GAP: f64 = 10.0;
/// Mirrors `primitive_settings_vid::GROUP_HEADER_H` — the Levels/Base
/// group's own default (`group: None`) run stays headerless, exactly as
/// before the W2 group model existed; only a named `PropertyGroup` run
/// (e.g. `PropertyGroup::General`) draws one, at the same height every
/// other tab's own group header uses.
const GROUP_HEADER_H: f64 = 22.0;
const INSPECTOR_ROW_H: f64 = 22.0;
const INSPECTOR_H: f64 = INSPECTOR_ROW_H * 5.0 + 20.0;
/// Gap between the top row's percent readout and its on/off toggle chip,
/// and between the slider's own track and that readout — same purpose as
/// `COL_GAP` in the ladder's own `x1`-derived columns.
const GLOBAL_TOGGLE_GAP: f64 = 8.0;
/// Reserved width for the "100%" readout, so the slider's own available
/// width can be computed up front (`render_single_slider` needs a concrete
/// `rect.width` before it draws, not measured after the fact).
const GLOBAL_PERCENT_W: f64 = 34.0;

/// The Levels/Base group's own rows: whatever the primitive declares into
/// `PropertyCategory::LevelsBase` (`style_properties()` and/or
/// `level_properties()`, sorted by their own `order`) if it declares
/// anything — `FibRetracement` (via `style_properties()`) and `Pitchfork`
/// (via `level_properties()`: `level_mode`/`extend`/`show_labels`, folded in
/// 2026-08-09 — see `primitive_settings_property::declared_properties`'s own
/// doc) are the two types that do today — or, for every OTHER level-having
/// primitive, the universal fallback: the SAME three base `PrimitiveData`
/// fields (colour/width/line style) every level falls back to, which this
/// tab already drew unconditionally before any primitive declared them.
/// Declaring vs. falling back renders the exact same three rows for
/// `FibRetracement` (same ids, same values) — the declaration only changes
/// how the modal LEARNED to draw them, not what it draws.
enum BaseGroupRows {
    /// The primitive declared its own Levels/Base properties — render them
    /// generically via `primitive_settings_property::draw_property_row`.
    Declared(Vec<mlc_core::drawing::primitives_v2::config::ConfigProperty>),
    /// No primitive-declared Levels/Base properties — the bespoke
    /// PrimitiveData-driven fallback (colour/width/line style), unchanged
    /// from before this primitive ever had the option to declare them.
    Fallback,
}

fn base_group_rows(prim: &dyn Primitive) -> BaseGroupRows {
    let all = declared_properties(prim);
    let declared: Vec<_> = properties_in(&all, PropertyCategory::LevelsBase).into_iter().map(|(p, _)| p.clone()).collect();
    if declared.is_empty() {
        BaseGroupRows::Fallback
    } else {
        BaseGroupRows::Declared(declared)
    }
}

/// The Extend Left / Extend Right / Trend Line ids (item 4, 2026-08-09):
/// merged onto one row whenever all three sit consecutively in a
/// primitive's own declared Levels/Base properties (`FibRetracement` is the
/// only declarer today — see its own `style_properties()` doc). A lone id
/// from this set with the other two missing or non-adjacent draws exactly
/// as before, one row per `ConfigProperty` — this is a narrow, id-scoped
/// merge, not a generic "group adjacent booleans" rule.
const EXTEND_TRIO: [&str; 3] = ["extend_left", "extend_right", "show_trend_line"];

/// Whether `props[i..i+3]` is exactly the [`EXTEND_TRIO`] set (any order —
/// declaration order is `order`-sorted 10/11/12 today, so in practice
/// already left-to-right). Takes a reference slice — what one
/// [`bucket_by_group`] run's own rows carry (the primitive's declared list
/// split by its own declared `PropertyGroup`, W2), rather than the whole
/// `BaseGroupRows::Declared` vec at once.
fn is_extend_trio_start_refs(props: &[&ConfigProperty], i: usize) -> bool {
    let Some(slice) = props.get(i..i + 3) else { return false };
    EXTEND_TRIO.iter().all(|id| slice.iter().any(|p| p.id == *id))
}

/// Visual row count for one already-bucketed Levels/Base run's own rows —
/// what `draw_top_section` actually paints for it, given `is_palette`: the
/// base colour row is skipped entirely in Палитра mode (item 1 — every
/// level's colour comes from the ramp there, so a base-colour control has
/// nothing to control), and the [`EXTEND_TRIO`] collapses from three rows to
/// one (item 4) — never reaching across a run boundary, since a merge only
/// ever looks within the slice it is handed. Shared by
/// [`base_group_content_height`] and `draw_top_section` so the two can never
/// drift apart, the same discipline every other height/draw pair in this
/// module already follows.
fn declared_run_visual_rows(props: &[&ConfigProperty], is_palette: bool) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < props.len() {
        if is_palette && props[i].id == "stroke_color" {
            i += 1;
            continue;
        }
        if is_extend_trio_start_refs(props, i) {
            n += 1;
            i += 3;
            continue;
        }
        n += 1;
        i += 1;
    }
    n
}

/// Visual CONTENT height for a [`BaseGroupRows`] (the Mono/Palette toggle
/// row itself is accounted separately by [`top_section_h`]) — `Declared`
/// buckets its own rows by declared [`PropertyGroup`] (W2 group model,
/// `bucket_by_group`): the default (`group: None`) run stays headerless,
/// exactly as this tab always rendered before named groups existed; a named
/// run (e.g. `PropertyGroup::General`, the ladder archetypes' own base
/// group) gets its own `GROUP_HEADER_H` above it. `Fallback` is unaffected —
/// it declares nothing to group.
fn base_group_content_height(rows: &BaseGroupRows, is_palette: bool) -> f64 {
    match rows {
        BaseGroupRows::Declared(props) => {
            let mut h = 0.0;
            for (group, run) in bucket_by_group(props, |p| p.group) {
                let n = declared_run_visual_rows(&run, is_palette);
                if n == 0 {
                    continue;
                }
                let header_h = if group.is_some() { GROUP_HEADER_H } else { 0.0 };
                h += header_h + n as f64 * ROW_H;
            }
            h
        }
        // colour (skipped in Палитра), width, line style.
        BaseGroupRows::Fallback => (if is_palette { 2 } else { 3 }) as f64 * ROW_H,
    }
}

/// Height of the top section (2026-08-08 appearance-surface consolidation):
/// the Mono/Palette colour-mode segment, then the Levels/Base group (see
/// [`BaseGroupRows`]) — moved out of the Appearance tab for a level-capable
/// primitive (`crate::layout::modals::primitive_settings_vid`'s own doc on
/// why), plus one `BLOCK_GAP` before whatever section follows (the four
/// palette cards in Palette mode, or straight to the global fill row in
/// Mono mode — see `draw`). `is_palette` must be the SAME value the caller
/// passes to `draw_top_section` — see [`base_group_content_height`]'s own
/// doc for why the content height depends on it.
fn top_section_h(prim: &dyn Primitive, is_palette: bool) -> f64 {
    let rows = base_group_rows(prim);
    ROW_H + base_group_content_height(&rows, is_palette) + BLOCK_GAP
}

const EPS: f64 = 1e-9;

/// Widget-id key for a level: its own value, 3-decimal — stable across a
/// resort, unlike a positional index would be (see
/// `PrimitiveSettingsState::levels_excluded`'s own doc comment).
pub fn vkey(level: f64) -> String {
    format!("{level:.3}")
}

/// The level VALUES currently shown as ladder rows — every
/// `level_configs()` entry whose value has not been explicitly excluded
/// this session (see `PrimitiveSettingsState::levels_excluded`). Not
/// filtered by each entry's own `visible` (the checkbox: dims a row, keeps
/// it in the list) — composition and visibility are independent axes.
pub fn composition(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> Vec<f64> {
    prim.level_configs()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.level)
        .filter(|level| !state.is_level_excluded(*level))
        .collect()
}

/// The body height this tab needs — computed before the shell opens (the
/// shell needs it up front), from the same numbers the draw pass below
/// uses, so the two can never drift apart. Static with respect to a live
/// settings-search query, same tradeoff `primitive_settings_vid`'s own
/// `content_height` accepts (see its doc comment): a filtered-out ladder
/// row just stops advancing `row_y` during the draw pass instead of
/// shrinking the reserved scroll range.
///
/// The four palette cards are reserved space ONLY in Палитра mode
/// (`is_palette_mode`) — real, not a query filter: switching to Моно hides
/// them in `draw` too, so a reserved-but-unpainted gap never shows up.
pub fn content_height(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    let configs = prim.level_configs().unwrap_or_default();
    let is_palette = is_palette_mode(&configs, state);
    let n = composition(prim, state).len();
    let inspector_h = if state.selected_level.is_some() { INSPECTOR_H } else { 0.0 };
    let ladder_h = (n as f64 * ROW_H + inspector_h).max(ROW_H);
    let ramps_h = if is_palette { RAMP_CARD_H + BLOCK_GAP } else { 0.0 };
    // The global fill row only exists for a primitive that actually has a
    // fill master (`Primitive::fill_master`) — a level-having primitive
    // with no fill concept at all (e.g. Pitchfork) used to reserve this row
    // anyway and draw a checkbox/slider that did nothing.
    let global_fill_h = if prim.fill_master().is_some() { GLOBAL_ROW_H + BLOCK_GAP } else { 0.0 };
    top_section_h(prim, is_palette) + ramps_h + global_fill_h + ladder_h + BLOCK_GAP + ADD_ROW_H
}

/// Body height for [`draw_search`] — the cross-tab settings-search result
/// view (item 8, 2026-08-08 follow-up). Unlike `content_height`, this is
/// NOT static: the four palette cards, the global fill-opacity row and the
/// "+ добавить уровень" row are chrome, not searchable settings (see
/// `draw_search`'s own doc comment), so a search result reserves no space
/// for them — only the ladder rows the query actually matches, plus the
/// selected row's own inspector when ITS level is one of the matches.
pub fn search_content_height(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    let query = super::primitive_settings::search_query(state);
    let matching: Vec<f64> = composition(prim, state)
        .into_iter()
        .filter(|level| super::primitive_settings::search_matches(&query, &[&vkey(*level)]))
        .collect();
    if matching.is_empty() {
        return 0.0;
    }
    let inspector_h = if matching.iter().any(|v| state.is_level_selected(*v)) { INSPECTOR_H } else { 0.0 };
    matching.len() as f64 * ROW_H + inspector_h
}

/// `footer_top` is the content viewport's own bottom edge — where the modal
/// footer begins — in the same absolute coordinate space `row_y` already
/// uses. Only `draw_add_row` needs it (to decide whether its quick-value
/// dropdown fits below the input, or must flip upward — item 2 follow-up,
/// 2026-08-08); every other row ignores it.
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
    footer_top: f64,
) {
    let configs = prim.level_configs().unwrap_or_default();
    let fallback_color = prim.data().color.stroke.clone();
    let fallback_width = prim.data().width;
    let query = super::primitive_settings::search_query(state);
    let is_palette = is_palette_mode(&configs, state);

    draw_top_section(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, *row_y, prim, is_palette);
    *row_y += top_section_h(prim, is_palette);

    // Палитра mode only — see `is_palette_mode`'s own doc. Switching to
    // Моно (`primitive_settings_intent::LevelsIntent::SetColorMode`) both
    // clears every level's own colour AND, via this same derived read,
    // hides the cards on the very next frame — no separate "close the
    // palette section" step to keep in sync.
    if is_palette {
        draw_ramps_row(ctx, toolbar_theme, result, x0, x1, *row_y, state);
        *row_y += RAMP_CARD_H + BLOCK_GAP;
    }

    // The global fill row only exists for a primitive with a real fill
    // master — see `content_height`'s own doc.
    if prim.fill_master().is_some() {
        draw_global_opacity_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y, prim);
        *row_y += GLOBAL_ROW_H + BLOCK_GAP;
    }

    let mut sorted: Vec<f64> = composition(prim, state);
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    draw_ladder(
        ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, &fallback_color, fallback_width,
        &configs, &sorted, &query, prim,
    );
    *row_y += BLOCK_GAP;

    draw_add_row(ctx, toolbar_theme, widget_theme, state, result, x0, *row_y, footer_top);
    *row_y += ADD_ROW_H;
}

/// Filtered draw entry point for the cross-tab settings-search result view
/// (item 8, 2026-08-08 follow-up): only the ladder's own per-level rows —
/// the four palette cards, the global fill-opacity row and the
/// "+ добавить уровень" row are chrome, not searchable settings, and have
/// no place in a search result (contrast `draw` above, which still shows
/// all three unconditionally for the normal, non-search single-tab view).
/// Reuses `draw_ladder` verbatim — the same per-row `search_matches` gate
/// the single-tab ladder already applied, not a second matching rule.
/// Returns `true` if at least one level matched the query — the caller
/// (the cross-tab search combiner in `primitive_settings.rs`) uses this to
/// decide whether to show this tab's own header.
#[allow(clippy::too_many_arguments)]
pub fn draw_search(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    row_y: &mut f64,
    prim: &dyn Primitive,
) -> bool {
    let configs = prim.level_configs().unwrap_or_default();
    let fallback_color = prim.data().color.stroke.clone();
    let fallback_width = prim.data().width;
    let query = super::primitive_settings::search_query(state);

    let mut sorted: Vec<f64> = composition(prim, state);
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let has_match = sorted.iter().any(|level| super::primitive_settings::search_matches(&query, &[&vkey(*level)]));
    if !has_match {
        return false;
    }
    draw_ladder(
        ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, &fallback_color, fallback_width,
        &configs, &sorted, &query, prim,
    );
    true
}

// =============================================================================
// Top section — colour mode (Моно/Палитра) + base colour/width/style
// (2026-08-08 appearance-surface consolidation). This is the ladder's own
// base: for a level-capable primitive it replaced Вид's LINE group
// entirely (`crate::layout::modals::primitive_settings_vid`'s own doc), so
// every row here reuses the SAME widget ids that group used —
// `stroke_color`/`stroke_width`/`line_style` — no new dispatch wiring on
// either host (`mlc-app`'s `input/settings.rs`/`sliders.rs`,
// `mlc-ui-lab`'s `dispatch.rs`) was needed for them.
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_top_section(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    row_y: f64,
    prim: &dyn Primitive,
    is_palette: bool,
) {
    let lang = current_language();
    let mut y = row_y;

    // Row 1: Моно / Палитра — derived from the composition, never stored
    // (`is_palette_mode`'s own doc, computed once by the caller and passed
    // in as `is_palette` so this row and the base-colour skip below can
    // never disagree). No leading text label: the two segment labels
    // already say what they do, and a third "colour mode" caption next to
    // the "Base colour" row's own "Color" label two rows down would read as
    // the same word twice for two different things.
    let mono_label = ConfigKey::ColorModeMono.get(lang);
    let palette_label = ConfigKey::ColorModePalette.get(lang);
    ctx.set_font("11px sans-serif");
    let seg_w = (ctx.measure_text(mono_label).max(ctx.measure_text(palette_label)) + 16.0).max(56.0);
    let mono_rect = WidgetRect::new(x0, y + 2.0, seg_w, ROW_H - 4.0);
    let palette_rect = WidgetRect::new(mono_rect.right() + 4.0, y + 2.0, seg_w, ROW_H - 4.0);
    draw_toggle_segment(ctx, toolbar_theme, mono_rect, mono_label, !is_palette);
    draw_toggle_segment(ctx, toolbar_theme, palette_rect, palette_label, is_palette);
    result.content_items.push(("levels_color_mode:mono".to_string(), mono_rect));
    result.content_items.push(("levels_color_mode:palette".to_string(), palette_rect));
    y += ROW_H;

    // Rows 2..N: the Levels/Base group — see [`BaseGroupRows`]. A primitive
    // that declares its own (`FibRetracement` today) renders generically
    // from `PropertyType`; every other level-having primitive falls back to
    // the same three bespoke `PrimitiveData`-driven rows this tab always
    // drew (`"stroke_color"`/`"stroke_width"`/`"line_style"` — no new
    // dispatch wiring on either host, `mlc-app`'s `input/settings.rs`/
    // `sliders.rs` or `mlc-ui-lab`'s `dispatch.rs`, is needed for the
    // fallback path since it is byte-for-byte what already shipped).
    match base_group_rows(prim) {
        BaseGroupRows::Declared(props) => {
            // Bucketed by declared `PropertyGroup` (W2 group model): the
            // default (`group: None`) run draws exactly as this tab always
            // did — no header — a named run (e.g. `PropertyGroup::General`)
            // gets its own header first. The Extend-trio merge and the
            // Палитра base-colour skip apply WITHIN each run's own slice,
            // never across a run boundary.
            for (group, run) in bucket_by_group(&props, |p| p.group) {
                if declared_run_visual_rows(&run, is_palette) == 0 {
                    continue;
                }
                if let Some(g) = group {
                    draw_group_header(ctx, toolbar_theme, x0, x1, y, g.label(), None);
                    y += GROUP_HEADER_H;
                }
                let mut i = 0;
                while i < run.len() {
                    // Base colour swatch — not drawn at all in Палитра mode
                    // (item 1): every level's own colour comes from the ramp
                    // there, so this control has nothing left to control.
                    if is_palette && run[i].id == "stroke_color" {
                        i += 1;
                        continue;
                    }
                    // Extend Left / Extend Right / Trend Line — one combined
                    // row instead of three stacked ones (item 4).
                    if is_extend_trio_start_refs(&run, i) {
                        draw_extend_trio_row(ctx, toolbar_theme, result, x0, x1, y, [run[i], run[i + 1], run[i + 2]]);
                        y += ROW_H;
                        i += 3;
                        continue;
                    }
                    draw_property_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, y, mlc_core::drawing::primitives_v2::PropertyKind::Style, run[i]);
                    y += ROW_H;
                    i += 1;
                }
            }
        }
        BaseGroupRows::Fallback => {
            // Row: base colour — writes `prim.data().color.stroke` under
            // the SAME `"stroke_color"` id the Appearance tab's own swatch
            // used. Meaningful only in Моно mode, where every level draws
            // in it directly — not drawn at all in Палитра mode (item 1):
            // every level's own colour comes from the ramp there, so this
            // control has nothing left to control.
            if !is_palette {
                let color_label = SettingsKey::Color.get(lang);
                draw_label(ctx, toolbar_theme, x0, label_col, y, color_label);
                let swatch_w = (x1 - (x0 + label_col)).clamp(40.0, 60.0);
                let color_rect = WidgetRect::new(x0 + label_col, y + 3.0, swatch_w, ROW_H - 6.0);
                ctx.set_fill_color(&prim.data().color.stroke);
                ctx.fill_rounded_rect(color_rect.x, color_rect.y, color_rect.width, color_rect.height, 4.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(color_rect.x, color_rect.y, color_rect.width, color_rect.height, 4.0);
                result.content_items.push(("stroke_color".to_string(), color_rect));
                y += ROW_H;
            }

            // Row: base width — `"stroke_width"`, same id/slider the
            // Appearance tab used. A global writer (item 3): both hosts
            // (`mlc-app`'s `sliders.rs`/`settings.rs`, `mlc-ui-lab`'s
            // `dispatch.rs`) push this value into every level's own
            // `cfg.width` alongside this base field — see
            // `primitive_settings_intent::apply_global_width`'s own doc.
            let width_label = ConfigKey::WidthLabel.get(lang);
            draw_slider_row(
                ctx, toolbar_theme, widget_theme, state, result, x0, x1, label_col, y, "stroke_width", width_label,
                prim.data().width, 1.0, 10.0, 0.1, true,
            );
            y += ROW_H;

            // Row: base style — `"line_style"`, same split control the
            // Appearance tab used. A global writer too (item 3) — see
            // `primitive_settings_intent::apply_global_style`'s own doc.
            let style_label = ConfigKey::Style.get(lang);
            draw_label(ctx, toolbar_theme, x0, label_col, y, style_label);
            let control_w = (x1 - (x0 + label_col)).max(100.0);
            let style_rect = WidgetRect::new(x0 + label_col, y + 2.0, control_w, ROW_H - 4.0);
            let style = prim.data().style;
            draw_split_value_control(
                ctx, toolbar_theme, state, result, style_rect, "line_style", line_style_display_name(style, lang),
                SplitValueKind::LineStyle(style),
            );
        }
    }
}

/// One row for the [`EXTEND_TRIO`] (item 4): three checkbox+label pairs
/// side by side instead of three stacked full-width rows. `props` is the
/// trio in whatever order `base_group_rows` sorted it into (its own
/// declared `order`, 10/11/12 today), drawn left to right in that order.
/// Registers each checkbox under the SAME `style_prop:{id}` id the generic
/// `draw_property_row`/`Boolean` path already used for it — the shape
/// `field_id()` in `primitive_settings_property` produces for any id that
/// is not one of the three bare `PrimitiveData` exceptions — so neither
/// host's click dispatch needed any new wiring for this row.
///
/// Distributed across the row's own whole width (item 2, 2026-08-09), not
/// bunched into the left third of three fixed equal columns: each group's
/// own natural width (checkbox + gap + its OWN measured label, not a
/// shared fixed column) is measured first, then the leftover
/// `x1 - x0 - total_content_width` is split into the two gaps BETWEEN the
/// three groups — `justify-content: space-between`, derived from `x0`/`x1`
/// so it stays correct at every modal width. The first group starts flush
/// at `x0`; the last group's own content — not just its column — ends
/// flush at `x1`.
fn draw_extend_trio_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    props: [&ConfigProperty; 3],
) {
    const LABEL_GAP: f64 = 5.0;

    ctx.set_font("11px sans-serif");
    let group_widths: Vec<f64> = props.iter().map(|p| CHECK + LABEL_GAP + ctx.measure_text(&p.name)).collect();
    let total_w: f64 = group_widths.iter().sum();
    let available = (x1 - x0).max(0.0);
    // Two gaps between three groups. Clamped to zero so three labels that
    // do not fit at a very narrow modal width sit adjacent from `x0`
    // instead of the last one overshooting past `x1`.
    let gap = ((available - total_w) / (props.len() - 1) as f64).max(0.0);

    let mut col_x = x0;
    for (i, prop) in props.iter().enumerate() {
        let checked = prop.value.as_bool().unwrap_or(false);
        let check_rect = WidgetRect::new(col_x, y + (ROW_H - CHECK) / 2.0, CHECK, CHECK);
        draw_checkbox(ctx, check_rect, checked, toolbar_theme, &toolbar_theme.background.clone());
        result.content_items.push((format!("style_prop:{}", prop.id), check_rect));

        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&prop.name, check_rect.right() + LABEL_GAP, y + ROW_H / 2.0);

        col_x += group_widths[i] + gap;
    }
}

// =============================================================================
// The four palette cards
// =============================================================================

fn draw_ramps_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    state: &PrimitiveSettingsState,
) {
    let gap = 7.0;
    let card_w = ((x1 - x0) - gap * 3.0) / 4.0;
    for (i, palette) in PALETTES.iter().enumerate() {
        let rect = WidgetRect::new(x0 + i as f64 * (card_w + gap), y, card_w, RAMP_CARD_H);
        let selected = state.palette == i;
        ctx.set_stroke_color(if selected { &toolbar_theme.accent } else { &toolbar_theme.separator });
        ctx.set_stroke_width(if selected { 2.0 } else { 1.0 });
        ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);

        let strip_h = 22.0;
        let strip_y = rect.y + 5.0;
        let seg_w = (rect.width - 10.0) / palette.colors.len() as f64;
        for (j, c) in palette.colors.iter().enumerate() {
            ctx.set_fill_color(c);
            ctx.fill_rect(rect.x + 5.0 + seg_w * j as f64, strip_y, seg_w, strip_h);
        }

        ctx.set_font("10px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(palette.name_key.get(current_language()), rect.center_x(), strip_y + strip_h + 10.0);

        result.content_items.push((format!("levels_palette:{i}"), rect));
    }
    // Restore left alignment for whatever draws next — `render_single_slider`
    // (called right after this by `draw_global_opacity_row`) never sets its
    // own text alignment before drawing its label, so leaving Center behind
    // clips that label's leading half against its own start x.
    ctx.set_text_align(TextAlign::Left);
}

// =============================================================================
// Global fill toggle + "Заливка между" (fill opacity) slider — the ONE fill
// toggle and ONE opacity the modal now has (2026-08-08 appearance-surface
// consolidation, collapsing Вид's old `show_fill`/`fill_opacity` FILL group
// into this tab's pre-existing global row instead of keeping two places
// that could disagree).
//
// Single-source-of-truth choice: this checkbox and slider mirror the
// PRIMITIVE's own fill master (`Primitive::fill_master`) directly — NOT a
// bulk write across every level's own `fill_enabled`/`fill_opacity`, which
// is what this row used to do, and NOT the Appearance tab's generic FILL
// group either: `fill_master`'s default reads the same `"fill"`/
// `"fill_opacity"` style-property ids that group would generically render
// (so a type like `FibWedge`, which still declares them, is unaffected),
// but `FibRetracement` overrides it to read its own dedicated fields
// directly, having deliberately stopped DECLARING those two ids (see its
// own doc) so the Appearance tab's generic renderer can no longer put a
// second, independent control next to this one for the same field. Reason:
// at render (`fibonacci::retracement::FibRetracement::render`, representative
// of every primitive with a level ladder and a real fill) a band paints
// only when BOTH `self.show_fill` (the ONE primitive-level master this row
// now owns) AND that band's own `cfg.fill_enabled` (the ladder row's own
// second chip, untouched here — row detail, not a duplicate of the master)
// are true, and its opacity is ALWAYS `self.fill_opacity` — the per-level
// `cfg.fill_opacity` this row used to bulk-write is never read there at
// all. Driving the master fields directly is therefore the only reading
// that cannot contradict what actually reaches the chart; the old bulk
// write changed a value the renderer ignored while leaving the true master
// (`show_fill`) exactly as a completely different, session-only bookkeeping
// field (`PrimitiveSettingsState::level_fill_opacity`, now deleted) had
// nothing to do with it.
//
// The ids stay `"levels_global_fill_toggle"`/`"levels_global_opacity"` —
// unchanged from before this port — so this row keeps the click-to-jump
// slider behaviour `mlc-app/src/input/settings.rs` already special-cases
// for `"levels_global_opacity"` (a bare click, not just a drag, commits a
// value); switching to the generic `style_prop:fill_opacity` id would have
// silently dropped that.
#[allow(clippy::too_many_arguments)]
fn draw_global_opacity_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    prim: &dyn Primitive,
) {
    const FIELD: &str = "levels_global_opacity";
    const TOGGLE_ID: &str = "levels_global_fill_toggle";
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    // Caller (`draw`) only invokes this when `fill_master()` is `Some` —
    // `unwrap_or` here is a fallback for the theoretical "changed between
    // the height pass and the draw pass" race, not a real path.
    let (fill_on, fill_opacity_committed) = prim.fill_master().unwrap_or((false, 0.0));

    // Right-anchored off `x1`, same as the ladder's own columns
    // (`draw_ladder`'s `col4_x`/`col5_x`): the toggle chip flush to the
    // edge, the percent readout's reserved width just left of it, and the
    // slider taking whatever is left back to `x0` — so at the modal's
    // narrow end the toggle and readout never collide with the track.
    let toggle_rect = WidgetRect::new(x1 - CHECK, y + (GLOBAL_ROW_H - CHECK) / 2.0, CHECK, CHECK);
    let slider_right = toggle_rect.x - GLOBAL_TOGGLE_GAP - GLOBAL_PERCENT_W - GLOBAL_TOGGLE_GAP;

    let slider_cfg = SliderConfig::new(0.0, 1.0).without_input();
    let display = state
        .slider_drag
        .as_ref()
        .filter(|d| d.field_id == FIELD)
        .and_then(|d| d.floating_value)
        .unwrap_or(fill_opacity_committed);
    let hovered = state.slider_drag.as_ref().map(|d| d.field_id == FIELD).unwrap_or(false);
    let rect = WidgetRect::new(x0, y, (slider_right - x0).max(80.0), GLOBAL_ROW_H);
    let label = ConfigKey::GlobalFillBetween.get(current_language());
    let res = render_single_slider(ctx, &slider_cfg, display, rect, label, widget_theme, hovered, None);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&widget_theme.text_normal);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&format!("{:.0}%", display * 100.0), res.track_rect.right() + GLOBAL_TOGGLE_GAP, y + GLOBAL_ROW_H / 2.0);
    result.content_items.push((FIELD.to_string(), res.full_rect));
    if let Some(t) = res.track_info {
        result.slider_tracks.push(SliderTrackInfo { field_id: FIELD.to_string(), track_x: t.track_x, track_width: t.track_width, min_val: 0.0, max_val: 1.0 });
    }

    // Global fill on/off: mirrors the primitive's own `show_fill` (the
    // `"fill"` style property) directly — see this function's own doc for
    // why that, and not a per-level bulk flag, is the single source of
    // truth.
    draw_checkbox(ctx, toggle_rect, fill_on, toolbar_theme, &toolbar_theme.background.clone());
    result.content_items.push((TOGGLE_ID.to_string(), toggle_rect));
}

// =============================================================================
// The ladder
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_ladder(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    row_y: &mut f64,
    fallback_color: &str,
    fallback_width: f64,
    configs: &[FibLevelConfig],
    sorted_composition: &[f64],
    query: &str,
    prim: &dyn Primitive,
) {
    if sorted_composition.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(ConfigKey::EmptyLadder.get(current_language()), x0, *row_y);
        *row_y += ROW_H;
        return;
    }

    let chip_block_w = CHIP * 2.0 + 5.0;
    let col1_x = x0;
    let col2_x = x0 + CHECK + COL_GAP;
    let col5_x = x1 - DEL_W;
    let col4_x = col5_x - COL_GAP - chip_block_w;
    let col3_x = col2_x + 52.0 + COL_GAP;
    let col3_w = (col4_x - COL_GAP - col3_x).max(0.0);

    for (row_idx, &level) in sorted_composition.iter().enumerate() {
        let Some(cfg) = configs.iter().find(|c| (c.level - level).abs() < EPS) else { continue };
        // Cross-tab settings search (item 8, 2026-08-08 review): a row
        // whose own value does not match the live query simply does not
        // advance `row_y` — same "stop advancing, don't shrink the
        // reserved height" tradeoff `primitive_settings_vid` already makes
        // for its own filtered-out rows.
        if !super::primitive_settings::search_matches(query, &[&vkey(level)]) {
            continue;
        }
        let y = *row_y;
        let this_color = cfg.color.as_deref().unwrap_or(fallback_color);
        let next_color: Option<String> = sorted_composition
            .get(row_idx + 1)
            .and_then(|nv| configs.iter().find(|c| (c.level - nv).abs() < EPS))
            .map(|n| n.color.clone().unwrap_or_else(|| fallback_color.to_string()));

        let selected = state.is_level_selected(level);
        let key = vkey(level);

        if row_idx > 0 {
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(x0, y);
            ctx.line_to(x1, y);
            ctx.stroke();
        }
        if selected {
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, 0.14));
            ctx.fill_rect(x0, y, x1 - x0, ROW_H);
        }

        // Registered FIRST so the checkbox / value field / chips / delete
        // (registered after, below) win the hit test on their own smaller
        // rects — "last registered wins" — everywhere else on the row
        // falls through to select/collapse its inspector.
        result.content_items.push((format!("levels_row_select:{key}"), WidgetRect::new(x0, y, x1 - x0, ROW_H)));

        let alpha = if cfg.visible { 1.0 } else { 0.42 };
        ctx.set_global_alpha(alpha);

        let check_rect = WidgetRect::new(col1_x, y + (ROW_H - CHECK) / 2.0, CHECK, CHECK);
        draw_checkbox(ctx, check_rect, cfg.visible, toolbar_theme, &toolbar_theme.background.clone());
        result.content_items.push((format!("level_v{key}_visible"), check_rect));

        let value_rect = WidgetRect::new(col2_x, y + 2.0, 52.0, ROW_H - 4.0);
        draw_level_value_field(ctx, toolbar_theme, widget_theme, state, result, value_rect, &key, level, prim);

        let bar_rect = WidgetRect::new(col3_x, y + (ROW_H - BAR_H) / 2.0, col3_w, BAR_H);
        draw_level_bar(ctx, bar_rect, this_color, next_color.as_deref(), cfg.visible);

        let color_chip = WidgetRect::new(col4_x, y + (ROW_H - CHIP) / 2.0, CHIP, CHIP);
        ctx.set_fill_color(this_color);
        ctx.fill_rounded_rect(color_chip.x, color_chip.y, color_chip.width, color_chip.height, 3.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(color_chip.x, color_chip.y, color_chip.width, color_chip.height, 3.0);
        result.content_items.push((format!("level_v{key}_color"), color_chip));

        // Second chip = this level's band on/off (item 6, 2026-08-08
        // review; re-routed from a colour-picker opener to a toggle in the
        // 2026-08-08 follow-up — the fill COLOUR itself moved into the
        // inspector's own `level_v{key}_fill_color`, see `draw_inspector`):
        // "on" reads as a solid, highlighted-border chip; "off" as a
        // hollow one (background fill, thin muted border) — a plainly
        // different SHAPE, not just a dimmer copy of the "on" one, so a
        // glance at the column tells which rows have a band enabled.
        let fill_color = cfg.fill_color.as_deref().unwrap_or(this_color);
        let fill_chip = WidgetRect::new(col4_x + CHIP + 5.0, y + (ROW_H - CHIP) / 2.0, CHIP, CHIP);
        if cfg.fill_enabled {
            ctx.set_fill_color(&mlc_core::apply_opacity(fill_color, cfg.fill_opacity.max(0.35)));
            ctx.fill_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
            ctx.set_stroke_color(&toolbar_theme.item_bg_active);
            ctx.set_stroke_width(1.5);
        } else {
            ctx.set_fill_color(&toolbar_theme.background);
            ctx.fill_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
        }
        ctx.stroke_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
        result.content_items.push((format!("level_v{key}_fill"), fill_chip));

        if state.is_level_overridden(level) {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.begin_path();
            ctx.arc(color_chip.x - 3.0, color_chip.y - 1.0, 2.0, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }

        ctx.set_global_alpha(1.0);

        let del_rect = WidgetRect::new(col5_x, y + (ROW_H - CHIP) / 2.0, DEL_W, CHIP);
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("\u{d7}", del_rect.center_x(), del_rect.center_y());
        result.content_items.push((format!("level_v{key}_delete"), del_rect));

        *row_y += ROW_H;

        if selected {
            let next_label = sorted_composition.get(row_idx + 1).map(|n| prim.level_value_label(*n));
            draw_inspector(
                ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y, &key, cfg,
                fallback_color, fallback_width, next_label,
            );
            *row_y += INSPECTOR_H;
        }
    }
}

pub fn draw_checkbox(ctx: &mut dyn RenderContext, rect: WidgetRect, checked: bool, toolbar_theme: &ToolbarTheme, off_bg: &str) {
    ctx.set_fill_color(if checked { &toolbar_theme.item_bg_active } else { off_bg });
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
}

#[allow(clippy::too_many_arguments)]
pub fn draw_level_value_field(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    key: &str,
    level: f64,
    prim: &dyn Primitive,
) {
    let field_id = format!("level_v{key}_value");
    let is_editing = state.active_field_id.as_deref() == Some(field_id.as_str());
    let label = prim.level_value_label(level);
    let ar = &state.active_render;
    let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
        (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
    } else {
        (&label, label.len(), None, None)
    };
    let cfg = InputConfig::new(display)
        .with_focused(is_editing)
        .with_type(InputType::Number)
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
    result.content_items.push((field_id, rect));
}

pub fn draw_level_bar(ctx: &mut dyn RenderContext, rect: WidgetRect, this_color: &str, next_color: Option<&str>, visible: bool) {
    let band_alpha = if visible { 0.16 } else { 0.05 };
    let a = mlc_core::apply_opacity(this_color, band_alpha);
    ctx.begin_path();
    ctx.rect(rect.x, rect.y, rect.width, rect.height);
    match next_color {
        Some(nc) => {
            let b = mlc_core::apply_opacity(nc, band_alpha);
            ctx.fill_linear_gradient(&[(0.0, a.as_str()), (1.0, b.as_str())], rect.x, rect.y, rect.right(), rect.y);
        }
        None => {
            ctx.set_fill_color(&a);
            ctx.fill();
        }
    }
    let line_alpha = if visible { 1.0 } else { 0.35 };
    let line_color = mlc_core::apply_opacity(this_color, line_alpha);
    ctx.set_stroke_color(&line_color);
    ctx.set_stroke_width(2.0);
    ctx.begin_path();
    let mid_y = rect.y + rect.height / 2.0;
    ctx.move_to(rect.x, mid_y);
    ctx.line_to(rect.right(), mid_y);
    ctx.stroke();
}

// =============================================================================
// The inspector — line colour / width / style / fill colour+target / fill
// opacity, for the selected row. The fill on/off toggle itself lives on the
// ladder row's own second chip (`draw_ladder`), not here — see Row 4 below.
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_inspector(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y0: f64,
    key: &str,
    cfg: &FibLevelConfig,
    fallback_color: &str,
    fallback_width: f64,
    next_label: Option<String>,
) {
    let lang = current_language();
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(x0, y0, x1 - x0, INSPECTOR_H);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x0, y0);
    ctx.line_to(x1, y0);
    ctx.stroke();

    let label_x = x0 + 10.0;
    let control_x = x0 + 100.0;
    let control_w = (x1 - 10.0 - control_x).max(60.0);
    let mut y = y0 + 10.0;

    let draw_label = |ctx: &mut dyn RenderContext, text: &str, y: f64| {
        ctx.set_font("11.5px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(text, label_x, y + INSPECTOR_ROW_H / 2.0);
    };

    // Row 1: line colour.
    draw_label(ctx, ConfigKey::LineColorLabel.get(lang), y);
    let this_color = cfg.color.as_deref().unwrap_or(fallback_color);
    let color_chip = WidgetRect::new(control_x, y + 3.0, CHIP + 4.0, CHIP + 4.0);
    ctx.set_fill_color(this_color);
    ctx.fill_rounded_rect(color_chip.x, color_chip.y, color_chip.width, color_chip.height, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(color_chip.x, color_chip.y, color_chip.width, color_chip.height, 3.0);
    result.content_items.push((format!("level_v{key}_color"), color_chip));
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(this_color, color_chip.right() + 8.0, y + INSPECTOR_ROW_H / 2.0);
    y += INSPECTOR_ROW_H;

    // Row 2: width.
    draw_label(ctx, ConfigKey::WidthLabel.get(lang), y);
    let width_field = format!("level_v{key}_width");
    let width_slider_cfg = SliderConfig::new(1.0, 10.0).with_step(0.1).without_input();
    let committed_width = cfg.width.unwrap_or(fallback_width);
    let display_width = state
        .slider_drag
        .as_ref()
        .filter(|d| d.field_id == width_field)
        .and_then(|d| d.floating_value)
        .unwrap_or(committed_width);
    let hovered_width = state.slider_drag.as_ref().map(|d| d.field_id == width_field).unwrap_or(false);
    let width_rect = WidgetRect::new(control_x, y, control_w.min(160.0), INSPECTOR_ROW_H);
    let width_result = render_single_slider(ctx, &width_slider_cfg, display_width, width_rect, "", widget_theme, hovered_width, None);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&format!("{display_width:.1}"), width_result.track_rect.right() + 8.0, y + INSPECTOR_ROW_H / 2.0);
    result.content_items.push((width_field.clone(), width_result.full_rect));
    if let Some(t) = width_result.track_info {
        result.slider_tracks.push(SliderTrackInfo { field_id: width_field, track_x: t.track_x, track_width: t.track_width, min_val: 1.0, max_val: 10.0 });
    }
    y += INSPECTOR_ROW_H;

    // Row 3: style — solid / dashed / dotted, direct-select.
    draw_label(ctx, ConfigKey::Style.get(lang), y);
    let seg_w = 34.0;
    let seg_h = INSPECTOR_ROW_H - 4.0;
    for (i, style_id) in ["solid", "dashed", "dotted"].iter().enumerate() {
        let seg_rect = WidgetRect::new(control_x + i as f64 * (seg_w + 4.0), y + 2.0, seg_w, seg_h);
        let selected = cfg.style == *style_id;
        draw_style_segment(ctx, seg_rect, style_id, selected, toolbar_theme, &toolbar_theme.background.clone());
        result.content_items.push((format!("level_v{key}_style:{style_id}"), seg_rect));
    }
    y += INSPECTOR_ROW_H;

    // Row 4: fill colour + "up to <next level>". The on/off toggle itself
    // lives on the ladder row's own second chip now (`level_v{key}_fill`,
    // `draw_ladder` — 2026-08-08 follow-up); the inspector's own separate
    // checkbox is gone, so this row offers only what the row chip does
    // not — the fill's own colour, under its own id,
    // `level_v{key}_fill_color`, distinct from the row chip's toggle id.
    draw_label(ctx, ConfigKey::Fill.get(lang), y);
    let fill_color = cfg.fill_color.as_deref().unwrap_or(this_color);
    let fill_chip = WidgetRect::new(control_x, y + (INSPECTOR_ROW_H - CHIP) / 2.0, CHIP, CHIP);
    ctx.set_fill_color(&mlc_core::apply_opacity(fill_color, cfg.fill_opacity.max(0.2)));
    ctx.fill_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
    result.content_items.push((format!("level_v{key}_fill_color"), fill_chip));
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let target_label = next_label
        .map(|l| format!("{} {l}", ConfigKey::UpToLevel.get(lang)))
        .unwrap_or_else(|| "\u{2014}".to_string());
    ctx.fill_text(&target_label, fill_chip.right() + 8.0, y + INSPECTOR_ROW_H / 2.0);
    y += INSPECTOR_ROW_H;

    // Row 5: fill opacity.
    draw_label(ctx, ConfigKey::FillOpacity.get(lang), y);
    let opacity_field = format!("level_v{key}_fill_opacity");
    let opacity_slider_cfg = SliderConfig::new(0.0, 1.0).without_input();
    let display_opacity = state
        .slider_drag
        .as_ref()
        .filter(|d| d.field_id == opacity_field)
        .and_then(|d| d.floating_value)
        .unwrap_or(cfg.fill_opacity);
    let hovered_opacity = state.slider_drag.as_ref().map(|d| d.field_id == opacity_field).unwrap_or(false);
    let opacity_rect = WidgetRect::new(control_x, y, control_w.min(160.0), INSPECTOR_ROW_H);
    let opacity_result = render_single_slider(ctx, &opacity_slider_cfg, display_opacity, opacity_rect, "", widget_theme, hovered_opacity, None);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&format!("{:.0}%", display_opacity * 100.0), opacity_result.track_rect.right() + 8.0, y + INSPECTOR_ROW_H / 2.0);
    result.content_items.push((opacity_field.clone(), opacity_result.full_rect));
    if let Some(t) = opacity_result.track_info {
        result.slider_tracks.push(SliderTrackInfo { field_id: opacity_field, track_x: t.track_x, track_width: t.track_width, min_val: 0.0, max_val: 1.0 });
    }
}

pub fn draw_style_segment(ctx: &mut dyn RenderContext, rect: WidgetRect, style_id: &str, selected: bool, toolbar_theme: &ToolbarTheme, off_bg: &str) {
    ctx.set_fill_color(if selected { &toolbar_theme.item_bg_active } else { off_bg });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);

    let color = if selected { toolbar_theme.item_text_active.clone() } else { toolbar_theme.item_text.clone() };
    ctx.set_stroke_color(&color);
    ctx.set_stroke_width(1.5);
    let mid_y = rect.center_y();
    let sx0 = rect.x + 6.0;
    let sx1 = rect.right() - 6.0;
    match style_id {
        "dashed" => {
            let mut x = sx0;
            while x < sx1 {
                ctx.begin_path();
                ctx.move_to(x, mid_y);
                ctx.line_to((x + 4.0).min(sx1), mid_y);
                ctx.stroke();
                x += 7.0;
            }
        }
        "dotted" => {
            let mut x = sx0;
            while x <= sx1 {
                ctx.set_fill_color(&color);
                ctx.begin_path();
                ctx.arc(x, mid_y, 1.0, 0.0, std::f64::consts::TAU);
                ctx.fill();
                x += 4.0;
            }
        }
        _ => {
            ctx.begin_path();
            ctx.move_to(sx0, mid_y);
            ctx.line_to(sx1, mid_y);
            ctx.stroke();
        }
    }
}

// =============================================================================
// "+ добавить уровень"
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_add_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    y: f64,
    footer_top: f64,
) {
    let lang = current_language();
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.accent);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let mid_y = y + ADD_ROW_H / 2.0;
    ctx.fill_text("+", x0, mid_y);

    let add_level_label = ConfigKey::AddLevel.get(lang);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.fill_text(add_level_label, x0 + 14.0, mid_y);

    let field_id = "levels_add_value";
    let is_editing = state.active_field_id.as_deref() == Some(field_id);
    let ar = &state.active_render;
    let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
        (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
    } else {
        ("", 0, None, None)
    };
    let field_x = x0 + 14.0 + ctx.measure_text(add_level_label) + 10.0;
    let field_rect = WidgetRect::new(field_x, y + (ADD_ROW_H - 22.0) / 2.0, 60.0, 22.0);
    let input_cfg = InputConfig::new(display)
        .with_focused(is_editing)
        .with_type(InputType::Number)
        .with_font_size(11.5)
        .with_padding(4.0)
        .with_radius(3.0)
        .with_cursor(cursor)
        .with_selection(sel_start, sel_end);
    let res = draw_input(ctx, &input_cfg, WidgetState::Normal, field_rect, widget_theme);
    if is_editing {
        draw_input_cursor(ctx, res.cursor_x, res.cursor_y, res.cursor_height, &toolbar_theme.item_text);
        result.active_input_char_positions = res.char_x_positions;
        result.active_input_rect = Some(field_rect);
    }
    result.content_items.push((field_id.to_string(), field_rect));

    // Quick-add values (item 10, 2026-08-08 review): a dropdown under the
    // input, shown only while it has focus — not a permanent row fixture.
    // Typing a value by hand stays possible either way; this only offers a
    // shortcut for the common ones. This pass only computes WHERE it goes
    // (flipping it upward when there is no room below, before the footer)
    // and stores the anchor on `result`; the actual paint + registration
    // happens in the OUTER pass (`primitive_settings::
    // render_primitive_settings_modal`, via `draw_add_chip_dropdown`
    // below), after `scrollable.end()` closes THIS function's own scroll
    // clip — drawing it in here, inside the clip, is what let it slide
    // under the footer and get clipped (item 2 follow-up, 2026-08-08).
    if is_editing {
        let menu_h = ADD_CHIP_ROW_H * ADD_CHIP_VALUES.len() as f64 + 6.0;
        let space_below = footer_top - field_rect.bottom();
        let open_upward = space_below < menu_h + 2.0;
        result.add_chip_dropdown_anchor = Some((field_rect, open_upward));
    }
}

/// Paint + register the add-level quick-value dropdown from the anchor
/// `draw_add_row` above computed (`field_rect`, `open_upward`). Called from
/// the OUTER pass (`primitive_settings::render_primitive_settings_modal`),
/// after `scrollable.end()` closes the ladder's own scroll clip and after
/// `body_items_end` is captured — so this menu is never clipped by the
/// ladder's viewport and never culled against it at registration time
/// (item 2 follow-up, 2026-08-08; mirrors `template_dropdown_menu`'s own
/// outside-the-clip treatment in that same function).
pub(crate) fn draw_add_chip_dropdown(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    field_rect: WidgetRect,
    x1: f64,
    open_upward: bool,
) {
    let menu_w = field_rect.width.max(70.0).min((x1 - field_rect.x).max(70.0));
    let total_h = ADD_CHIP_ROW_H * ADD_CHIP_VALUES.len() as f64 + 6.0;
    let menu_y = if open_upward { field_rect.y - 2.0 - total_h } else { field_rect.bottom() + 2.0 };

    ctx.set_fill_color(&toolbar_theme.dropdown_bg);
    ctx.fill_rounded_rect(field_rect.x, menu_y, menu_w, total_h, SETTINGS_BUTTON_RADIUS);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(field_rect.x, menu_y, menu_w, total_h, SETTINGS_BUTTON_RADIUS);

    let mut row_y = menu_y + 3.0;
    for v in ADD_CHIP_VALUES {
        let row_rect = WidgetRect::new(field_rect.x, row_y, menu_w, ADD_CHIP_ROW_H);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(v, row_rect.center_x(), row_rect.center_y());
        result.content_items.push((format!("levels_add_chip:{v}"), row_rect));
        row_y += ADD_CHIP_ROW_H;
    }
}
