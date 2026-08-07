//! Sector C's design: template creation and management — a dedicated
//! surface with no tab strip of its own (one purpose, not five; see the
//! coordinator's 2026-08-07 layout direction: "if it needs its own
//! sector, make it a third").
//!
//! Reuses the exact model the footer's own template dropdown already uses
//! everywhere in this app — `mlc_core::templates::PrimitiveTemplate`,
//! `from_primitive_data` / `apply_to_primitive_data` — this is a bigger
//! SURFACE for the same data, not a parallel template system. Rename is
//! the one capability that dropdown never had; it is real, typed keyboard
//! input through the same shared inline-edit buffer every other sub-field
//! in this modal already uses (`crate::dispatch::start_editing_raw`), not
//! a faked control.

use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::layout::toolbar_state::ToolbarState;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType, ModalTab, WidgetState, WidgetTheme};
use mlc_app_layout::ui::Icon;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::engine::render::draw_svg_icon;
use mlc_core::i18n::{t_settings, SettingsKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::templates::PrimitiveTemplate;
use uzor::input::InputCoordinator;
use uzor::types::Rect as WidgetRect;

use crate::design_state::DesignUiState;
use crate::shell_common;

const BLOCK_GAP: f64 = 10.0;
const CREATE_ROW_H: f64 = 34.0;
const ROW_GAP: f64 = 8.0;
const ROW_H: f64 = 58.0;
const SWATCH_W: f64 = 46.0;
const SWATCH_H: f64 = 30.0;
const ICON_BTN: f64 = 20.0;
const ICON_GAP: f64 = 6.0;
const EMPTY_H: f64 = 30.0;

#[allow(clippy::too_many_arguments)]
pub fn render_templates_panel(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    default_modal_y: f64,
    state: &PrimitiveSettingsState,
    primitives: &[Box<dyn Primitive>],
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    _toolbar_state: &ToolbarState,
    _current_time_ms: u64,
    coord: &mut InputCoordinator,
    templates: &[PrimitiveTemplate],
    _design_ui: &mut DesignUiState,
) -> PrimitiveSettingsResult {
    let mut result = PrimitiveSettingsResult::default();
    let Some(prim_id) = state.primitive_id else { return result };
    let Some(prim) = primitives.iter().find(|p| p.data().id == prim_id) else { return result };

    let n = templates.len();
    let list_h = if n == 0 { EMPTY_H } else { n as f64 * ROW_H + n.saturating_sub(1) as f64 * ROW_GAP };
    let content_height = CREATE_ROW_H + BLOCK_GAP + list_h;

    let no_tabs: [ModalTab<'_>; 0] = [];
    let body = shell_common::begin_tab_body_shell(
        ctx, coord, screen_w, screen_h, default_modal_y, state, prim.display_name(), &no_tabs, 0, frame_theme,
        toolbar_theme, content_height, &mut result,
    );

    let x0 = body.content_left;
    let x1 = body.content_right;
    let mut row_y = body.row_y;

    draw_create_row(ctx, toolbar_theme, &mut result, x0, x1, row_y);
    row_y += CREATE_ROW_H + BLOCK_GAP;

    if templates.is_empty() {
        draw_empty_state(ctx, toolbar_theme, x0, row_y);
    } else {
        for tmpl in templates {
            draw_template_row(ctx, toolbar_theme, &body.widget_theme, state, &mut result, x0, x1, row_y, tmpl);
            row_y += ROW_H + ROW_GAP;
        }
    }

    shell_common::end_tab_body_shell(ctx, coord, body, &mut result, content_height, state, templates, toolbar_theme);

    result
}

/// The "create a template from the current settings" affordance — a full-
/// width button, not a hidden footer-menu entry. Reuses the exact same
/// footer-menu string (`SaveAsTemplate`) since it does the same thing:
/// `crate::dispatch`'s handler names and immediately opens the new
/// template's name for editing, so this call is the ONE place that promise
/// is actually kept (the footer menu's own "Save as…" never opened a name
/// prompt at all — it just appended "Template N").
fn draw_create_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
) {
    let rect = WidgetRect::new(x0, y, (x1 - x0).max(0.0), CREATE_ROW_H);
    ctx.set_fill_color(&toolbar_theme.button_bg);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);

    let icon_size = 16.0;
    let icon_x = rect.x + 10.0;
    let icon_y = rect.center_y() - icon_size / 2.0;
    draw_svg_icon(ctx, Icon::Plus.svg(), icon_x, icon_y, icon_size, icon_size, &toolbar_theme.item_text);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::SaveAsTemplate), icon_x + icon_size + 8.0, rect.center_y());

    result.content_items.push(("templates_panel:create".to_string(), rect));
}

