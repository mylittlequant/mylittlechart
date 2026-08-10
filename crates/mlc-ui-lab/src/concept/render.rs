//! Concept-modal renderer — draws a [`super::spec`] archetype (one of the
//! 14 object-bound [`ConceptType`]s or one of the 9 [`ConceptModal`]s)
//! using the app's REAL modal chrome (`begin_modal_shell`/
//! `end_modal_shell`/`ModalShellSpec`/`ModalTabs`/`ScrollableContainer`,
//! copied verbatim from
//! `mlc-app-layout::layout::modals::primitive_settings`'s own render call)
//! and REAL row widgets (`primitive_settings_vid`'s row builders —
//! `draw_bool_row`/`draw_toggle_segment`/`draw_split_value_control`/
//! `render_single_slider`/`draw_input`/…).
//!
//! This is a SECOND surface, not a redesign of the first: nothing here
//! touches `mlc-app-layout`, `mlc-core`, `crate::approved_design`,
//! `crate::variants`, or the production `render_primitive_settings_modal`
//! path. `spec.rs` is the approved, fixed design; this file only has to
//! draw it faithfully.
//!
//! # Row coverage
//!
//! Every [`Row`] variant draws for real, with the app's own widgets where
//! one is directly callable against concept data, or a from-scratch redraw
//! matching a private app widget's own look where it is not (see
//! [`render_collections`]'s own module doc for the one case where even that
//! is not possible without data this crate does not have —
//! `primitive_settings_levels::draw_level_value_field`'s own `&dyn
//! Primitive` requirement). The eight collection/composite variants
//! ([`Row::Ladder`], [`Row::Ramps`], [`Row::FillMaster`], [`Row::Table`],
//! [`Row::Cells`], [`Row::Icons`], [`Row::Stepper`], [`Row::Range`]) live in
//! [`render_collections`], split out once this file crossed its own
//! soft line-count ceiling; everything else stays here.
//!
//! # Known simplifications (named, not worked around silently)
//!
//! - **Tab icons / the `chart_settings` icon rail** (`ConceptTab::icon`,
//!   `ConceptModal::rail`) are not drawn — every target's tab strip is a
//!   plain horizontal label bar (`ModalTabs::Horizontal`). Mapping
//!   `spec.rs`'s free-form icon-name strings onto the closed
//!   `mlc_app_layout::ui::Icon` enum needs a real name-to-variant table;
//!   this task did not build one.
//! - **`ConceptModal::width`/`height`** are not read — every target
//!   (type or modal) sizes itself from its own row content, clamped to the
//!   screen, exactly like the real primitive-settings modal already does.
//!   Whether `spec.rs`'s two numbers mean the outer frame or the inner
//!   body was not specified closely enough to wire without guessing.
//! - **[`Row::Slider`]'s own `value` string** (e.g. `"1.0"`, `"18%"`) is
//!   the row's own display caption; the widget it draws
//!   (`render_single_slider`) is real, but the SLIDER POSITION comes from
//!   `pct` against a synthetic 0..100 range — `spec.rs` carries no real
//!   min/max/unit for a concept slider, only a percentage position plus a
//!   caption meant for a human, so there is no numeric domain to recover
//!   dragging into (this task does not wire dragging for it anyway).
//! - **[`Row::Dropdown`]** draws the real split cycle+chevron control
//!   (`draw_split_value_control`) with a single synthetic option (the
//!   row's own current value) — `spec.rs` carries only the selected label,
//!   not a real option catalogue, so the dropdown this control could open
//!   would have exactly one row in it.
//! - **[`Row::Ladder`]'s own colour/gradient bar** reads the Classic
//!   palette's own interpolation across the ladder's row index — `spec.rs`
//!   carries no per-level colour, and this row is not linked to the Ramps
//!   row's own (also static) selection; see `render_collections`'s own doc.
//! - **[`Row::Range`]'s own `cap`** (e.g. `"1с–30с"`) is not drawn — the
//!   real widget this row mirrors (`draw_timeframes_group`) has no such
//!   caption either.

