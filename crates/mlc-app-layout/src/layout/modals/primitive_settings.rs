//! Primitive settings modal renderer.
//!
//! The tab strip is DERIVED, not fixed: up to four tabs — Levels (the level
//! ladder) / Appearance (line/fill) / Labels (content/font/style/position)
//! / Placement (points + timeframe visibility) — in that fixed order,
//! filtered per primitive by [`available_tabs`] to only the ones that type
//! actually has anything for (a primitive with no level ladder never shows
//! Levels; one that declares nothing label-related and overrides
//! `Primitive::text_properties()` never shows Labels — see
//! `mlc_core::drawing::primitives_v2::config::PropertyCategory`'s own doc).
//! Placement is unconditional. Replaces the original five-tab set
//! (Style/Text/Coordinates/Levels/Visibility, retired 2026-08-08) and that
//! set's own three-tab successor (Вид/Уровни/Размещение, same day, Labels
//! not yet split out of Вид) — ported from the `mlc-ui-lab` "approved
//! design" prototype (owner direction, 2026-08-07, after live review) —
//! hard cutover each time, no compatibility variants kept alongside these.
//! Each tab's own row-drawing lives in its own sibling module
//! (`primitive_settings_vid`/`_labels`/`_levels`/`_placement`); this file
//! owns the modal shell (header/tabs/scroll/footer/resize), the strip's own
//! per-primitive membership ([`available_tabs`]) and the colour-picker
//! popup.

use mlc_core::engine::render::RenderContext;
use crate::ui::toolbar_theme::ToolbarTheme;
use uzor::types::Rect as WidgetRect;
use crate::ui::modal_settings::ResizableModal;
use crate::ui::modal_settings::{PrimitiveSettingsState, PrimitiveSettingsTab};
use crate::layout::render_frame::PrimitiveSettingsResult;
use crate::layout::toolbar_state::ToolbarState;
use crate::layout::render_chart::FrameTheme;
use crate::ui::widgets::{
    begin_modal_shell, draw_input, draw_input_cursor, end_modal_shell, labelled_tabs_width, InputConfig,
    InputType, ModalFooter, ModalPlacement, ModalShellSpec, ModalTab, ModalTabs, ModalTheme, TabFit,
    WidgetState, WidgetTheme,
};
use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig};
use crate::ui::widgets::SETTINGS_BUTTON_RADIUS;
use crate::ui::modal_settings::LevelSet;
use crate::layout::render_ui::toolbar_to_widget_theme;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::i18n::{current_language, ConfigKey, SettingsKey, t_settings};

use super::{
    primitive_settings_intent, primitive_settings_labels, primitive_settings_levels,
    primitive_settings_placement, primitive_settings_vid,
};

// =============================================================================
// Tab strip membership — the ONE place that decides which tabs exist for a
// given primitive (item 1, 2026-08-08 4-tab cutover).
// =============================================================================

/// The tabs `prim` actually has anything for, in the enum's own fixed strip
/// order (`PrimitiveSettingsTab::all()`) — never the order `prim` happened
/// to declare its own properties in. `Levels` appears only when the
/// primitive has a level ladder (the ladder itself forces it — a bespoke
/// collection, not a declared property, per `PropertyCategory`'s own doc);
/// `Appearance`/`Labels` appear only when that tab body's own `has_content`
/// says there is something to draw (a declared property in the matching
/// category, or — for `Appearance` — the bespoke base-stroke row a
/// level-less primitive always has, or — for `Labels` — the bespoke
/// content/style/position block a primitive without custom
/// `text_properties()` always has); `Placement` is unconditional — the
/// points table and timeframe chips are bespoke collections every
/// primitive has, not declared properties.
///
/// Each tab body (`primitive_settings_vid`/`_labels`/`_levels`/
/// `_placement`) owns its OWN internal grouping, but none of them decides
/// whether IT ITSELF is present in the strip — that membership decision
/// lives here, once, so the strip built for the header and the content
/// pane drawn below it can never disagree about which tabs exist. Never
/// empty: `Placement` always passes its own filter.
fn available_tabs(prim: &dyn Primitive) -> Vec<PrimitiveSettingsTab> {
    PrimitiveSettingsTab::all()
        .iter()
        .copied()
        .filter(|tab| match tab {
            PrimitiveSettingsTab::Levels => prim.level_configs().is_some(),
            PrimitiveSettingsTab::Appearance => primitive_settings_vid::has_content(prim),
            PrimitiveSettingsTab::Labels => primitive_settings_labels::has_content(prim),
            PrimitiveSettingsTab::Placement => true,
        })
        .collect()
}

/// `state.active_tab` if it names one of `available`, otherwise the first
/// available tab. Covers the case `PrimitiveSettingsState::active_tab`'s
/// own doc promises but cannot itself guarantee — `open()` only ever sees a
/// `primitive_id`, never the primitive, so it cannot know this primitive's
/// real tab set; this is the one place that DOES have `prim` in hand, so
/// the fallback happens here, every render, rather than trusting whatever
/// tab was active for a PREVIOUS primitive (or a still-open modal switched
/// to a differently-shaped one) to still be valid for THIS one. `available`
/// is never empty (`Placement` is unconditional), so this always resolves
/// to a real tab.
fn resolve_active_tab(state: &PrimitiveSettingsState, available: &[PrimitiveSettingsTab]) -> PrimitiveSettingsTab {
    if available.contains(&state.active_tab) {
        state.active_tab
    } else {
        available.first().copied().unwrap_or(PrimitiveSettingsTab::Placement)
    }
}

// =============================================================================
// Template menu — merged system-preset + user-template list (item 3, 2026-08-08 review)
// =============================================================================

/// One row in the merged Template menu list — either a canned system
/// level-set preset (not stored, not renameable, not deletable) or a
/// user-saved [`mlc_core::templates::PrimitiveTemplate`] (stored, its own
/// rename/delete row actions — see `draw_template_row` below).
enum TemplateListEntry<'a> {
    System(LevelSet),
    User(&'a mlc_core::templates::PrimitiveTemplate),
}

