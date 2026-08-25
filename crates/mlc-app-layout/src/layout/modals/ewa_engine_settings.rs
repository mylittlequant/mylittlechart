//! EWA Engine Settings modal renderer.
//!
//! One process-global, RAM+profile settings surface for the Elliott-wave
//! ENGINE itself (owner ask 2026-08-14: "для настройки ЕВА движка можно
//! сделать отдельную настройку, ну вот прям модалку ЕВА движка, в которую
//! можно попасть из каждого ЕВА дравлинга — из его модалки настроек или по
//! какой-то ещё общей кнопке типа EWA settings в ПКМ или дабл-ЛКМ") —
//! distinct from the per-drawing settings every Elliott primitive already
//! carries in its own settings modal (`primitive_settings_vid`'s own Subject
//! tab). Two entrances reach this modal: the wave-menu popups' own trailing
//! row (`crate::ui::context_menu::{build_wave_menu, build_successor_menu,
//! build_predecessor_menu}`, action `"wave_menu:engine_settings"`) and a
//! button in the Wave archetype's own Subject tab
//! (`primitive_settings_vid::draw_ewa_engine_settings_button`).
//!
//! Every edit applies immediately (writes through to
//! `mlc_core::state::ewa_engine::set_ewa_engine_config` and the profile) —
//! this config has no per-drawing counterpart and therefore no OK/Cancel
//! diff to manage, exactly mirroring how
//! `mlc_core::drawing::primitives_v2::set_control_point_style` already
//! behaves for the User Settings modal's own Control Points pickers. The
//! footer's ✕/OK both simply close the modal.
//!
//! Built from the SAME small building blocks
//! `layout::modals::primitive_settings_vid` already exports for exactly this
//! reuse (`draw_label`/`draw_group_header`/`draw_toggle_segment`/`ROW_H`/
//! `line_style_display_name`) — the ones that do not need a
//! `PrimitiveSettingsState`/`PrimitiveSettingsResult` in hand. The row
//! functions that DO (`draw_bool_row`/`draw_slider_row`/
//! `draw_split_value_control`) are typed to that specific pair, so this
//! modal's own checkbox and segmented-control rows are written locally
//! below, visually matching those functions rather than requiring a
//! refactor of a load-bearing, heavily-used file to genericize them.
//!
//! Fixed size, no tabs, no scrolling — the whole body is a dozen rows and
//! always fits.

use mlc_core::drawing::primitives_v2::{LabelSeparationStyle, LineStyle};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, ConceptKey, ConfigKey, SettingsKey, t_concept, t_settings};
use uzor::types::Rect as WidgetRect;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::EwaEngineSettingsResult;
use crate::ui::modal_settings::EwaEngineSettingsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{render_modal_shell, ModalFooter, ModalShellSpec, ModalTheme};

use super::primitive_settings_vid::{draw_group_header, draw_label, draw_toggle_segment, line_style_display_name, ROW_H};

/// Matches `primitive_settings_vid`'s own private `GROUP_HEADER_H` — that
/// constant is not `pub` (nothing outside its own module needed it before
/// this file), so this is a documented, intentional duplicate of the same
/// literal rather than a widened export for one caller.
const GROUP_HEADER_H: f64 = 22.0;
const GROUP_GAP: f64 = 12.0;
const ROW_GAP: f64 = 6.0;
const SEGMENT_GAP: f64 = 4.0;

const MODAL_W: f64 = 460.0;

/// The 5-swatch palette the ratio-rib COLOUR row offers, `#rrggbb` only
/// (never `#rrggbbaa` — the ribs' own default already carries the alpha, see
/// `elliott::ratio::default_rib_color`). Not exhaustive by design: a fixed,
/// small closed set (mirrors `ControlPointSize`'s own "small closed set"
/// rationale) so a saved profile can never carry a degenerate value, and a
/// user who wants a bespoke colour still has the per-primitive picker on
/// each individual drawing's own settings modal.
const RATIO_RIB_COLOR_PALETTE: [&str; 4] = ["#2196F3", "#FF9800", "#4CAF50", "#F44336"];

/// The ratio-rib WIDTH row's own fixed option set, in pixels.
const RATIO_RIB_WIDTH_OPTIONS: [f64; 4] = [1.0, 1.5, 2.0, 3.0];