use mlc_app_layout::layout::modals::primitive_settings_levels::draw_checkbox;
use mlc_app_layout::layout::modals::primitive_settings_vid::{
    draw_bool_row, draw_group_header, draw_label, draw_split_value_control, draw_toggle_segment, SplitValueKind, ROW_H,
};
use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::layout::render_ui::toolbar_to_widget_theme;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::scroll_widget::{ScrollableConfig, ScrollableContainer};
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{
    begin_modal_shell, draw_input, end_modal_shell, labelled_tabs_width, render_single_slider, InputConfig, InputType,
    ModalFooter, ModalPlacement, ModalShellSpec, ModalTab, ModalTabs, ModalTheme, SliderConfig, SliderTrackInfo, TabFit,
    WidgetState, WidgetTheme,
};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, Sense};
use uzor::types::Rect as WidgetRect;

use super::spec::{self, ConceptTab, Row, ThemeColor, Tone};
use super::state::{ConceptModalState, ConceptTarget};

/// The eight collection/composite [`Row`] variants — see this module's own
/// doc.
mod render_collections;

// =============================================================================
// Layout constants — this crate's own copies of the numbers
// `primitive_settings_vid`/`primitive_settings.rs` keep private. Same
// values, so a concept row measures identically to the real one; kept as
// separate constants because the source ones are not `pub`.
// =============================================================================

const GROUP_HEADER_H: f64 = 22.0;
const GROUP_PAD_BOTTOM: f64 = 8.0;
const GROUP_GAP: f64 = 9.0;
/// [`Row::Area`]'s own row height — a textarea reads taller than an
/// ordinary one-line row.
const AREA_ROW_H: f64 = ROW_H * 3.0;
const SCROLLBAR_WIDTH: f64 = 8.0;
const CONTENT_SCROLLBAR_GAP: f64 = 6.0;

// =============================================================================
// Public entry point
// =============================================================================