impl<'a> TemplateListEntry<'a> {
    /// This row's own position key — a system preset's `LevelSet::id()`, or
    /// a user template's own `id`. The SAME string [`merged_template_list`]
    /// reads back out of a persisted order, and `mlc-app`'s drag-to-reorder
    /// commit writes back in (via `result.template_menu_rows`) — there is
    /// only ever this one string identifying a row, never a positional
    /// index (a drag or an add/delete would silently repoint an index at
    /// the wrong row the next time the list is not in the order it was
    /// computed against).
    fn key(&self) -> &str {
        match self {
            TemplateListEntry::System(s) => s.id(),
            TemplateListEntry::User(t) => t.id.as_str(),
        }
    }
}

/// Merge the four system level-set presets with the user's saved primitive
/// templates into one ordered list — no divider between the two kinds
/// (item 3), drag-reorderable as one list (item 4).
///
/// `order` is the persisted position-key sequence
/// (`mlc-backend`'s `TemplateManager::primitive_template_order`, threaded in
/// from the caller as `template_order`): every key it names places that row
/// where `order` says. Anything `order` never named — a system preset that
/// has never been dragged, or a template created since the last reorder —
/// falls back to its own natural position, appended after everything
/// `order` DOES place: system presets first (their own canonical order),
/// then templates (their own list order). An empty `order` (nothing has
/// ever been dragged) is exactly that fallback in full — the pre-reorder
/// layout this feature replaces, so a fresh install renders identically to
/// before this existed.
fn merged_template_list<'a>(
    templates: &'a [mlc_core::templates::PrimitiveTemplate],
    order: &[String],
) -> Vec<TemplateListEntry<'a>> {
    let mut remaining_system: Vec<LevelSet> = LevelSet::PRESETS.to_vec();
    let mut remaining_user: Vec<&mlc_core::templates::PrimitiveTemplate> = templates.iter().collect();
    let mut out = Vec::with_capacity(order.len() + remaining_system.len() + remaining_user.len());

    for key in order {
        if let Some(pos) = remaining_system.iter().position(|s| s.id() == key.as_str()) {
            out.push(TemplateListEntry::System(remaining_system.remove(pos)));
            continue;
        }
        if let Some(pos) = remaining_user.iter().position(|t| &t.id == key) {
            out.push(TemplateListEntry::User(remaining_user.remove(pos)));
        }
        // A key naming neither (a template deleted since `order` was last
        // saved) is silently dropped — nothing left to place for it.
    }
    for s in remaining_system {
        out.push(TemplateListEntry::System(s));
    }
    for t in remaining_user {
        out.push(TemplateListEntry::User(t));
    }
    out
}

/// Height of a tab-name header in the cross-tab settings-search result view
/// (item 8, 2026-08-08 follow-up) — see [`draw_search_tab_header`].
const SEARCH_TAB_HEADER_H: f64 = 24.0;

/// Gap between two consecutive tab sections in the cross-tab search result
/// view — a little more breathing room than `GROUP_GAP` inside a tab (each
/// tab module's own constant), so a glance at the list tells a tab boundary
/// apart from an ordinary group boundary within one tab.
const SEARCH_TAB_GAP: f64 = 14.0;

/// Kept equal to `ScrollableConfig::default().scrollbar_width` — this is
/// the one place `content_right` (what every tab module lays its rows out
/// against) and the scrollbar strip `ScrollableContainer` actually reserves
/// are computed from the SAME number, so a row drawn flush to
/// `content_right` never ends up half under the scrollbar's clip.
const SCROLLBAR_WIDTH: f64 = 8.0;

/// Small breathing room between the last content column and the scrollbar
/// strip. Item 2 (2026-08-08 review): the scrollbar itself sits flush with
/// the modal's own right edge — `modal_right - SCROLLBAR_WIDTH` — and
/// `content_right` stops this much short of THAT, both derived from the
/// same `modal_right` expression so they cannot drift apart.
const CONTENT_SCROLLBAR_GAP: f64 = 6.0;

/// Widget id for the modal-wide settings search field. Moved into the
/// footer (item 8, 2026-08-08 review) — filters rows across all three
/// tabs, not only Вид, which is why the tab modules read it via
/// [`search_query`]/[`search_matches`] rather than owning a copy each.
/// `pub`, not `pub(crate)`: `mlc-app`'s own click dispatch
/// (`input/settings.rs`) matches against it directly to start editing.
pub const SEARCH_FIELD_ID: &str = "prim_settings_search";

/// Live settings-search query — the field's own live buffer while it is
/// focused, nothing otherwise. Not persisted once the field blurs: each tab
/// reads this straight from the in-progress edit buffer during render, the
/// same way any other in-progress edit already does, with no second copy
/// of the string kept anywhere.
pub(crate) fn search_query(state: &PrimitiveSettingsState) -> String {
    if state.active_field_id.as_deref() == Some(SEARCH_FIELD_ID) {
        state.active_render.text.to_lowercase()
    } else {
        String::new()
    }
}

/// Whether `query` matches any of `texts` (case-insensitive substring), or
/// `query` is empty — the shared predicate every tab's own row/group
/// filtering calls, so "no query" and "matched" are decided identically
/// everywhere.
pub(crate) fn search_matches(query: &str, texts: &[&str]) -> bool {
    query.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(query))
}

/// Query-aware body height for one tab, used by both [`search_results_height`]
/// and [`draw_search_results`] so the two can never disagree about which
/// tabs contribute or how much space they need.
fn tab_query_height(tab: PrimitiveSettingsTab, prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    match tab {
        PrimitiveSettingsTab::Levels => primitive_settings_levels::search_content_height(prim, state),
        PrimitiveSettingsTab::Appearance => primitive_settings_vid::content_height_for_query(prim, state),
        PrimitiveSettingsTab::Labels => primitive_settings_labels::content_height_for_query(prim, state),
        PrimitiveSettingsTab::Placement => primitive_settings_placement::content_height_for_query(prim, state),
    }
}

/// Body height for [`draw_search_results`] — the sum of every AVAILABLE
/// tab's own query-aware height ([`tab_query_height`]), each already zero
/// when that tab has nothing matching `state`'s live query, plus one
/// [`SEARCH_TAB_HEADER_H`] and one [`SEARCH_TAB_GAP`] for every tab that
/// DOES contribute — mirrors `draw_search_results`'s own bookkeeping so the
/// scroll container is sized to exactly what that function paints.
fn search_results_height(prim: &dyn Primitive, state: &PrimitiveSettingsState) -> f64 {
    let mut h = 0.0;
    let mut any = false;
    for tab in available_tabs(prim) {
        let tab_h = tab_query_height(tab, prim, state);
        if tab_h > 0.0 {
            if any { h += SEARCH_TAB_GAP; }
            h += SEARCH_TAB_HEADER_H + tab_h;
            any = true;
        }
    }
    h
}