/// The fixed 5-variant line-style catalogue, in the same order every other
/// line-style picker in this crate lists them (see
/// `primitive_settings_vid`'s own `LINE_STYLE_CATALOGUE`, not `pub` — this
/// is the same literal list, kept local for the same reason
/// `GROUP_HEADER_H` above is).
const LINE_STYLE_CATALOGUE: [LineStyle; 5] =
    [LineStyle::Solid, LineStyle::Dashed, LineStyle::Dotted, LineStyle::LargeDashed, LineStyle::SparseDotted];

/// Body height this modal always needs — fixed, since nothing here can grow
/// (no ladder, no template list, no search). Kept as one function so the
/// modal's own `content_height` and this file's own row-by-row `draw` can
/// never drift apart.
fn content_height() -> f64 {
    let mut h = 0.0;
    h += GROUP_HEADER_H + ROW_H + GROUP_GAP; // Label separation
    h += ROW_H * 2.0 + ROW_GAP + GROUP_GAP; // Detect higher-degree / show projections
    h += GROUP_HEADER_H + ROW_H * 3.0 + ROW_GAP * 2.0 + GROUP_GAP; // Rule guard (3 checkboxes)
    h += GROUP_HEADER_H + ROW_H * 3.0 + ROW_GAP * 2.0; // Ratio-rib defaults (colour/width/style)
    h
}

/// Render the EWA Engine Settings modal.
pub fn render_ewa_engine_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &EwaEngineSettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> EwaEngineSettingsResult {
    let mut result = EwaEngineSettingsResult::default();
    let lang = current_language();

    let modal_theme = ModalTheme::from_toolbar(&frame_theme.toolbar_bg, &toolbar_theme.separator, toolbar_theme);
    let spec = ModalShellSpec {
        placement: crate::ui::widgets::ModalPlacement::At(Some(
            state.position.unwrap_or(((screen_w - MODAL_W) / 2.0, (screen_h - content_height()) / 2.0)),
        )),
        // Above the modal that opened it, not beside it. Both entrances to
        // an engine-settings modal are inside ANOTHER modal (a primitive's
        // own Subject tab, the wave-gesture popup), so this is by
        // definition a modal stacked over a modal — the case
        // `ZLayer::ModalOverlay` exists for, and the one
        // `template_name_modal` already documents.
        //
        // On the default `ZLayer::Modal` it was unreachable, and the
        // mechanism is worth writing down because nothing about it is
        // visible on screen: `hit_test_at_filtered` sorts layers by
        // z-order with a STABLE sort, so two modals at the same z keep
        // registration order — the one underneath is scanned FIRST, and
        // being modal it raises the barrier that skips every non-chrome
        // layer after it. The modal drawn on top therefore received no
        // clicks at all: rows inside the lower modal's rect went to the
        // lower modal, everything else fell through to the backdrop and
        // closed the very modal that was clicked.
        layer: crate::ui::z_order::ZLayer::ModalOverlay,
        ..ModalShellSpec::new("ewa_engine_settings", t_concept(ConceptKey::EwaEngineSettingsTitle), MODAL_W, content_height())
            .with_footer(ModalFooter::Editor { template: None })
    };

    let cfg = &state.config;
    let label_col = 100.0;

    let shell = render_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h), |ctx, coord, layer_id, shell| {
        let x0 = shell.content.x;
        let x1 = shell.content.x + shell.content.width;
        let mut row_y = shell.content.y;

        // ── Label separation ────────────────────────────────────────────
        draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::EwaLabelSeparationGroupTitle), None);
        row_y += GROUP_HEADER_H;
        let separation_segments: [(LabelSeparationStyle, &str); 3] = [
            (LabelSeparationStyle::Stacked, t_concept(ConceptKey::EwaLabelSeparationStacked)),
            (LabelSeparationStyle::Parallel, t_concept(ConceptKey::EwaLabelSeparationParallel)),
            (LabelSeparationStyle::Triangle, t_concept(ConceptKey::EwaLabelSeparationTriangle)),
        ];
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, separation_segments.len(), |i| {
            let (value, label) = separation_segments[i];
            (label_separation_field_id(value), label, cfg.label_separation == value)
        });
        row_y += ROW_H + GROUP_GAP;

        // ── Detect higher-degree cycles / show projections ─────────────
        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::EwaDetectHigherDegreeLabel), cfg.detect_higher_degree, "ewa:detect_higher_degree");
        row_y += ROW_H + ROW_GAP;
        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::EwaShowProjectionsLabel), cfg.show_projections, "ewa:show_projections");
        row_y += ROW_H + GROUP_GAP;

        // ── Rule guard ───────────────────────────────────────────────────
        draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::EwaGuardFamiliesGroupTitle), None);
        row_y += GROUP_HEADER_H;
        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::EwaGuardGeometryLabel), cfg.guard_families.geometry, "ewa:guard_geometry");
        row_y += ROW_H + ROW_GAP;
        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::EwaGuardGrammarLabel), cfg.guard_families.grammar, "ewa:guard_grammar");
        row_y += ROW_H + ROW_GAP;
        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::EwaGuardChainLabel), cfg.guard_families.chain, "ewa:guard_chain");
        row_y += ROW_H + GROUP_GAP;

        // ── New-structure ratio-rib defaults ────────────────────────────
        draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::EwaRatioRibDefaultsGroupTitle), None);
        row_y += GROUP_HEADER_H;

        draw_label(ctx, toolbar_theme, x0, label_col, row_y, t_settings(SettingsKey::Color));
        draw_ratio_color_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0 + label_col, row_y, cfg.ratio_rib_defaults.color.as_deref());
        row_y += ROW_H + ROW_GAP;

        draw_label(ctx, toolbar_theme, x0, label_col, row_y, ConfigKey::WidthLabel.get(lang));
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0 + label_col, x1, row_y, RATIO_RIB_WIDTH_OPTIONS.len() + 1, |i| {
            if i == 0 {
                ("ewa:ratio_width:auto".to_string(), "Auto", cfg.ratio_rib_defaults.width.is_none())
            } else {
                let w = RATIO_RIB_WIDTH_OPTIONS[i - 1];
                (format!("ewa:ratio_width:{w}"), width_label(w), cfg.ratio_rib_defaults.width == Some(w))
            }
        });
        row_y += ROW_H + ROW_GAP;

        draw_label(ctx, toolbar_theme, x0, label_col, row_y, ConfigKey::Style.get(lang));
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0 + label_col, x1, row_y, LINE_STYLE_CATALOGUE.len(), |i| {
            let style = LINE_STYLE_CATALOGUE[i];
            (format!("ewa:ratio_style:{}", style.as_str()), line_style_display_name(style, lang), cfg.ratio_rib_defaults.style == style)
        });
    });

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result
}