/// Render the concept modal named by `state.open`, or draw nothing (and
/// return a default result) when no target is open — mirrors
/// `render_primitive_settings_modal`'s own early-return shape.
#[allow(clippy::too_many_arguments)]
pub fn render_concept_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    default_modal_y: f64,
    state: &ConceptModalState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut InputCoordinator,
) -> PrimitiveSettingsResult {
    let mut result = PrimitiveSettingsResult::default();

    let Some(target) = state.open else { return result };
    let Some((title, tabs)) = lookup_target(target) else { return result };
    let target_id = target.id();

    let active_tab_id = state.active_tab_id(target_id, tabs);
    let Some(active_tab) = tabs.iter().find(|t| t.id == active_tab_id) else { return result };

    let label_col = concept_label_col(ctx, tabs);
    let content_height = content_height_for_tab(active_tab);
    let max_content_height = (screen_h - 250.0).clamp(200.0, 460.0);

    let tab_items: Vec<ModalTab<'_>> = tabs.iter().map(|t| ModalTab::new(t.id, t.label.as_str())).collect();
    let active_tab_idx = tabs.iter().position(|t| t.id == active_tab_id).unwrap_or(0);
    let tabs_row_width = labelled_tabs_width(ctx, &tab_items);

    ctx.set_font("13px sans-serif");
    let title_width = ctx.measure_text(title);

    let modal_padding = 12.0;
    let close_btn_size = 20.0;
    let title_close_gap = 12.0;
    let min_header_width = title_width + title_close_gap + close_btn_size + modal_padding * 2.0;
    let min_content_width = label_col + 150.0 + modal_padding * 2.0;
    let modal_width = min_header_width.max(tabs_row_width).max(min_content_width).max(560.0);

    let modal_theme = ModalTheme::from_toolbar(&frame_theme.toolbar_bg, &toolbar_theme.separator, toolbar_theme);
    let spec = ModalShellSpec {
        padding: modal_padding,
        placement: ModalPlacement::At(Some(state.position.unwrap_or(((screen_w - modal_width) / 2.0, default_modal_y)))),
        ..ModalShellSpec::new("concept_modal", title, modal_width, content_height)
    }
    .with_max_content_height(max_content_height)
    .layer_named("concept_modal")
    .with_tabs(ModalTabs::Horizontal { items: &tab_items, active: active_tab_idx, fit: TabFit::Equal })
    .with_footer(ModalFooter::Editor { template: None })
    .resizable(state.size, (360.0, 240.0));

    let (shell, layer_id) = begin_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.tab_rects = shell.tab_rects.clone();
    result.total_content_height = content_height;
    result.viewport_height = shell.content.height;

    let modal_right = shell.frame.x + shell.frame.width;
    let content_left = shell.frame.x + modal_padding;
    let content_right = modal_right - SCROLLBAR_WIDTH - CONTENT_SCROLLBAR_GAP;
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let scroll_config = ScrollableConfig { scrollbar_width: SCROLLBAR_WIDTH, ..ScrollableConfig::default() };
    let scroll_viewport = WidgetRect::new(shell.content.x, shell.content.y, modal_right - shell.content.x, shell.content.height);
    // Reported back for the wheel routing (`crate::lib`'s own
    // `PlatformEvent::Scroll` handler, wired the same way it already
    // routes into the Object Tree's own `ScrollState` — see that call
    // site's own doc): the pane-local rect a pointer position is
    // hit-tested against before a wheel tick reaches `state.scroll`.
    result.content_rect = scroll_viewport;
    let scrollable = ScrollableContainer::new(scroll_viewport, &state.scroll, Some(scroll_config));
    scrollable.begin(ctx);
    let mut row_y = scrollable.content_y();

    let dummy_state = PrimitiveSettingsState::default();
    draw_tab_body(
        ctx,
        toolbar_theme,
        &widget_theme,
        &dummy_state,
        &mut result,
        content_left,
        content_right,
        label_col,
        &mut row_y,
        state,
        target_id,
        active_tab,
    );

    let body_items_end = result.content_items.len();
    let scroll_result = scrollable.end(ctx, content_height, &widget_theme);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;

    input_coordinator.register_on_layer(
        "concept_modal:scroll_viewport",
        uzor::types::Rect::new(shell.content.x, shell.content.y, shell.content.width, shell.content.height),
        Sense::SCROLL,
        &layer_id,
    );
    if let Some(hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer("concept_modal:scrollbar_handle", inflated, Sense::DRAG, &layer_id);
    }
    if let Some(track_rect) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "concept_modal:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            Sense::CLICK,
            &layer_id,
        );
    }

    // Body items are confined to the scroll viewport, exactly like the real
    // modal's own registration loop: a row scrolled out of sight stops
    // taking hover/clicks, and a row only half past the bottom edge keeps
    // exactly the half still visible.
    for (idx, (item_id, item_rect)) in result.content_items.iter().enumerate() {
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
        input_coordinator.register_on_layer(
            format!("concept_modal:item:{item_id}"),
            uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            Sense::CLICK | Sense::HOVER,
            &layer_id,
        );
    }

    // Re-registered after the resize grips (below, inside `end_modal_shell`)
    // so the scrollbar wins the hit test against them where the two overlap
    // — same trick `render_primitive_settings_modal` uses for the same
    // reason (see `end_modal_shell`'s own doc).
    let mut priority_regions: Vec<(&str, WidgetRect, Sense)> = Vec::new();
    if let Some(hr) = result.scrollbar_handle_rect {
        let inflated = WidgetRect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        priority_regions.push(("concept_modal:scrollbar_handle", inflated, Sense::DRAG));
    }
    if let Some(track_rect) = result.scrollbar_track_rect {
        priority_regions.push(("concept_modal:scrollbar_track", track_rect, Sense::CLICK));
    }
    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id, &priority_regions);

    result
}