/// Cross-tab settings-search result body (item 8, 2026-08-08 follow-up):
/// with a non-empty query the content area stops being "the active tab" and
/// becomes a single list of every matching setting from all AVAILABLE tabs
/// ([`available_tabs`]), in tab order, each introduced by a header naming
/// that tab — nothing else is drawn (no palette cards / global
/// fill-opacity row / add-level row on Levels, no group on
/// Appearance/Labels/Placement whose every row was filtered out; see each
/// tab's own `content_height_for_query`/`search_content_height` doc
/// comment). Reuses each tab's own row drawing verbatim
/// (`primitive_settings_vid::draw`, `primitive_settings_labels::draw`,
/// `primitive_settings_levels::draw_search`,
/// `primitive_settings_placement::draw`) — the per-row `search_matches`
/// gates inside them are the SAME ones the single-tab view already used,
/// not a second matching rule. Rows land in `result.content_items` under
/// their own real ids exactly as they would in their own tab, so a control
/// found through search is fully functional where it is found.
#[allow(clippy::too_many_arguments)]
fn draw_search_results(
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
    let mut any = false;
    for tab in available_tabs(prim) {
        if tab_query_height(tab, prim, state) <= 0.0 {
            continue;
        }
        if any { *row_y += SEARCH_TAB_GAP; }
        draw_search_tab_header(ctx, toolbar_theme, x0, x1, *row_y, tab.label());
        *row_y += SEARCH_TAB_HEADER_H;
        match tab {
            PrimitiveSettingsTab::Levels => {
                primitive_settings_levels::draw_search(ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, prim);
            }
            PrimitiveSettingsTab::Appearance => {
                primitive_settings_vid::draw(ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, prim);
            }
            PrimitiveSettingsTab::Labels => {
                primitive_settings_labels::draw(ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, prim);
            }
            PrimitiveSettingsTab::Placement => {
                primitive_settings_placement::draw(ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, prim);
            }
        }
        any = true;
    }
}

/// Paints one tab-name header in the cross-tab search result view — plain
/// chrome, not registered as a `content_items` entry (no click/hover sense
/// needed for a label), so it is not counted by any of the tabs' own
/// "did I draw anything" bool returns and does not participate in hit
/// testing; it is still clipped and scrolled correctly because it is
/// painted between the same `scrollable.begin()`/`end()` calls as every
/// other body row.
fn draw_search_tab_header(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x0: f64, x1: f64, y: f64, label: &str) {
    use mlc_core::render::{TextAlign, TextBaseline};
    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rect(x0, y, (x1 - x0).max(0.0), SEARCH_TAB_HEADER_H);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.accent);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&label.to_uppercase(), x0, y + SEARCH_TAB_HEADER_H / 2.0 + 2.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x0, y + SEARCH_TAB_HEADER_H - 2.0);
    ctx.line_to(x1, y + SEARCH_TAB_HEADER_H - 2.0);
    ctx.stroke();
}

