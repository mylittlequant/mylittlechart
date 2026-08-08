//! The approved design's "Уровни" tab: Набор selector + вернуть, the four
//! palette cards, one global fill-opacity slider, the ladder (checkbox,
//! editable value, preview bar, colour/fill chips, delete), the selected
//! row's inspector, and "+ добавить уровень" at the bottom.
//!
//! Source of truth: `crates/mlc-app-shell-web/pkg/fib_modal.html`'s
//! "УРОВНИ" column. `DesignUiState::composition` is what decides which
//! rows exist (see its own doc comment for why that is a separate list
//! from each row's own `visible` checkbox); this module only draws — every
//! mutation (набор swap, add, delete, colour/width/style edits) happens in
//! `crate::dispatch` in response to the widget ids this module registers.

use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{
    draw_input, draw_input_cursor, render_single_slider, InputConfig, InputType, SliderConfig, WidgetState,
    WidgetTheme,
};
use mlc_app_layout::layout::render_frame::{PrimitiveSettingsResult, SliderTrackInfo};
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_core::drawing::primitives_v2::config::FibLevelConfig;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::i18n::{current_language, ConfigKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use crate::design_state::{DesignUiState, LevelSet};
use crate::palettes::PALETTES;

const ROW_H: f64 = 26.0;
const CHECK: f64 = 16.0;
const CHIP: f64 = 14.0;
const BAR_H: f64 = 14.0;
const DEL_W: f64 = 14.0;
const COL_GAP: f64 = 7.0;
const RAMP_CARD_H: f64 = 46.0;
const SET_ROW_H: f64 = 26.0;
const GLOBAL_ROW_H: f64 = 26.0;
const ADD_ROW_H: f64 = 34.0;
const BLOCK_GAP: f64 = 10.0;
const INSPECTOR_ROW_H: f64 = 22.0;
const INSPECTOR_H: f64 = INSPECTOR_ROW_H * 5.0 + 20.0;
const SET_OPTION_H: f64 = 24.0;

/// Widget-id key for a level: its own value, 3-decimal — stable across a
/// набор swap or a resort, unlike a positional index would be (see
/// `DesignUiState::overridden`'s doc comment).
fn vkey(level: f64) -> String {
    format!("{level:.3}")
}

/// The body height this tab needs — computed before the shell opens (the
/// shell needs it up front), from the same numbers the draw pass below
/// uses, so the two can never drift apart.
pub fn content_height(design_ui: &DesignUiState) -> f64 {
    let n = design_ui.composition.len();
    let inspector_h = if design_ui.selected.is_some() { INSPECTOR_H } else { 0.0 };
    let ladder_h = (n as f64 * ROW_H + inspector_h).max(ROW_H);
    SET_ROW_H + BLOCK_GAP + RAMP_CARD_H + BLOCK_GAP + GLOBAL_ROW_H + BLOCK_GAP + ladder_h + BLOCK_GAP + ADD_ROW_H
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    row_y: &mut f64,
    prim: &dyn Primitive,
    design_ui: &DesignUiState,
) {
    let configs = prim.level_configs().unwrap_or_default();
    let fallback_color = prim.data().color.stroke.clone();
    let fallback_width = prim.data().width;

    let set_sel_rect = draw_set_row(ctx, toolbar_theme, result, x0, x1, *row_y, design_ui);
    *row_y += SET_ROW_H + BLOCK_GAP;

    draw_ramps_row(ctx, toolbar_theme, result, x0, x1, *row_y, design_ui);
    *row_y += RAMP_CARD_H + BLOCK_GAP;

    draw_global_opacity_row(ctx, widget_theme, state, result, x0, x1, *row_y, design_ui);
    *row_y += GLOBAL_ROW_H + BLOCK_GAP;

    let mut sorted: Vec<f64> = design_ui.composition.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    draw_ladder(
        ctx, toolbar_theme, widget_theme, state, result, x0, x1, row_y, &fallback_color, fallback_width,
        &configs, &sorted, design_ui,
    );
    *row_y += BLOCK_GAP;

    draw_add_row(ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y);
    *row_y += ADD_ROW_H;

    // Drawn LAST so it paints over the ladder/add-row beneath it and its
    // own rows, registered last, win the hit test there too ("last
    // registered wins" — same convention `draw_ladder`'s row-select rect
    // and the template footer dropdown already rely on).
    if design_ui.set_dropdown_open {
        draw_set_dropdown(ctx, toolbar_theme, result, set_sel_rect);
    }
}

// =============================================================================
// Набор
// =============================================================================

fn draw_set_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    design_ui: &DesignUiState,
) -> WidgetRect {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Набор", x0, y + SET_ROW_H / 2.0);

    let revert_w = 64.0;
    let sel_x = x0 + 52.0;
    let sel_w = ((x1 - revert_w - 8.0) - sel_x).clamp(80.0, 150.0);
    let sel_rect = WidgetRect::new(sel_x, y + 1.0, sel_w, SET_ROW_H - 2.0);

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rounded_rect(sel_rect.x, sel_rect.y, sel_rect.width, sel_rect.height, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(sel_rect.x, sel_rect.y, sel_rect.width, sel_rect.height, 4.0);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(design_ui.level_set.label(), sel_rect.x + 8.0, sel_rect.center_y());
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text("\u{25be}", sel_rect.right() - 8.0, sel_rect.center_y());
    result.content_items.push(("levels_set_select".to_string(), sel_rect));

    let revert_rect = WidgetRect::new(x1 - revert_w, y + 1.0, revert_w, SET_ROW_H - 2.0);
    let can_revert = design_ui.level_set != design_ui.last_preset;
    let revert_color = if can_revert { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(revert_rect.x, revert_rect.y, revert_rect.width, revert_rect.height, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(revert_color);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("вернуть", revert_rect.center_x(), revert_rect.center_y());
    if can_revert {
        result.content_items.push(("levels_set_revert".to_string(), revert_rect));
    }

    sel_rect
}

fn draw_set_dropdown(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    anchor: WidgetRect,
) {
    let menu_w = anchor.width.max(150.0);
    let menu_y = anchor.bottom() + 2.0;
    let total_h = SET_OPTION_H * LevelSet::PRESETS.len() as f64 + 6.0;

    ctx.set_fill_color(&toolbar_theme.dropdown_bg);
    ctx.fill_rounded_rect(anchor.x, menu_y, menu_w, total_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(anchor.x, menu_y, menu_w, total_h, 4.0);
    result.content_items.push((
        "levels_set_dropdown_menu".to_string(),
        WidgetRect::new(anchor.x, menu_y, menu_w, total_h),
    ));

    let mut row_y = menu_y + 3.0;
    for preset in LevelSet::PRESETS {
        ctx.set_font("11.5px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(preset.label(), anchor.x + 8.0, row_y + SET_OPTION_H / 2.0);
        result.content_items.push((
            format!("levels_set_option:{}", preset.id()),
            WidgetRect::new(anchor.x, row_y, menu_w, SET_OPTION_H),
        ));
        row_y += SET_OPTION_H;
    }
}

// =============================================================================
// The four palette cards + global fill-opacity slider
// =============================================================================

fn draw_ramps_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    design_ui: &DesignUiState,
) {
    let gap = 7.0;
    let card_w = ((x1 - x0) - gap * 3.0) / 4.0;
    for (i, palette) in PALETTES.iter().enumerate() {
        let rect = WidgetRect::new(x0 + i as f64 * (card_w + gap), y, card_w, RAMP_CARD_H);
        let selected = design_ui.palette == i;
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
        ctx.fill_text(palette.name, rect.center_x(), strip_y + strip_h + 10.0);

        result.content_items.push((format!("levels_palette:{i}"), rect));
    }
    // Restore left alignment for whatever draws next — `render_single_slider`
    // (called right after this by `draw_global_opacity_row`) never sets its
    // own text alignment before drawing its label, so leaving Center behind
    // clips that label's leading half against its own start x.
    ctx.set_text_align(TextAlign::Left);
}

fn draw_global_opacity_row(
    ctx: &mut dyn RenderContext,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    design_ui: &DesignUiState,
) {
    const FIELD: &str = "levels_global_opacity";
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let slider_cfg = SliderConfig::new(0.0, 1.0).without_input();
    let display = state
        .slider_drag
        .as_ref()
        .filter(|d| d.field_id == FIELD)
        .and_then(|d| d.floating_value)
        .unwrap_or(design_ui.global_fill_opacity);
    let hovered = state.slider_drag.as_ref().map(|d| d.field_id == FIELD).unwrap_or(false);
    let rect = WidgetRect::new(x0, y, (x1 - x0 - 60.0).max(80.0), GLOBAL_ROW_H);
    let res = render_single_slider(ctx, &slider_cfg, display, rect, "Заливка между", widget_theme, hovered, None);
    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&widget_theme.text_normal);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&format!("{:.0}%", display * 100.0), res.track_rect.right() + 10.0, y + GLOBAL_ROW_H / 2.0);
    result.content_items.push((FIELD.to_string(), res.full_rect));
    if let Some(t) = res.track_info {
        result.slider_tracks.push(SliderTrackInfo { field_id: FIELD.to_string(), track_x: t.track_x, track_width: t.track_width, min_val: 0.0, max_val: 1.0 });
    }
}

/// Recolour every non-overridden level in the composition from
/// `palette_idx`'s five stops, ranked by ascending level value. Palette
/// changes never touch which levels exist — only `apply_level_set` does —
/// per the spec's "набор и палитра — разные оси".
pub(crate) fn apply_palette(prim: &mut dyn Primitive, design_ui: &DesignUiState, palette_idx: usize) {
    let Some(mut configs) = prim.level_configs() else { return };
    let palette = &PALETTES[palette_idx.min(PALETTES.len().saturating_sub(1))];
    let mut sorted_vals = design_ui.composition.clone();
    sorted_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let denom = sorted_vals.len().saturating_sub(1).max(1) as f64;
    for cfg in configs.iter_mut() {
        let Some(rank) = sorted_vals.iter().position(|v| (*v - cfg.level).abs() < 1e-9) else { continue };
        if design_ui.is_overridden(cfg.level) {
            continue;
        }
        let t = rank as f64 / denom;
        let color = crate::palettes::palette_color_at(&palette.colors, t);
        cfg.color = Some(color.clone());
        cfg.fill_color = Some(color);
    }
    prim.set_level_configs(configs);
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
    design_ui: &DesignUiState,
) {
    if sorted_composition.is_empty() {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("Список пуст — добавьте уровень ниже.", x0, *row_y);
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
        let Some(cfg) = configs.iter().find(|c| (c.level - level).abs() < 1e-9) else { continue };
        let y = *row_y;
        let this_color = cfg.color.as_deref().unwrap_or(fallback_color);
        let next_color: Option<String> = sorted_composition
            .get(row_idx + 1)
            .and_then(|nv| configs.iter().find(|c| (c.level - nv).abs() < 1e-9))
            .map(|n| n.color.clone().unwrap_or_else(|| fallback_color.to_string()));

        let selected = design_ui.is_selected(level);
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
        draw_level_value_field(ctx, toolbar_theme, widget_theme, state, result, value_rect, &key, level);

        let bar_rect = WidgetRect::new(col3_x, y + (ROW_H - BAR_H) / 2.0, col3_w, BAR_H);
        draw_level_bar(ctx, bar_rect, this_color, next_color.as_deref(), cfg.visible);

        let color_chip = WidgetRect::new(col4_x, y + (ROW_H - CHIP) / 2.0, CHIP, CHIP);
        ctx.set_fill_color(this_color);
        ctx.fill_rounded_rect(color_chip.x, color_chip.y, color_chip.width, color_chip.height, 3.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(color_chip.x, color_chip.y, color_chip.width, color_chip.height, 3.0);
        result.content_items.push((format!("level_v{key}_color"), color_chip));

        let fill_color = cfg.fill_color.as_deref().unwrap_or(this_color);
        let fill_chip = WidgetRect::new(col4_x + CHIP + 5.0, y + (ROW_H - CHIP) / 2.0, CHIP, CHIP);
        ctx.set_fill_color(&mlc_core::apply_opacity(fill_color, if cfg.fill_enabled { cfg.fill_opacity.max(0.2) } else { 0.15 }));
        ctx.fill_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
        ctx.set_stroke_color(if cfg.fill_enabled { &toolbar_theme.item_bg_active } else { &toolbar_theme.separator });
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
        result.content_items.push((format!("level_v{key}_fill"), fill_chip));

        if design_ui.is_overridden(level) {
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
            let next_label = sorted_composition.get(row_idx + 1).map(|n| format!("{n:.3}"));
            draw_inspector(
                ctx, toolbar_theme, widget_theme, state, result, x0, x1, *row_y, &key, cfg,
                fallback_color, fallback_width, next_label,
            );
            *row_y += INSPECTOR_H;
        }
    }
}

fn draw_checkbox(ctx: &mut dyn RenderContext, rect: WidgetRect, checked: bool, toolbar_theme: &ToolbarTheme, off_bg: &str) {
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
fn draw_level_value_field(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    rect: WidgetRect,
    key: &str,
    level: f64,
) {
    let field_id = format!("level_v{key}_value");
    let is_editing = state.active_field_id.as_deref() == Some(field_id.as_str());
    let label = format!("{level:.3}");
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

fn draw_level_bar(ctx: &mut dyn RenderContext, rect: WidgetRect, this_color: &str, next_color: Option<&str>, visible: bool) {
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
// The inspector — line colour / width / style / fill toggle+colour+target /
// fill opacity, for the selected row.
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

    // Row 4: fill toggle + colour + "up to <next level>".
    draw_label(ctx, "Заливка", y);
    let toggle_rect = WidgetRect::new(control_x, y + (INSPECTOR_ROW_H - CHECK) / 2.0, CHECK, CHECK);
    draw_checkbox(ctx, toggle_rect, cfg.fill_enabled, toolbar_theme, &toolbar_theme.background.clone());
    result.content_items.push((format!("level_v{key}_fill_toggle"), toggle_rect));
    let fill_color = cfg.fill_color.as_deref().unwrap_or(this_color);
    let fill_chip = WidgetRect::new(toggle_rect.right() + 8.0, y + (INSPECTOR_ROW_H - CHIP) / 2.0, CHIP, CHIP);
    ctx.set_fill_color(&mlc_core::apply_opacity(fill_color, cfg.fill_opacity.max(0.2)));
    ctx.fill_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(fill_chip.x, fill_chip.y, fill_chip.width, fill_chip.height, 3.0);
    result.content_items.push((format!("level_v{key}_fill"), fill_chip));
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let target_label = next_label.map(|l| format!("до {l}")).unwrap_or_else(|| "—".to_string());
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

fn draw_style_segment(ctx: &mut dyn RenderContext, rect: WidgetRect, style_id: &str, selected: bool, toolbar_theme: &ToolbarTheme, off_bg: &str) {
    ctx.set_fill_color(if selected { &toolbar_theme.item_bg_active } else { off_bg });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 3.0);

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
    x1: f64,
    y: f64,
) {
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.accent);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    let mid_y = y + ADD_ROW_H / 2.0;
    ctx.fill_text("+", x0, mid_y);

    ctx.set_font("11.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.fill_text("добавить уровень", x0 + 14.0, mid_y);

    let field_id = "levels_add_value";
    let is_editing = state.active_field_id.as_deref() == Some(field_id);
    let ar = &state.active_render;
    let (display, cursor, sel_start, sel_end): (&str, usize, Option<usize>, Option<usize>) = if is_editing {
        (ar.text.as_str(), ar.cursor, ar.selection_start, Some(ar.cursor))
    } else {
        ("", 0, None, None)
    };
    let field_x = x0 + 14.0 + ctx.measure_text("добавить уровень") + 10.0;
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

    let common = ["1.272", "1.618", "2.618"];
    let mut chip_x = field_rect.right() + 8.0;
    for v in common {
        ctx.set_font("11px sans-serif");
        let w = ctx.measure_text(v) + 16.0;
        let chip_rect = WidgetRect::new(chip_x, y + (ADD_ROW_H - 20.0) / 2.0, w, 20.0);
        if chip_rect.right() > x1 {
            break;
        }
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(chip_rect.x, chip_rect.y, chip_rect.width, chip_rect.height, 10.0);
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(v, chip_rect.center_x(), chip_rect.center_y());
        result.content_items.push((format!("levels_add_chip:{v}"), chip_rect));
        chip_x += w + 6.0;
    }
}