/// Resolve `target` against `spec.rs`'s own registries — the title to draw
/// in the header, and the tab list to draw the strip/body from.
fn lookup_target(target: ConceptTarget) -> Option<(&'static str, &'static [ConceptTab])> {
    match target {
        ConceptTarget::Type(id) => spec::types().iter().find(|t| t.id == id).map(|t| (t.name, t.tabs.as_slice())),
        ConceptTarget::Modal(id) => spec::app_modals().iter().find(|m| m.id == id).map(|m| (m.name, m.tabs.as_slice())),
    }
}

/// The tab list for `target` — the half of [`lookup_target`] every caller
/// OUTSIDE this render pass actually needs (tab-switch dispatch, row-click
/// dispatch, the agent control plane, `crate::lib`'s own snapshot/metrics
/// readers). `lookup_target` itself stays private to this module.
pub fn tabs_for(target: ConceptTarget) -> Option<&'static [ConceptTab]> {
    lookup_target(target).map(|(_, tabs)| tabs)
}

// =============================================================================
// Sizing
// =============================================================================

/// The ONE label-column width every labelled row in `tabs` starts its own
/// control against — the concept-data equivalent of
/// `primitive_settings_vid::compute_label_col`, which measures a live
/// `Primitive`'s own declared properties and cannot be reused here (there
/// is no `Primitive` behind a concept row). Measures every row caption
/// across every tab of the target being opened, not only the active one,
/// so the column does not visibly shift width when the owner switches tabs.
fn concept_label_col(ctx: &mut dyn RenderContext, tabs: &[ConceptTab]) -> f64 {
    ctx.set_font("12px sans-serif");
    let mut max_w = 0.0_f64;
    for tab in tabs {
        for group in &tab.groups {
            for row in &group.rows {
                if let Some(caption) = row_caption(row) {
                    max_w = max_w.max(ctx.measure_text(&caption));
                }
            }
        }
    }
    (max_w + 8.0).clamp(90.0, 220.0)
}

/// The caption a row's own label column would show, for the rows that have
/// one — mirrors `compute_label_col`'s own caption list (a `Slider`/`Num`
/// row measures with its trailing `":"` too, since that is what actually
/// paints).
fn row_caption(row: &Row) -> Option<String> {
    match row {
        Row::Chk { label, .. } | Row::Color { label, .. } | Row::Dropdown { label, .. } | Row::Stepper { label, .. } => {
            Some(label.clone())
        }
        Row::Slider { label, .. } | Row::Num { label, .. } => Some(format!("{label}:")),
        Row::Mode { label, .. } if !label.is_empty() => Some(label.clone()),
        Row::Pills { label: Some(label), .. } => Some(label.clone()),
        _ => None,
    }
}

/// Height one `Row` occupies, regardless of tab — one `ROW_H` line for
/// every plain field, [`AREA_ROW_H`] for [`Row::Area`]'s own textarea, and
/// one dedicated `*_height` function per collection/composite variant (see
/// [`render_collections`]) — the single source of truth
/// [`draw_row`]'s own dispatch never recomputes inline, so the two can
/// never drift apart.
fn row_height(row: &Row) -> f64 {
    match row {
        Row::Area { .. } => AREA_ROW_H,
        Row::Ladder { values, .. } => render_collections::ladder_height(values.len()),
        Row::Ramps => render_collections::ramps_height(),
        Row::FillMaster { .. } => render_collections::fillmaster_height(),
        Row::Table { rows, .. } => render_collections::table_height(rows.len()),
        Row::Cells { rows, .. } => render_collections::cells_height(rows.len()),
        Row::Icons { options, .. } => render_collections::icons_height(options.len()),
        Row::Stepper { .. } => render_collections::stepper_height(),
        Row::Range { .. } => render_collections::range_height(),
        _ => ROW_H,
    }
}