/// Render primitive settings modal.
///
/// This is a floating, non-overlay modal that appears near the primitive.
/// No background dimming - user can still interact with chart.
///
/// `screen_w` and `screen_h` are the total available screen dimensions
/// (used for centering and clamping the modal position).
/// `default_modal_y` is the Y offset from the top for default positioning.
/// `template_order` is the Template menu's persisted merged display order
/// (`mlc-backend`'s `TemplateManager::primitive_template_order`) — see
/// [`merged_template_list`] for how it is applied.
pub fn render_primitive_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    default_modal_y: f64,
    state: &PrimitiveSettingsState,
    primitives: &[Box<dyn mlc_core::drawing::primitives_v2::Primitive>],
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    _toolbar_state: &ToolbarState,
    _current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    templates: &[mlc_core::templates::PrimitiveTemplate],
    template_order: &[String],
) -> PrimitiveSettingsResult {
    let mut result = PrimitiveSettingsResult::default();

    // Get primitive info
    let Some(prim_id) = state.primitive_id else {
        return result;
    };
    let Some(prim) = primitives.iter().find(|p| p.data().id == prim_id) else {
        return result;
    };
    let prim = prim.as_ref();

    let prim_name = prim.display_name();

    // Layout constants. Header, tab-bar and footer heights are the shell's —
    // this modal no longer has an opinion about them.
    let modal_padding = 12.0;
    let close_btn_size = 20.0;
    let title_close_gap = 12.0;
    // Only feeds the MINIMUM modal-width floor below (dominated by the
    // `.max(400.0)` a few lines down either way) — no tab lays a fixed
    // label column out against it any more.
    let label_width = 170.0;

    // The tabs THIS primitive actually has (item 1, 2026-08-08 4-tab
    // cutover) — never the fixed 4, and `resolved_tab` is what `state.
    // active_tab` falls back to the instant it names a tab not in this set
    // (switched from a differently-shaped primitive, or a stale default).
    // Both the header strip and every match on "the active tab" below read
    // off THESE two, never `state.active_tab` directly, so they can never
    // disagree about what is showing.
    let available_tab_list = available_tabs(prim);
    let resolved_tab = resolve_active_tab(state, &available_tab_list);

    let tab_items: Vec<ModalTab<'_>> = available_tab_list.iter().map(|t| ModalTab::new(t.id(), t.label())).collect();
    let active_tab_idx = available_tab_list.iter().position(|t| *t == resolved_tab).unwrap_or(0);

    // Cross-tab settings search (item 8, 2026-08-08 follow-up): a non-empty
    // query replaces "the active tab" with a combined search-result body
    // spanning every available tab — see `draw_search_results`/
    // `search_results_height` below.
    let query = search_query(state);

    // True height the active tab's rows need — padding excluded (the shell's
    // `content_height` is body-only, see `ModalShellSpec::content_height`
    // doc). NOT clamped: the shell's own `max_content_height` below is the
    // ceiling; this is the real number, so `ScrollableContainer` can report
    // the true overflow instead of silently discarding it.
    let content_height = if query.is_empty() {
        match resolved_tab {
            PrimitiveSettingsTab::Levels => primitive_settings_levels::content_height(prim, state),
            PrimitiveSettingsTab::Appearance => primitive_settings_vid::content_height(prim),
            PrimitiveSettingsTab::Labels => primitive_settings_labels::content_height(prim),
            PrimitiveSettingsTab::Placement => primitive_settings_placement::content_height(prim),
        }
    } else {
        search_results_height(prim, state)
    };

    // Ceiling on the body. A short tab still sizes the frame to its
    // content; a long one (the ladder with many levels, or Вид with every
    // group expanded) stops growing here and scrolls the rest.
    let max_content_height = (screen_h - 250.0).clamp(200.0, 460.0);

    // The shell lays the tab strip out; this modal only needs to know how wide
    // the strip will be, so it asks the same code rather than measuring twice.
    let tabs_row_width = labelled_tabs_width(ctx, &tab_items);

    // Calculate title width
    ctx.set_font("13px sans-serif");
    let title_text = prim_name.to_string();
    let title_width = ctx.measure_text(&title_text);

    // Modal width. The floor was 400 — visibly cramped the first time it
    // opens, before any manual resize (item 9, 2026-08-08 review): raised
    // to give the reflowed rows (item 9's other half, see the tab bodies'
    // own `x1`-derived control columns) room to actually show it.
    let min_header_width = title_width + title_close_gap + close_btn_size + modal_padding * 2.0;
    let min_content_width = label_width + 150.0 + modal_padding * 2.0;
    let modal_width = min_header_width.max(tabs_row_width).max(min_content_width).max(560.0);

    let modal_theme = ModalTheme::from_toolbar(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let spec = ModalShellSpec {
        // The body's own padding is separate from the chrome's own constants
        // (`FOOTER_PAD` etc.) — this is only the body's inset.
        padding: modal_padding,
        placement: ModalPlacement::At(Some(state.position.unwrap_or((
            (screen_w - modal_width) / 2.0,
            default_modal_y,
        )))),
        ..ModalShellSpec::new(
            "prim_settings",
            &title_text,
            modal_width,
            content_height,
        )
        .with_max_content_height(max_content_height)
        // The layer keeps its old name: it is the key
        // `close_topmost_known_modal_layer` matches on.
        .layer_named("primitive_settings")
        .with_tabs(ModalTabs::Horizontal {
            items: &tab_items,
            active: active_tab_idx,
            // Delta 2 (2026-08-08 port): tabs stretched to equal shares of
            // the modal width, not packed to their own label widths from
            // the left — still true with a variable tab COUNT (1-4) since
            // the 4-tab cutover.
            fit: TabFit::Equal,
        })
        .with_footer(ModalFooter::Editor {
            template: Some(t_settings(SettingsKey::ButtonTemplate)),
        })
        .resizable(state.size, state.min_size())
    };
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h));

    let modal_x = shell.frame.x;
    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.tab_rects = shell.tab_rects.clone();
    result.total_content_height = content_height;
    result.viewport_height = shell.content.height;

    // === CONTENT AREA ===
    // `shell.content` is the VIEWPORT the body draws into — not the height it
    // asked for (`content_height`). When they differ the body scrolls, via the
    // `ScrollableContainer` set up below.
    result.content_rect = WidgetRect::new(modal_x, shell.content.y, shell.frame.width, shell.content.height);

    // Delta 3 (2026-08-08 port), corrected item 2 (2026-08-08 review): the
    // scrollbar now sits flush with the modal's own right edge —
    // `modal_right - SCROLLBAR_WIDTH`, no `modal_padding` gap after it —
    // and `content_right` stops `CONTENT_SCROLLBAR_GAP` short of THAT.
    // Both read off `modal_right`, so they cannot drift apart.
    //
    // `ScrollableContainer` derives its own scrollbar track x from
    // `viewport.x + viewport.width - scrollbar_width`; feeding it
    // `shell.content` directly lands the bar `modal_padding` short of the
    // edge (the body viewport is inset by the body's own padding on every
    // side). `scroll_viewport` widens the RIGHT edge only, out to
    // `modal_right`, so the container's own arithmetic lands the bar in
    // the right place without a second copy of it here.
    // From the SHELL's frame, never from `modal_width`. `modal_width` is the
    // intrinsic width this modal asks for; `.resizable(state.size, ..)` lets
    // the user override it, and the shell returns the width it actually drew.
    // Reading the intrinsic one here laid every row out at the pre-resize
    // width while the frame around them grew — content stranded in the left
    // half with the old content edge still visible as a hairline.
    let modal_right = shell.frame.x + shell.frame.width;
    let content_left = modal_x + modal_padding;
    let content_right = modal_right - SCROLLBAR_WIDTH - CONTENT_SCROLLBAR_GAP;
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let scroll_config = ScrollableConfig { scrollbar_width: SCROLLBAR_WIDTH, ..ScrollableConfig::default() };
    let scroll_viewport = WidgetRect::new(shell.content.x, shell.content.y, modal_right - shell.content.x, shell.content.height);
    let scrollable = ScrollableContainer::new(scroll_viewport, &state.scroll, Some(scroll_config));
    scrollable.begin(ctx);
    let mut row_y = scrollable.content_y();

    if query.is_empty() {
        match resolved_tab {
            PrimitiveSettingsTab::Levels => {
                primitive_settings_levels::draw(ctx, toolbar_theme, &widget_theme, state, &mut result, content_left, content_right, &mut row_y, prim, shell.content.bottom());
            }
            PrimitiveSettingsTab::Appearance => {
                primitive_settings_vid::draw(ctx, toolbar_theme, &widget_theme, state, &mut result, content_left, content_right, &mut row_y, prim);
            }
            PrimitiveSettingsTab::Labels => {
                primitive_settings_labels::draw(ctx, toolbar_theme, &widget_theme, state, &mut result, content_left, content_right, &mut row_y, prim);
            }
            PrimitiveSettingsTab::Placement => {
                primitive_settings_placement::draw(ctx, toolbar_theme, &widget_theme, state, &mut result, content_left, content_right, &mut row_y, prim);
            }
        }
    } else {
        draw_search_results(ctx, toolbar_theme, &widget_theme, state, &mut result, content_left, content_right, &mut row_y, prim);
    }

    // Everything the tab bodies pushed lives inside the scrolling viewport.
    // Anything appended after this index (the template menu, the footer
    // search field) sits OUTSIDE it and must not be culled against it.
    let body_items_end = result.content_items.len();

    // Closes the clip opened by `scrollable.begin()` above and draws the
    // scrollbar.
    //
    // The clip governs PAINT only — the rects the match above pushed are
    // registered at their raw coordinates, including rows scrolled past the
    // bottom of the viewport and therefore lying under the footer. Relying
    // on "the footer registers last and last-registered wins" only ever
    // covered clicks that land exactly on Template/OK; hover, and any click
    // in the empty footer band between the buttons, still reached the row
    // hidden underneath. So body items are culled against the viewport at
    // registration time below.
    let scroll_result = scrollable.end(ctx, content_height, &widget_theme);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;

    // Route mouse-wheel scroll here via InputCoordinator's scroll-sense pass
    // (`process_scroll` in the app's wheel handler).
    input_coordinator.register_on_layer(
        "prim_settings:scroll_viewport",
        uzor::types::Rect::new(shell.content.x, shell.content.y, shell.content.width, shell.content.height),
        uzor::input::Sense::SCROLL,
        &layer_id,
    );

    // Register scrollbar handle (inflated ±5px X for an easier grab) and
    // track, mirroring the indicator-settings modal's pattern exactly — the
    // ":scrollbar_handle" / ":scrollbar_track" id suffixes are what the
    // app's drag-start/track-click dispatch matches on.
    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "prim_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            &layer_id,
        );
    }
    if let Some(ref track_rect) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "prim_settings:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            uzor::input::Sense::CLICK,
            &layer_id,
        );
    }

    // === LEVELS TAB: "+ добавить уровень" quick-value dropdown ===
    // Painted here — after `scrollable.end()` above closed the ladder's own
    // scroll clip, and after `body_items_end` was captured — so it is
    // neither clipped by the ladder's viewport nor culled against it at
    // registration time below (mirrors `template_dropdown_menu`'s own
    // outside-the-clip treatment, a few lines down). `open_upward` was
    // already decided by `primitive_settings_levels::draw_add_row`, from
    // the space between the input and this same viewport's bottom edge —
    // the only place that knows it (item 2 follow-up, 2026-08-08).
    if let Some((field_rect, open_upward)) = result.add_chip_dropdown_anchor {
        primitive_settings_levels::draw_add_chip_dropdown(ctx, toolbar_theme, &mut result, field_rect, content_right, open_upward);
    }

    // === Split cycle+chevron control's dropdown (item 1, 2026-08-09
    // restore) ===
    // Painted here for the same reason `draw_add_chip_dropdown` above is:
    // after `scrollable.end()` closed whichever tab's own scroll clip, and
    // after `body_items_end` was captured, so it is neither clipped by that
    // tab's own viewport nor culled against it at registration time below.
    // `.take()`, not a `Copy` read: `ValueDropdownAnchor` owns a `String`
    // and (for a `Select` row) a `Vec`, so this also leaves
    // `result.value_dropdown_anchor` `None` for the next frame's own fresh
    // read — mirrors `add_chip_dropdown_anchor` being recomputed every
    // frame instead of persisted.
    if let Some(anchor) = result.value_dropdown_anchor.take() {
        primitive_settings_vid::draw_value_dropdown(ctx, toolbar_theme, &mut result, &anchor);
    }

    // === TEMPLATE FOOTER ===
    // Single 52px row: "Шаблон" outline button left, "Отмена" + "OK" right.
    {
        use mlc_core::render::{TextAlign, TextBaseline};
        use mlc_core::engine::render::draw_svg_icon;

        // The footer bar itself — separator, Template button, OK — belongs to
        // the shell now. What is left here is the menu that hangs off the
        // Template button, placed from the rect the shell reported rather than
        // from a second copy of the footer arithmetic.
        if let (Some(template_btn), true) = (shell.template_btn, state.template_dropdown_open) {
            let button_y = template_btn.y;
            let button_height = template_btn.height;
            let template_btn_width = template_btn.width;
            let template_btn_x = template_btn.x;
            let opt_h = 28.0;
            let sep_h = 1.0;
            let action_btn_w = 24.0;
            let action_gap = 2.0;

            // Item 3 (2026-08-08 review): system level-set presets show
            // whenever the PRIMITIVE itself has levels to offer a set for
            // — not gated to the Levels tab happening to be the active one
            // right now. The Template menu names one control for the whole
            // primitive; switching tabs must not hide the choice.
            let show_level_sets = prim.level_configs().is_some();

            // Item 3: system presets + user templates, one merged,
            // drag-reordered list — no divider between the two kinds.
            let all_entries = merged_template_list(templates, template_order);
            let entries: Vec<TemplateListEntry> = if show_level_sets {
                all_entries
            } else {
                all_entries.into_iter().filter(|e| matches!(e, TemplateListEntry::User(_))).collect()
            };

            // "Save as…" is an action, and it only exists when there is
            // something NEW to save: the live settings have drifted from
            // the REFERENCE the primitive itself persistently claims —
            // `PrimitiveData::style_reference` (a template id or a system
            // level-set preset id, `#[serde(default)]` so it survives
            // reload with the drawing). Three rules have been tried here
            // and rejected before this one, all for the same underlying
            // reason: they read from something that does not survive a
            // reload — `applied_template_id` (modal-only state, showed the
            // action before the user touched a pixel), "matches no row in
            // the merged list" (a SYSTEM row only ever compared level
            // COMPOSITION, so recolouring a level, changing its width/
            // style, toggling its band, or flipping timeframe visibility
            // all left the primitive still reading as "Standard"), and a
            // session-only `settings_baseline` snapshot captured fresh
            // every time the modal opened — which on reload re-captured
            // the ALREADY-DRIFTED loaded state as the new baseline,
            // silently erasing the drift the owner had specifically
            // reported. Resolving a persisted reference by ID, at the
            // moment it is needed, is what survives all three failure
            // modes: a template resolves against the live template store
            // (so an edit to the template itself is also visible here); a
            // level-set resolves against the preset's own fixed
            // definition (`primitive_settings_intent::resolve_level_set_
            // reference`). `None` — no reference has ever been established
            // for this primitive, OR the primitive's own creation-time
            // reference (`DrawingManager::initial_style_reference`, in
            // `mlc-backend`) found nothing nameable to stamp (a level
            // composition matching no system preset, or a primitive with
            // no level ladder at all, seeded only from the bubble's own
            // un-id'd last-used-style snapshot) — SHOWS the action rather
            // than hiding it (2026-08-09 fix): a state that matches
            // nothing on record is exactly a state worth offering to save,
            // not one that should hide the offer. A reference naming
            // something that no longer exists (a deleted template) reads
            // as drift and shows the action too, the same as any other
            // mismatch.
            let show_save_as = match &prim.data().style_reference {
                None => true,
                Some(mlc_core::drawing::StyleReference::Template(tmpl_id)) => {
                    match templates.iter().find(|t| &t.id == tmpl_id) {
                        Some(reference) => !reference.matches_primitive(prim),
                        None => true,
                    }
                }
                Some(mlc_core::drawing::StyleReference::LevelSet(set_id)) => {
                    match primitive_settings_intent::resolve_level_set_reference(set_id, prim) {
                        Some(reference) => !reference.matches_primitive(prim),
                        None => true,
                    }
                }
            };

            let save_as_h = if show_save_as { opt_h + sep_h + 2.0 } else { 0.0 };
            let total_h = save_as_h + entries.len() as f64 * opt_h + 6.0;
            let menu_w = template_btn_width.max(200.0);
            let dd_x = template_btn_x;
            let menu_y = button_y + button_height + 2.0;

            // Menu background + border
            ctx.set_fill_color(&toolbar_theme.dropdown_bg);
            ctx.fill_rounded_rect(dd_x, menu_y, menu_w, total_h, SETTINGS_BUTTON_RADIUS);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(dd_x, menu_y, menu_w, total_h, SETTINGS_BUTTON_RADIUS);

            // Register background FIRST with coordinator (last-registered wins, so
            // items registered AFTER will override this in hit_test_at)
            input_coordinator.register_on_layer(
                "prim_settings:item:template_dropdown_menu".to_string(),
                uzor::types::Rect::new(dd_x, menu_y, menu_w, total_h),
                uzor::input::Sense::CLICK,
                &layer_id,
            );

            let mut row_y = menu_y + 3.0;

            // "Save as…" — its own top row, own colour, present only while
            // the current settings differ from the applied template (item
            // 5). Not part of the merged list below: it is an action, not
            // a row that could ever be dragged or carry a check mark.
            if show_save_as {
                let is_sa_hovered = state.hovered_item_id.as_deref() == Some("template_save_as");
                if is_sa_hovered {
                    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                    ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
                }
                // Same weight as every other row in the menu — it was
                // painted in the accent colour, which read as "selected"
                // on a row that is an action and can never be current.
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(t_settings(SettingsKey::SaveAsTemplate), dd_x + 8.0, row_y + opt_h / 2.0);
                result.content_items.push(("template_save_as".to_string(), WidgetRect::new(dd_x, row_y, menu_w, opt_h)));
                row_y += opt_h;

                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(dd_x + 4.0, row_y);
                ctx.line_to(dd_x + menu_w - 4.0, row_y);
                ctx.stroke();
                row_y += sep_h + 2.0;
            }

            // The merged system-preset + user-template list (items 3/4).
            // `result.template_menu_rows` records every row's own key and
            // on-screen rect, in THIS exact paint order — `mlc-app`'s own
            // drag-to-reorder reads it back to compute the drop index and
            // the reorder commit, so the two can never disagree about what
            // order the list is actually in.
            for entry in &entries {
                let key = entry.key().to_string();
                let row_rect = WidgetRect::new(dd_x, row_y, menu_w, opt_h);
                result.template_menu_rows.push((key, row_rect));

                match entry {
                    TemplateListEntry::System(preset) => {
                        let row_id = format!("levels_set_option:{}", preset.id());
                        let is_current = state.level_set == *preset;
                        let is_hovered = state.hovered_item_id.as_deref() == Some(row_id.as_str());
                        if is_current || is_hovered {
                            ctx.set_fill_color(if is_current { &toolbar_theme.item_bg_active } else { &toolbar_theme.item_bg_hover });
                            ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
                        }
                        ctx.set_fill_color(if is_current { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
                        ctx.set_font("11px sans-serif");
                        ctx.set_text_align(TextAlign::Left);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.fill_text(preset.label(), dd_x + 8.0, row_y + opt_h / 2.0);
                        result.content_items.push((row_id, row_rect));
                    }
                    TemplateListEntry::User(tmpl) => {
                        let row_id = format!("template_option:{}", tmpl.id);
                        let rename_id = format!("template_rename:{}", tmpl.id);
                        let del_id = format!("template_delete:{}", tmpl.id);
                        let editing_field = format!("template_name:{}", tmpl.id);
                        let is_editing = state.active_field_id.as_deref() == Some(editing_field.as_str());
                        // The currently-applied entry's own mark (item 3):
                        // the same highlight a system row's own `is_current`
                        // already draws with, so a glance at either kind
                        // reads identically.
                        let is_current = state.applied_template_id.as_deref() == Some(tmpl.id.as_str());
                        let is_row_hovered = state.hovered_item_id.as_deref() == Some(row_id.as_str());
                        let is_rename_hovered = state.hovered_item_id.as_deref() == Some(rename_id.as_str());
                        let is_del_hovered = state.hovered_item_id.as_deref() == Some(del_id.as_str());

                        if is_current || is_row_hovered || is_rename_hovered || is_del_hovered {
                            ctx.set_fill_color(if is_current { &toolbar_theme.item_bg_active } else { &toolbar_theme.item_bg_hover });
                            ctx.fill_rounded_rect(dd_x + 1.0, row_y, menu_w - 2.0, opt_h, 3.0);
                        }

                        let icons_w = 2.0 * action_btn_w + action_gap;
                        let name_w = (menu_w - icons_w - 12.0).max(20.0);

                        if is_editing {
                            // Rename in place, in the row (item 4) — the
                            // same shared inline-edit buffer
                            // (`state.active_render`) every other sub-field
                            // in this modal already uses, focused by the
                            // pencil click below via `start_prim_field_editing`.
                            let ar = &state.active_render;
                            let field_rect = WidgetRect::new(dd_x + 8.0, row_y + 3.0, name_w, opt_h - 6.0);
                            let cfg = InputConfig::new(ar.text.as_str())
                                .with_focused(true)
                                .with_type(InputType::Text)
                                .with_font_size(11.0)
                                .with_padding(4.0)
                                .with_radius(3.0)
                                .with_cursor(ar.cursor)
                                .with_selection(ar.selection_start, Some(ar.cursor));
                            let res = draw_input(ctx, &cfg, WidgetState::Normal, field_rect, &widget_theme);
                            draw_input_cursor(ctx, res.cursor_x, res.cursor_y, res.cursor_height, &toolbar_theme.item_text);
                            result.active_input_char_positions = res.char_x_positions;
                            result.active_input_rect = Some(field_rect);
                            result.content_items.push((editing_field, field_rect));
                        } else {
                            ctx.set_fill_color(if is_current { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
                            ctx.set_font("11px sans-serif");
                            ctx.set_text_align(TextAlign::Left);
                            ctx.set_text_baseline(TextBaseline::Middle);
                            ctx.save();
                            ctx.begin_path();
                            ctx.rect(dd_x + 8.0, row_y, name_w, opt_h);
                            ctx.clip();
                            ctx.fill_text(&tmpl.name, dd_x + 8.0, row_y + opt_h / 2.0);
                            ctx.restore();
                            result.content_items.push((row_id, WidgetRect::new(dd_x, row_y, name_w + 8.0, opt_h)));
                        }

                        // System rows get neither action (item 4) — only a
                        // user row draws these two.
                        let icon_y = row_y + (opt_h - action_btn_w) / 2.0;
                        let del_x = dd_x + menu_w - action_btn_w - 4.0;
                        let rename_x = del_x - action_gap - action_btn_w;

                        let rename_color = if is_rename_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
                        draw_svg_icon(ctx, crate::ui::Icon::Pencil.svg(), rename_x, icon_y, action_btn_w, action_btn_w, rename_color);
                        result.content_items.push((rename_id, WidgetRect::new(rename_x, icon_y, action_btn_w, action_btn_w)));

                        // Trash idiom, not a bare "×" (item 4).
                        let del_color = if is_del_hovered { &toolbar_theme.danger } else { &toolbar_theme.item_text_muted };
                        draw_svg_icon(ctx, crate::ui::Icon::Delete.svg(), del_x, icon_y, action_btn_w, action_btn_w, del_color);
                        result.content_items.push((del_id, WidgetRect::new(del_x, icon_y, action_btn_w, action_btn_w)));
                    }
                }
                row_y += opt_h;
            }

            // Register dropdown background LAST so specific items win in hit-test
            result.content_items.push(("template_dropdown_menu".to_string(), WidgetRect::new(dd_x, menu_y, menu_w, total_h)));
        }
    }

    // Draw hover highlight for hovered content item (skip non-interactive
    // background rects). Painted at `SETTINGS_BUTTON_RADIUS` — the SAME
    // constant every button shape in this modal now draws its own rounded
    // rect with (item 1, 2026-08-08 review) — so the highlight can never
    // mismatch the shape underneath it again.
    if let Some(ref hovered_id) = state.hovered_item_id {
        if hovered_id != "template_dropdown_menu" && hovered_id != SEARCH_FIELD_ID {
            for (id, rect) in &result.content_items {
                if id == hovered_id {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.1));
                    ctx.fill_rounded_rect(rect.x - 2.0, rect.y - 2.0, rect.width + 4.0, rect.height + 4.0, SETTINGS_BUTTON_RADIUS);
                    break;
                }
            }
        }
    }

    // === FOOTER SEARCH FIELD (item 8, 2026-08-08 review) ===
    // Horizontally centred between Template and OK, filters all three tabs
    // (`search_query`/`search_matches` above). Registered here so the
    // generic loop below picks it up like any other content item; its
    // VISUALS are painted after `end_modal_shell` further down — this
    // modal's footer background is drawn there, and painting the field
    // before it would just get covered.
    if let (Some(template_btn), Some(primary_btn)) = (shell.template_btn, shell.primary_btn) {
        let gap = 8.0;
        let search_x = template_btn.right() + gap;
        let search_w = (primary_btn.x - gap - search_x).max(40.0);
        let search_rect = WidgetRect::new(search_x, template_btn.y, search_w, template_btn.height);
        result.content_items.push((SEARCH_FIELD_ID.to_string(), search_rect));
    }

    // Build set of slider field IDs to determine which items need DRAG sense
    let slider_field_ids: std::collections::HashSet<&str> = result.slider_tracks
        .iter()
        .map(|s| s.field_id.as_str())
        .collect();

    // Register all content items with coordinator (skip template_dropdown_menu —
    // already registered early so it has lowest priority in hit_test_at).
    for (idx, (item_id, item_rect)) in result.content_items.iter().enumerate() {
        if item_id == "template_dropdown_menu" {
            continue;
        }
        // Body items are confined to the scroll viewport: a row that scrolled
        // out of sight must stop taking hover and clicks, and a row only half
        // past the bottom edge keeps exactly the half still visible. Items
        // appended after the body (template menu, footer search) are not
        // inside the viewport and are registered whole.
        let item_rect = if idx < body_items_end {
            let top = item_rect.y.max(scroll_viewport.y);
            let bottom = (item_rect.y + item_rect.height).min(scroll_viewport.y + scroll_viewport.height);
            if bottom <= top {
                continue;
            }
            WidgetRect::new(item_rect.x, top, item_rect.width, bottom - top)
        } else {
            *item_rect
        };
        let item_rect = &item_rect;
        let sense = if item_id == "levels_global_opacity" {
            // Item 5 (2026-08-08 review): a plain click on this slider's
            // track never reached the drag pipeline under DRAG-only sense.
            // Mirrors `compare_settings.rs`'s own
            // `cmp_settings:line_width_slider` — the modal-settings
            // family's precedent for "a slider that must respond to a bare
            // click" — which registers CLICK alongside DRAG.
            // `handle_prim_settings_item` (`mlc-app`'s `input/settings.rs`)
            // also applies the value directly on click now, a second,
            // deterministic path that does not depend on drag-start/
            // drag-end timing.
            uzor::input::Sense::CLICK | uzor::input::Sense::DRAG | uzor::input::Sense::SCROLL
        } else if slider_field_ids.contains(item_id.as_str()) {
            uzor::input::Sense::DRAG | uzor::input::Sense::SCROLL
        } else {
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER
        };

        input_coordinator.register_on_layer(
            format!("prim_settings:item:{}", item_id),
            uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            sense,
            &layer_id,
        );
    }

    // The footer goes in after the body — see `end_modal_shell`.
    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id);

    // Paint the footer search field's own visuals LAST — after the
    // footer's own background fill above, so they are not painted over by
    // it. Its widget was already registered by the generic `content_items`
    // loop; this only draws (item 8, 2026-08-08 review).
    if let Some(search_rect) = result.content_items.iter().find(|(id, _)| id == SEARCH_FIELD_ID).map(|(_, r)| *r) {
        use mlc_core::render::{TextAlign, TextBaseline};
        let is_editing = state.active_field_id.as_deref() == Some(SEARCH_FIELD_ID);
        ctx.set_fill_color(&toolbar_theme.background);
        ctx.fill_rounded_rect(search_rect.x, search_rect.y, search_rect.width, search_rect.height, SETTINGS_BUTTON_RADIUS);
        ctx.set_stroke_color(if is_editing { &toolbar_theme.accent } else { &toolbar_theme.separator });
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(search_rect.x, search_rect.y, search_rect.width, search_rect.height, SETTINGS_BUTTON_RADIUS);
        ctx.set_font("11.5px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.save();
        ctx.begin_path();
        ctx.rect(search_rect.x, search_rect.y, search_rect.width - 6.0, search_rect.height);
        ctx.clip();
        if is_editing {
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.fill_text(&state.active_render.text, search_rect.x + 9.0, search_rect.center_y());
            result.active_input_rect = Some(search_rect);
        } else {
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.fill_text(ConfigKey::SearchSettingsPlaceholder.get(current_language()), search_rect.x + 9.0, search_rect.center_y());
        }
        ctx.restore();
    }

    result
}

/// Render color picker popup for primitive settings (L1 or L2 based on state)
pub fn render_primitive_color_picker_popup(
    ctx: &mut dyn RenderContext,
    primitive_state: &PrimitiveSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> crate::layout::render_frame::ColorPickerRenderResult {
    use crate::ui::widgets::{
        draw_color_picker_l1, draw_color_picker_l2,
        ColorPickerLevel, PopupTheme,
    };

    let popup_theme = PopupTheme::new(&toolbar_theme.background, &toolbar_theme.separator)
        .with_active(&toolbar_theme.item_bg_active);
    let origin = primitive_state.color_picker.origin;
    let level = primitive_state.color_picker.level;

    let result = match level {
        ColorPickerLevel::L1 => {
            let config = primitive_state.color_picker.l1_config();
            let hovered = primitive_state.color_picker.hovered_swatch_str();
            let l1_result = draw_color_picker_l1(ctx, &config, origin, &popup_theme, hovered);
            crate::layout::render_frame::ColorPickerRenderResult {
                level,
                l1_result: Some(l1_result),
                l2_result: None,
            }
        }
        ColorPickerLevel::L2 => {
            let config = primitive_state.color_picker.l2_config();
            let hovered_area = primitive_state.color_picker.hovered_area;
            let l2_result = draw_color_picker_l2(ctx, &config, origin, &popup_theme, hovered_area);
            crate::layout::render_frame::ColorPickerRenderResult {
                level,
                l1_result: None,
                l2_result: Some(l2_result),
            }
        }
        ColorPickerLevel::Closed => {
            crate::layout::render_frame::ColorPickerRenderResult {
                level,
                l1_result: None,
                l2_result: None,
            }
        }
    };

    use uzor::{Rect, input::Sense};
    use crate::ui::z_order::ZLayer;

    let layer_id = ZLayer::ColorPicker.push_named(input_coordinator, "color_picker_primitive");

    if let Some(ref l1) = result.l1_result {
        let popup_rect = &l1.popup_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:popup",
            Rect { x: popup_rect.x, y: popup_rect.y, width: popup_rect.width, height: popup_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        for (idx, (_, swatch_rect)) in l1.swatch_rects.iter().enumerate() {
            input_coordinator.register_on_layer(
                format!("color_picker_primitive:swatch:{}", idx),
                Rect { x: swatch_rect.x, y: swatch_rect.y, width: swatch_rect.width, height: swatch_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }

        if let Some(ref plus_rect) = l1.plus_button_rect {
            input_coordinator.register_on_layer(
                "color_picker_primitive:plus",
                Rect { x: plus_rect.x, y: plus_rect.y, width: plus_rect.width, height: plus_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }

        if let Some(ref slider_rect) = l1.opacity_slider_rect {
            input_coordinator.register_on_layer(
                "color_picker_primitive:opacity_slider",
                Rect { x: slider_rect.x, y: slider_rect.y, width: slider_rect.width, height: slider_rect.height },
                Sense::DRAG | Sense::SCROLL,
                &layer_id,
            );
        }

        if let Some(ref toggle_rect) = l1.opacity_toggle_rect {
            input_coordinator.register_on_layer(
                "color_picker_primitive:opacity_toggle",
                Rect { x: toggle_rect.x, y: toggle_rect.y, width: toggle_rect.width, height: toggle_rect.height },
                Sense::CLICK,
                &layer_id,
            );
        }
    } else if let Some(ref l2) = result.l2_result {
        let popup_rect = &l2.popup_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:popup",
            Rect { x: popup_rect.x, y: popup_rect.y, width: popup_rect.width, height: popup_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let sv_rect = &l2.sv_square_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:sv_square",
            Rect { x: sv_rect.x, y: sv_rect.y, width: sv_rect.width, height: sv_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let hue_rect = &l2.hue_bar_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:hue_bar",
            Rect { x: hue_rect.x, y: hue_rect.y, width: hue_rect.width, height: hue_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let hex_rect = &l2.hex_input_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:hex_input",
            Rect { x: hex_rect.x, y: hex_rect.y, width: hex_rect.width, height: hex_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let slider_rect = &l2.opacity_slider_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:opacity_slider",
            Rect { x: slider_rect.x, y: slider_rect.y, width: slider_rect.width, height: slider_rect.height },
            Sense::DRAG,
            &layer_id,
        );

        let toggle_rect = &l2.opacity_toggle_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:opacity_toggle",
            Rect { x: toggle_rect.x, y: toggle_rect.y, width: toggle_rect.width, height: toggle_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let add_rect = &l2.add_button_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:add",
            Rect { x: add_rect.x, y: add_rect.y, width: add_rect.width, height: add_rect.height },
            Sense::CLICK,
            &layer_id,
        );

        let back_rect = &l2.back_button_rect;
        input_coordinator.register_on_layer(
            "color_picker_primitive:back",
            Rect { x: back_rect.x, y: back_rect.y, width: back_rect.width, height: back_rect.height },
            Sense::CLICK,
            &layer_id,
        );
    }

    input_coordinator.pop_layer(&layer_id);

    result
}