/// The widget id one [`LabelSeparationStyle`] variant registers under.
fn label_separation_field_id(style: LabelSeparationStyle) -> String {
    match style {
        LabelSeparationStyle::Stacked => "ewa:label_separation:stacked".to_string(),
        LabelSeparationStyle::Parallel => "ewa:label_separation:parallel".to_string(),
        LabelSeparationStyle::Triangle => "ewa:label_separation:triangle".to_string(),
    }
}

/// `1.5px` reads better than a bare `1.5` on a narrow segment — trims a
/// trailing `.0` (`1.0` -> `1`) but keeps a real fraction (`1.5` -> `1.5`).
fn width_label(w: f64) -> &'static str {
    match w {
        w if w == 1.0 => "1px",
        w if w == 1.5 => "1.5px",
        w if w == 2.0 => "2px",
        w if w == 3.0 => "3px",
        _ => "px",
    }
}

/// One checkbox row — visually matches `primitive_settings_vid::draw_bool_row`
/// (same rounded checkbox + checkmark shape), written locally because that
/// function is typed to `PrimitiveSettingsResult`, not this modal's own
/// [`EwaEngineSettingsResult`] (see this file's own module doc).
#[allow(clippy::too_many_arguments)]
fn draw_checkbox_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut EwaEngineSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    label: &str,
    checked: bool,
    field_id: &str,
) {
    let checkbox_size = 16.0;
    let cb_x = x0;
    let cb_y = y + (ROW_H - checkbox_size) / 2.0;
    ctx.set_fill_color(if checked { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
    ctx.fill_rounded_rect(cb_x, cb_y, checkbox_size, checkbox_size, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb_y, checkbox_size, checkbox_size, 3.0);
    if checked {
        ctx.set_stroke_color(&toolbar_theme.item_text_active);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(cb_x + 3.0, cb_y + checkbox_size * 0.55);
        ctx.line_to(cb_x + checkbox_size * 0.42, cb_y + checkbox_size - 3.0);
        ctx.line_to(cb_x + checkbox_size - 3.0, cb_y + 3.0);
        ctx.stroke();
    }
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, cb_x + checkbox_size + 8.0, y + ROW_H / 2.0);

    let rect = WidgetRect::new(x0, y, (x1 - x0).max(checkbox_size), ROW_H);
    result.content_items.push((field_id.to_string(), rect));
    let hit_id = format!("ewa_engine_settings:{field_id}");
    coord.register_on_layer(hit_id.as_str(), rect, uzor::input::Sense::CLICK | uzor::input::Sense::HOVER, layer_id);
}

/// One row of `count` equal-width segments, each drawn with
/// `primitive_settings_vid::draw_toggle_segment` (reused verbatim — that
/// function takes no state/result of its own). `resolve(i)` returns
/// `(field_id, label, is_current)` for segment `i`.
#[allow(clippy::too_many_arguments)]
fn draw_segmented_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut EwaEngineSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    count: usize,
    resolve: impl Fn(usize) -> (String, &'static str, bool),
) {
    if count == 0 {
        return;
    }
    let n = count as f64;
    let w = (((x1 - x0) - SEGMENT_GAP * (n - 1.0)) / n).max(20.0);
    let mut x = x0;
    for i in 0..count {
        let (field_id, label, is_current) = resolve(i);
        let rect = WidgetRect::new(x, y + 2.0, w, ROW_H - 4.0);
        draw_toggle_segment(ctx, toolbar_theme, rect, label, is_current);
        result.content_items.push((field_id.clone(), rect));
        let hit_id = format!("ewa_engine_settings:{field_id}");
        coord.register_on_layer(hit_id.as_str(), rect, uzor::input::Sense::CLICK | uzor::input::Sense::HOVER, layer_id);
        x += w + SEGMENT_GAP;
    }
}

/// The ratio-rib COLOUR row: an "Auto" chip (inherit from the new
/// structure's own stroke colour, `ratio_rib_defaults.color == None`) plus
/// [`RATIO_RIB_COLOR_PALETTE`]'s own fixed swatches.
#[allow(clippy::too_many_arguments)]
fn draw_ratio_color_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut EwaEngineSettingsResult,
    x0: f64,
    y: f64,
    current: Option<&str>,
) {
    const AUTO_CHIP_W: f64 = 64.0;
    const SWATCH: f64 = ROW_H - 6.0;

    let auto_rect = WidgetRect::new(x0, y + 2.0, AUTO_CHIP_W, ROW_H - 4.0);
    draw_toggle_segment(ctx, toolbar_theme, auto_rect, "Auto", current.is_none());
    result.content_items.push(("ewa:ratio_color:auto".to_string(), auto_rect));
    coord.register_on_layer("ewa_engine_settings:ewa:ratio_color:auto", auto_rect, uzor::input::Sense::CLICK | uzor::input::Sense::HOVER, layer_id);

    let mut x = x0 + AUTO_CHIP_W + SEGMENT_GAP;
    for hex in RATIO_RIB_COLOR_PALETTE {
        let rect = WidgetRect::new(x, y + (ROW_H - SWATCH) / 2.0, SWATCH, SWATCH);
        let is_current = current == Some(hex);
        ctx.set_fill_color(hex);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
        ctx.set_stroke_color(if is_current { &toolbar_theme.item_text_active } else { &toolbar_theme.separator });
        ctx.set_stroke_width(if is_current { 2.0 } else { 1.0 });
        ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
        let field_id = format!("ewa:ratio_color:{}", hex.trim_start_matches('#'));
        result.content_items.push((field_id.clone(), rect));
        let hit_id = format!("ewa_engine_settings:{field_id}");
        coord.register_on_layer(hit_id.as_str(), rect, uzor::input::Sense::CLICK | uzor::input::Sense::HOVER, layer_id);
        x += SWATCH + SEGMENT_GAP;
    }
}