/// True height `tab`'s own rows need, padding included — the scroll
/// container's own content height. Mirrors `draw_tab_body`'s bookkeeping
/// exactly (one `GROUP_GAP` between groups, one `GROUP_HEADER_H` +
/// `GROUP_PAD_BOTTOM` per group, one [`row_height`] per row) so the two can
/// never drift apart.
fn content_height_for_tab(tab: &ConceptTab) -> f64 {
    let mut total = 0.0;
    for (idx, group) in tab.groups.iter().enumerate() {
        if idx > 0 {
            total += GROUP_GAP;
        }
        total += GROUP_HEADER_H;
        for row in &group.rows {
            total += row_height(row);
        }
        total += GROUP_PAD_BOTTOM;
    }
    total
}

// =============================================================================
// Body
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_tab_body(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    dummy_state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    row_y: &mut f64,
    state: &ConceptModalState,
    target_id: &str,
    tab: &ConceptTab,
) {
    for (group_idx, group) in tab.groups.iter().enumerate() {
        if group_idx > 0 {
            *row_y += GROUP_GAP;
        }
        draw_group_header(ctx, toolbar_theme, x0, x1, *row_y, &group.title, group.hint.as_deref());
        *row_y += GROUP_HEADER_H;
        for (row_idx, row) in group.rows.iter().enumerate() {
            let key = super::state::row_key(target_id, tab.id, group_idx, row_idx);
            draw_row(ctx, toolbar_theme, widget_theme, dummy_state, result, x0, x1, label_col, *row_y, state, &key, row);
            *row_y += row_height(row);
        }
        *row_y += GROUP_PAD_BOTTOM;
    }
}

/// Dispatch one `Row` to its own real widget — the eight collection/
/// composite variants forward into [`render_collections`], everything else
/// draws right here.
#[allow(clippy::too_many_arguments)]
fn draw_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    dummy_state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    state: &ConceptModalState,
    key: &str,
    row: &Row,
) {
    match row {
        Row::Chk { label, on } => {
            let checked = state.chk(key, *on);
            draw_bool_row(ctx, toolbar_theme, result, x0, label_col, y, label, checked, key);
        }
        Row::Chk3 { items } => draw_chk3_row(ctx, toolbar_theme, result, x0, x1, y, key, items, state),
        Row::Color { label, value, dim } => {
            draw_color_row(ctx, toolbar_theme, result, x0, x1, label_col, y, label, *value, *dim, key);
        }
        Row::Slider { label, pct, value } => {
            draw_slider_row(ctx, toolbar_theme, widget_theme, result, x0, x1, label_col, y, key, label, *pct, value);
        }
        Row::Dropdown { label, value } => {
            draw_dropdown_row(ctx, toolbar_theme, dummy_state, result, x0, x1, label_col, y, label, value, key);
        }
        Row::Mode { label, options, selected } => {
            let idx = state.segment(key, *selected);
            draw_segmented_row(ctx, toolbar_theme, result, x0, x1, label_col, y, Some(label.as_str()), options, idx, key, "mode");
        }
        Row::Pills { label, options, selected } => {
            let idx = state.segment(key, *selected);
            draw_segmented_row(ctx, toolbar_theme, result, x0, x1, label_col, y, label.as_deref(), options, idx, key, "pills");
        }
        Row::Note { text } => draw_note_row(ctx, toolbar_theme, x0, x1, y, text),
        Row::Area { value } => draw_area_row(ctx, widget_theme, result, x0, x1, y, value, key),
        Row::Num { label, value } => draw_num_row(ctx, toolbar_theme, widget_theme, result, x0, x1, label_col, y, label, value, key),
        Row::Hr => draw_hr_row(ctx, toolbar_theme, x0, x1, y),
        Row::Stat { label, value, tone } => draw_stat_row(ctx, toolbar_theme, x0, x1, y, label, value, *tone),
        Row::Ladder { values, unit } => {
            render_collections::draw_ladder_row(ctx, toolbar_theme, widget_theme, result, x0, x1, y, state, key, values, *unit);
        }
        Row::Ramps => render_collections::draw_ramps_row(ctx, toolbar_theme, x0, x1, y),
        Row::FillMaster { label, pct, value, on } => {
            render_collections::draw_fillmaster_row(ctx, toolbar_theme, widget_theme, x0, x1, y, label, *pct, value, *on);
        }
        Row::Table { head, rows } => {
            render_collections::draw_table_row(ctx, toolbar_theme, result, x0, x1, y, state, key, head, rows);
        }
        Row::Cells { cols, rows } => render_collections::draw_cells_row(ctx, widget_theme, x0, x1, y, *cols, rows),
        Row::Icons { options, selected } => {
            render_collections::draw_icons_row(ctx, toolbar_theme, result, x0, x1, y, state, key, options, *selected);
        }
        Row::Stepper { label, value } => {
            render_collections::draw_stepper_row(ctx, toolbar_theme, result, x0, label_col, y, state, key, label, *value);
        }
        Row::Range { label, from, to, cap: _ } => {
            render_collections::draw_range_row(ctx, toolbar_theme, widget_theme, dummy_state, result, x0, x1, y, key, label, *from, *to);
        }
    }
}

