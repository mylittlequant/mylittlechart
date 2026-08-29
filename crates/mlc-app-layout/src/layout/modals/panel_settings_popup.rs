//! Per-panel settings surface (owner spec 2026-07-28).
//!
//! Every STANDALONE workspace panel's overlay gear opens this generic
//! surface — as an anchored dropdown or a small centered modal, per the
//! persisted `panel_gear_opens_modal` preference (togglable from the
//! surface itself via the universal "Open as" row the caller prepends).
//! Content is data-driven ([`PanelSettingRow`]) so each panel kind supplies
//! its own toggles/choices without new render code. Container-hosted panels
//! never reach this — their gears are suppressed in favor of the trading
//! container's common modal.

use uzor::input::Sense;
use uzor::types::Rect as WidgetRect;

use crate::layout::render_chart::FrameTheme;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use mlc_core::engine::render::draw_svg_icon;
use mlc_core::engine::render::RenderContext;
use mlc_core::render::{TextAlign, TextBaseline};

// =============================================================================
// Data
// =============================================================================

/// One settings row.
#[derive(Clone, Debug)]
pub enum PanelSettingRow {
    /// Muted section caption.
    Header(String),
    /// On/off switch — widget `pset:toggle:{id}`.
    Toggle { id: String, label: String, on: bool },
    /// One-of-N option buttons — widgets `pset:opt:{id}:{idx}`.
    Choice { id: String, label: String, options: Vec<String>, active: usize },
}

/// The whole surface, flattened by the app layer.
#[derive(Clone, Debug, Default)]
pub struct PanelSettingsView {
    /// Panel display name (surface title).
    pub title: String,
    /// Rows top-to-bottom. The caller prepends the universal "Open as" row.
    pub rows: Vec<PanelSettingRow>,
    /// `true` = centered modal, `false` = dropdown anchored at `anchor`.
    pub as_modal: bool,
    /// Dropdown anchor (top-left, screen coords) — the gear's bottom edge.
    pub anchor: (f64, f64),
}

/// Hit geometry for the input layer.
#[derive(Clone, Debug, Default)]
pub struct PanelSettingsPopupResult {
    pub rect: WidgetRect,
}

// =============================================================================
// Layout constants
// =============================================================================

const POPUP_W: f64 = 240.0;
const TITLE_H: f64 = 30.0;
const ROW_H: f64 = 26.0;
const HEADER_H: f64 = 22.0;
const PAD: f64 = 10.0;
const CLOSE_SIZE: f64 = 16.0;

fn rows_height(rows: &[PanelSettingRow]) -> f64 {
    rows.iter().map(|r| match r {
        PanelSettingRow::Header(_) => HEADER_H,
        PanelSettingRow::Toggle { .. } => ROW_H,
        PanelSettingRow::Choice { .. } => ROW_H + 24.0,
    }).sum()
}

// =============================================================================
// Renderer
// =============================================================================