fn draw_empty_state(ctx: &mut dyn RenderContext, toolbar_theme: &ToolbarTheme, x: f64, y: f64) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(t_settings(SettingsKey::NoTemplates), x, y);
}

/// One template: a swatch that shows what it actually applies (colour,
/// width, dash style, and — when it carries levels — a strip of their
/// colours), an editable name, a one-line caption, and three actions.
/// Clicking anywhere on the row's own background applies it (same gesture
/// the footer dropdown's `template_option:*` row already uses); the pencil
/// renames in place; the trash deletes, unconfirmed — same as the footer
/// dropdown's own `template_delete:*`.
#[allow(clippy::too_many_arguments)]
fn draw_template_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    widget_theme: &WidgetTheme,
    state: &PrimitiveSettingsState,
    result: &mut PrimitiveSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    tmpl: &PrimitiveTemplate,
) {
    let row_rect = WidgetRect::new(x0, y, (x1 - x0).max(0.0), ROW_H);

    // Row background applies the template on click — registered FIRST so
    // the smaller icon rects registered below win the hit test on their
    // own area ("last registered wins", same convention `draw_ladder`'s
    // row-select rect uses in `crate::levels_variants`).
    result.content_items.push((format!("templates_panel:apply:{}", tmpl.id), row_rect));

    ctx.set_fill_color(&toolbar_theme.background);
    ctx.fill_rounded_rect(row_rect.x, row_rect.y, row_rect.width, row_rect.height, 5.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(row_rect.x, row_rect.y, row_rect.width, row_rect.height, 5.0);

    let swatch = WidgetRect::new(row_rect.x + 8.0, row_rect.y + (ROW_H - SWATCH_H) / 2.0, SWATCH_W, SWATCH_H);
    draw_template_swatch(ctx, swatch, tmpl);

    let icons_w = 3.0 * ICON_BTN + 2.0 * ICON_GAP + 8.0;
    let text_x = swatch.right() + 10.0;
    let text_w = (row_rect.right() - icons_w - text_x).max(20.0);

    let editing_field = format!("template_name:{}", tmpl.id);
    let is_editing = state.active_field_id.as_deref() == Some(editing_field.as_str());
    if is_editing {
        let ar = &state.active_render;
        let field_rect = WidgetRect::new(text_x, row_rect.y + 8.0, text_w, 22.0);
        let cfg = InputConfig::new(ar.text.as_str())
            .with_focused(true)
            .with_type(InputType::Text)
            .with_font_size(12.0)
            .with_padding(4.0)
            .with_radius(3.0)
            .with_cursor(ar.cursor)
            .with_selection(ar.selection_start, Some(ar.cursor));
        let res = draw_input(ctx, &cfg, WidgetState::Normal, field_rect, widget_theme);
        draw_input_cursor(ctx, res.cursor_x, res.cursor_y, res.cursor_height, &toolbar_theme.item_text);
        result.active_input_char_positions = res.char_x_positions;
        result.active_input_rect = Some(field_rect);
        result.content_items.push((editing_field, field_rect));
    } else {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&tmpl.name, text_x, row_rect.y + 8.0);
    }

    let caption = template_caption(tmpl);
    ctx.set_font("10.5px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Bottom);
    ctx.fill_text(&caption, text_x, row_rect.bottom() - 8.0);

    // Icon buttons — right-aligned, registered LAST so they win over the
    // row-background apply target they sit on top of. Chevron is a purely
    // decorative "this row applies" hint; the row background already
    // carries the click.
    let icon_y = row_rect.center_y() - ICON_BTN / 2.0;
    let mut icon_x = row_rect.right() - 8.0 - ICON_BTN;

    let del_rect = WidgetRect::new(icon_x, icon_y, ICON_BTN, ICON_BTN);
    draw_svg_icon(ctx, Icon::Delete.svg(), del_rect.x, del_rect.y, ICON_BTN, ICON_BTN, &toolbar_theme.item_text_muted);
    result.content_items.push((format!("templates_panel:delete:{}", tmpl.id), del_rect));
    icon_x -= ICON_BTN + ICON_GAP;

    let rename_rect = WidgetRect::new(icon_x, icon_y, ICON_BTN, ICON_BTN);
    draw_svg_icon(ctx, Icon::Pencil.svg(), rename_rect.x, rename_rect.y, ICON_BTN, ICON_BTN, &toolbar_theme.item_text_muted);
    result.content_items.push((format!("templates_panel:rename:{}", tmpl.id), rename_rect));
    icon_x -= ICON_BTN + ICON_GAP;

    draw_svg_icon(ctx, Icon::ChevronRight.svg(), icon_x, icon_y, ICON_BTN, ICON_BTN, &toolbar_theme.accent);
}

fn draw_template_swatch(ctx: &mut dyn RenderContext, rect: WidgetRect, tmpl: &PrimitiveTemplate) {
    ctx.set_fill_color(&mlc_core::apply_opacity(&tmpl.color.stroke, 0.10));
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 4.0);

    let has_levels = tmpl.has_levels();
    let line_y = if has_levels { rect.y + rect.height * 0.32 } else { rect.center_y() };
    let inset = 6.0;
    ctx.set_stroke_color(&tmpl.color.stroke);
    ctx.set_stroke_width(tmpl.width.clamp(1.0, 4.0));
    let dash = tmpl.style.dash_pattern(tmpl.width.max(1.0));
    ctx.set_line_dash(&dash);
    ctx.begin_path();
    ctx.move_to(rect.x + inset, line_y);
    ctx.line_to(rect.right() - inset, line_y);
    ctx.stroke();
    ctx.set_line_dash(&[]);

    if has_levels {
        let strip_y = rect.y + rect.height * 0.55;
        let strip_h = (rect.bottom() - 4.0 - strip_y).max(2.0);
        let n = tmpl.level_configs.len().max(1);
        let seg_w = (rect.width - inset * 2.0) / n as f64;
        for (i, lvl) in tmpl.level_configs.iter().enumerate() {
            let color = lvl.color.as_deref().unwrap_or(&tmpl.color.stroke);
            ctx.set_fill_color(color);
            ctx.fill_rect(rect.x + inset + seg_w * i as f64, strip_y, seg_w, strip_h);
        }
    }
}

/// Width / style / level-count summary — short, ad hoc, lab-only text
/// (same precedent as the row labels in `crate::levels_variants`'s
/// inspector: no shipped i18n key exists for this exact string, and this
/// crate never ships to the product, only to the owner's own comparison
/// harness).
fn template_caption(tmpl: &PrimitiveTemplate) -> String {
    let mut caption = format!("{:.1}px, {}", tmpl.width, style_label_ru(tmpl.style.as_str()));
    if tmpl.has_levels() {
        caption.push_str(&format!(", {} ур.", tmpl.level_configs.len()));
    }
    caption
}

fn style_label_ru(style_id: &str) -> &'static str {
    match style_id {
        "dashed" => "пунктир",
        "dotted" => "точки",
        "large_dashed" => "штрих",
        "sparse_dotted" => "редкие точки",
        _ => "сплошная",
    }
}