// =============================================================================
// Real rows
// =============================================================================

/// [`Row::Chk3`] — three checkboxes sharing one row. No dedicated shared
/// widget exists for this layout (`primitive_settings_vid`'s own doc notes
/// the old two-up segmented pairing was dropped, 2026-08-08); this lays
/// [`draw_bool_row`] out three-across instead of once, which is still the
/// real checkbox widget, just repeated.
#[allow(clippy::too_many_arguments)]
fn draw_chk3_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    key: &str,
    items: &[(String, bool)],
    state: &ConceptModalState,
) {
    let n = items.len().max(1);
    let slab = ((x1 - x0) / n as f64).max(60.0);
    let current = state.chk3(key, items);
    let box_size = 16.0;
    let box_gap = 8.0;
    for (i, (label, _)) in items.iter().enumerate() {
        let sx0 = x0 + slab * i as f64;
        let checked = current.get(i).copied().unwrap_or(false);
        let field_id = format!("{key}::chk3:{i}");

        // Box FIRST, then its own caption — not `draw_bool_row`'s
        // caption-then-box order. That order is right for a full-width row
        // (one caption in the shared label column, its control in the
        // control column), but three of them side by side read as
        // caption-A box-B caption-B box-C: every box sits next to the
        // caption it does NOT belong to.
        let box_rect = WidgetRect::new(sx0, y + (ROW_H - box_size) / 2.0, box_size, box_size);
        draw_checkbox(ctx, box_rect, checked, toolbar_theme, &toolbar_theme.background);

        let text_x = box_rect.right() + box_gap;
        let text_w = (sx0 + slab - text_x - box_gap).max(0.0);
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.save();
        ctx.begin_path();
        ctx.rect(text_x, y, text_w, ROW_H);
        ctx.clip();
        ctx.fill_text(label, text_x, y + ROW_H / 2.0);
        ctx.restore();

        // The whole slab is the hit target, so the caption toggles its own
        // box the way it does in every other checkbox row in the app.
        result.content_items.push((field_id, WidgetRect::new(sx0, y, slab, ROW_H)));
    }
}

/// [`Row::Color`] — a labelled swatch, drawn the same way
/// `primitive_settings_vid::draw_base_stroke_rows` draws its own colour
/// row (there is no separately exported "colour row" widget to call
/// instead — that block is inlined in the source too).
#[allow(clippy::too_many_arguments)]
fn draw_color_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    label: &str,
    color: ThemeColor,
    dim: bool,
    key: &str,
) {
    draw_label(ctx, toolbar_theme, x0, label_col, y, label);
    let swatch_w = (x1 - (x0 + label_col)).clamp(40.0, 60.0);
    let rect = WidgetRect::new(x0 + label_col, y + 3.0, swatch_w, ROW_H - 6.0);
    let hex = theme_color(toolbar_theme, color);
    let fill = if dim { mlc_core::apply_opacity(hex, 0.5) } else { hex.to_string() };
    ctx.set_fill_color(&fill);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    result.content_items.push((key.to_string(), rect));
}