pub fn render_panel_settings_popup(
    ctx: &mut dyn RenderContext,
    view: &PanelSettingsView,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    screen_w: f64,
    screen_h: f64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> PanelSettingsPopupResult {
    let popup_h = TITLE_H + rows_height(&view.rows) + PAD;
    let (px, py) = if view.as_modal {
        (
            ((screen_w - POPUP_W) / 2.0).max(0.0),
            ((screen_h - popup_h) / 2.0).max(0.0),
        )
    } else {
        (
            view.anchor.0.min(screen_w - POPUP_W - 4.0).max(4.0),
            view.anchor.1.min(screen_h - popup_h - 4.0).max(4.0),
        )
    };
    let rect = WidgetRect::new(px, py, POPUP_W, popup_h);

    let layer_id = ZLayer::Modal.push_named(input_coordinator, "pset");
    input_coordinator.register_on_layer("pset:bg", rect, Sense::CLICK, &layer_id);

    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.modal_bg,
        &frame_theme.toolbar_border,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_active,
        &frame_theme.toolbar_border,
    );
    render_modal_frame_only(ctx, rect, &modal_theme, 0.0);

    // ── Title bar ────────────────────────────────────────────────────────
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(px, py, POPUP_W, TITLE_H);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&view.title, px + PAD, py + TITLE_H / 2.0);

    let close_x = px + POPUP_W - CLOSE_SIZE - 8.0;
    let close_y = py + (TITLE_H - CLOSE_SIZE) / 2.0;
    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, CLOSE_SIZE, CLOSE_SIZE, &toolbar_theme.item_text);
    input_coordinator.register_on_layer(
        "pset:close",
        WidgetRect::new(close_x, close_y, CLOSE_SIZE, CLOSE_SIZE),
        Sense::CLICK | Sense::HOVER,
        &layer_id,
    );

    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(px, py + TITLE_H);
    ctx.line_to(px + POPUP_W, py + TITLE_H);
    ctx.stroke();

    // ── Rows ─────────────────────────────────────────────────────────────
    let left = px + PAD;
    let mut row_y = py + TITLE_H + 4.0;
    for row in &view.rows {
        match row {
            PanelSettingRow::Header(text) => {
                ctx.set_font("10px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(text, left, row_y + HEADER_H / 2.0);
                row_y += HEADER_H;
            }
            PanelSettingRow::Toggle { id, label, on } => {
                let wid = format!("pset:toggle:{}", id);
                let hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
                if hovered {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.08));
                    ctx.fill_rounded_rect(px + 4.0, row_y, POPUP_W - 8.0, ROW_H, 3.0);
                }
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, left, row_y + ROW_H / 2.0);

                // Toggle pill, right-aligned.
                let pill_w = 28.0;
                let pill_h = 14.0;
                let pill_x = px + POPUP_W - PAD - pill_w;
                let pill_y = row_y + (ROW_H - pill_h) / 2.0;
                ctx.set_fill_color(if *on {
                    &toolbar_theme.accent
                } else {
                    &toolbar_theme.item_bg_hover
                });
                ctx.fill_rounded_rect(pill_x, pill_y, pill_w, pill_h, pill_h / 2.0);
                let knob = pill_h - 4.0;
                let knob_x = if *on { pill_x + pill_w - knob - 2.0 } else { pill_x + 2.0 };
                ctx.set_fill_color(&toolbar_theme.item_text_active);
                ctx.fill_rounded_rect(knob_x, pill_y + 2.0, knob, knob, knob / 2.0);

                input_coordinator.register_on_layer(
                    wid.as_str(),
                    WidgetRect::new(px + 4.0, row_y, POPUP_W - 8.0, ROW_H),
                    Sense::CLICK | Sense::HOVER,
                    &layer_id,
                );
                row_y += ROW_H;
            }
            PanelSettingRow::Choice { id, label, options, active } => {
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(label, left, row_y + ROW_H / 2.0);
                row_y += ROW_H - 4.0;

                let mut bx = left;
                for (i, opt) in options.iter().enumerate() {
                    ctx.set_font("10px sans-serif");
                    let opt_w = ctx.measure_text(opt) + 16.0;
                    let wid = format!("pset:opt:{}:{}", id, i);
                    let is_active = *active == i;
                    let hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
                    let bg = if is_active {
                        toolbar_theme.accent.clone()
                    } else if hovered {
                        toolbar_theme.item_bg_hover.clone()
                    } else {
                        mlc_core::apply_opacity(&toolbar_theme.item_text, 0.06)
                    };
                    ctx.set_fill_color(&bg);
                    ctx.fill_rounded_rect(bx, row_y, opt_w, 18.0, 3.0);
                    ctx.set_fill_color(if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(opt, bx + opt_w / 2.0, row_y + 9.0);
                    input_coordinator.register_on_layer(
                        wid.as_str(),
                        WidgetRect::new(bx, row_y, opt_w, 18.0),
                        Sense::CLICK | Sense::HOVER,
                        &layer_id,
                    );
                    bx += opt_w + 6.0;
                }
                row_y += 28.0;
            }
        }
    }

    input_coordinator.pop_layer(&layer_id);
    PanelSettingsPopupResult { rect }
}