/// Maps a [`ThemeColor`] onto the live `RuntimeTheme` — the ONE place this
/// crate does that translation (task requirement). `ToolbarTheme`'s own
/// fields are already themselves derived straight off `RuntimeTheme` (see
/// `crate::theme::toolbar_theme`), so this is a direct field pick, not a
/// second colour system.
fn theme_color(tt: &ToolbarTheme, color: ThemeColor) -> &str {
    match color {
        ThemeColor::Accent => &tt.accent,
        ThemeColor::Success => &tt.success,
        ThemeColor::Danger => &tt.danger,
        ThemeColor::Warning => &tt.warning,
        ThemeColor::TextPrimary => &tt.item_text,
        ThemeColor::TextMuted => &tt.item_text_muted,
        ThemeColor::BorderLight => &tt.separator,
        ThemeColor::ToolbarBg | ThemeColor::ModalBg => &tt.background,
    }
}

/// [`Row::Slider`] — calls the same underlying widget
/// (`render_single_slider`) `primitive_settings_vid::draw_slider_row`
/// itself wraps, rather than that wrapper: the wrapper's own trailing
/// caption is always the numeric value formatted `"{v:.0}"`, and a concept
/// slider's own `value` is a free-form display string (`"1.0"`, `"18%"`,
/// …) the wrapper has no parameter for. Calling the underlying renderer
/// directly draws the identical track/handle/label chrome and lets this
/// module paint the row's own real caption next to it. The track position
/// itself is `pct` (0..100) — see this module's own top-of-file doc for
/// why there is no real numeric domain to recover here.
#[allow(clippy::too_many_arguments)]
fn draw_slider_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    field_id: &str,
    label: &str,
    pct: u8,
    display_value: &str,
) {
    let cfg = SliderConfig::new(0.0, 100.0).with_step(1.0).with_label_column(label_col).without_input();
    let rect = WidgetRect::new(x0, y, (x1 - x0).max(80.0), ROW_H);
    let res = render_single_slider(ctx, &cfg, pct as f64, rect, &format!("{label}:"), widget_theme, false, None);

    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(display_value, res.track_rect.right() + 8.0, y + ROW_H / 2.0);

    result.content_items.push((field_id.to_string(), res.full_rect));
    if let Some(t) = res.track_info {
        result.slider_tracks.push(SliderTrackInfo {
            field_id: field_id.to_string(),
            track_x: t.track_x,
            track_width: t.track_width,
            min_val: 0.0,
            max_val: 100.0,
        });
    }
}

/// [`Row::Dropdown`] — the real split cycle+chevron control. See this
/// module's own top-of-file doc for why the option list is synthetic
/// (`spec.rs` carries only the current label).
#[allow(clippy::too_many_arguments)]
fn draw_dropdown_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    dummy_state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    label: &str,
    value: &str,
    key: &str,
) {
    draw_label(ctx, toolbar_theme, x0, label_col, y, label);
    let control_w = (x1 - (x0 + label_col)).max(100.0);
    let rect = WidgetRect::new(x0 + label_col, y + 2.0, control_w, ROW_H - 4.0);
    draw_split_value_control(
        ctx,
        toolbar_theme,
        dummy_state,
        result,
        rect,
        key,
        value,
        SplitValueKind::Select { options: vec![(value.to_string(), value.to_string())], current: value.to_string() },
    );
}

/// [`Row::Mode`]/[`Row::Pills`] — a row of [`draw_toggle_segment`] buttons,
/// with an optional leading label (`Row::Mode`'s own label is `""` on every
/// occurrence but the ladder tabs' Моно/Палитра switch; `Row::Pills`' is
/// `None` on every occurrence but two — see `spec.rs`'s own doc).
#[allow(clippy::too_many_arguments)]
fn draw_segmented_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    label: Option<&str>,
    options: &[String],
    selected: usize,
    key: &str,
    kind: &str,
) {
    let control_x0 = match label.filter(|l| !l.is_empty()) {
        Some(l) => {
            draw_label(ctx, toolbar_theme, x0, label_col, y, l);
            x0 + label_col
        }
        None => x0,
    };
    let n = options.len().max(1);
    let seg_w = ((x1 - control_x0) / n as f64).max(30.0);
    for (i, opt) in options.iter().enumerate() {
        let rect = WidgetRect::new(control_x0 + seg_w * i as f64, y + 2.0, (seg_w - 1.0).max(1.0), ROW_H - 4.0);
        draw_toggle_segment(ctx, toolbar_theme, rect, opt, i == selected);
        result.content_items.push((format!("{key}::{kind}:{i}"), rect));
    }
}

fn draw_note_row(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x0: f64, x1: f64, y: f64, text: &str) {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.save();
    ctx.begin_path();
    ctx.rect(x0, y, (x1 - x0).max(0.0), ROW_H);
    ctx.clip();
    ctx.fill_text(text, x0, y + ROW_H / 2.0);
    ctx.restore();
}

/// [`Row::Area`] — a free-text textarea's content, drawn with the real
/// input widget ([`draw_input`]) at [`AREA_ROW_H`], inert (no edit focus —
/// see this module's own top-of-file scope note).
fn draw_area_row(
    ctx: &mut dyn RenderContext,
    widget_theme: &WidgetTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    value: &str,
    key: &str,
) {
    let rect = WidgetRect::new(x0, y + 3.0, (x1 - x0).max(60.0), AREA_ROW_H - 6.0);
    let cfg = InputConfig::new(value);
    draw_input(ctx, &cfg, WidgetState::Normal, rect, widget_theme);
    result.content_items.push((key.to_string(), rect));
}

/// [`Row::Num`] — a numeric input, drawn with the real input widget.
#[allow(clippy::too_many_arguments)]
fn draw_num_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    label_col: f64,
    y: f64,
    label: &str,
    value: &str,
    key: &str,
) {
    draw_label(ctx, toolbar_theme, x0, label_col, y, label);
    let control_w = (x1 - (x0 + label_col)).max(60.0);
    let rect = WidgetRect::new(x0 + label_col, y + 3.0, control_w, ROW_H - 6.0);
    let cfg = InputConfig::new(value).with_type(InputType::Number);
    draw_input(ctx, &cfg, WidgetState::Normal, rect, widget_theme);
    result.content_items.push((key.to_string(), rect));
}

fn draw_hr_row(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x0: f64, x1: f64, y: f64) {
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x0, y + ROW_H / 2.0);
    ctx.line_to(x1, y + ROW_H / 2.0);
    ctx.stroke();
}

/// [`Row::Stat`] — a read-only computed value: label left, tone-coloured
/// value right, the same shape Position's Risk/Target/R:R and Regression's
/// slope/R² read as in the real modal (`PropertyCategory::AppearanceStats`).
fn draw_stat_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    x0: f64,
    x1: f64,
    y: f64,
    label: &str,
    value: &str,
    tone: Option<Tone>,
) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x0, y + ROW_H / 2.0);

    let color = match tone {
        Some(Tone::Good) => toolbar_theme.success.clone(),
        Some(Tone::Bad) => toolbar_theme.danger.clone(),
        Some(Tone::Neutral) | None => toolbar_theme.item_text.clone(),
    };
    ctx.set_fill_color(&color);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(value, x1, y + ROW_H / 2.0);
}
