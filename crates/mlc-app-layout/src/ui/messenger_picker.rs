//! Composer pickers for the messenger — the emoji grid and the sticker grid
//! (program slice U3). One popover, two tabs, anchored above the composer's
//! emoji button; the sticker tab exists only when the provider reports
//! `Capabilities::stickers`.
//!
//! The concept (`messenger_concept_v1.html`) draws no picker, so the popover
//! reuses its `.menu` surface tokens exactly — `--menu` fill, 1px `--border`,
//! 8px radius, 4px padding, the `.menu .sep` rule and the `.ib` hover cell —
//! all through [`ToolbarTheme`] fields, no literal colours.
//!
//! Emoji ARE plain text (a click types the glyph into the draft field);
//! stickers are events (a click enqueues `MessengerIntent::SendSticker`).
//! Geometry lives in [`picker_layout`] so the draw pass and the tests read
//! one definition.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::state::toolbar_geometry::WidgetRect as ArtRect;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::ui::overlay_fill::fill_overlay_rounded;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_sticker, sticker_pack};

/// Which grid the composer popover shows. `CommunityState::messenger_picker`
/// holds `Some` of this while the popover is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerPickerTab {
    Emoji,
    Stickers,
}

/// The only sticker pack the client bundles today.
pub const STICKER_PACK: &str = "mlc-hat";

/// Popover width: 4px padding either side of eight 34px emoji cells.
pub const PICKER_W: f64 = 280.0;
/// Gap between the popover's bottom edge and the composer it opens from.
pub const PICKER_GAP: f64 = 8.0;

const PAD: f64 = 4.0;
const RADIUS: f64 = 8.0;
const TAB_H: f64 = 30.0;
const TAB_W: f64 = 84.0;
const SEP_H: f64 = 5.0;
const EMOJI_CELL: f64 = 34.0;
const EMOJI_COLS: usize = 8;
const EMOJI_ROWS: usize = 6;
const STICKER_COLS: usize = 3;
const STICKER_ROWS: usize = 2;
const STICKER_INSET: f64 = 6.0;
const CELL_RADIUS: f64 = 6.0;

/// The emoji grid, row-major — sized to fill `EMOJI_COLS x EMOJI_ROWS`
/// exactly. Trading-flavoured on purpose (the last three rows).
pub const EMOJI: [&str; EMOJI_COLS * EMOJI_ROWS] = [
    "😀", "😄", "😁", "😂", "🤣", "😊", "😍", "😘",
    "😎", "🤔", "😅", "😉", "🙂", "😐", "😢", "😭",
    "😡", "😱", "🙏", "👍", "👎", "👏", "🙌", "💪",
    "👀", "🤝", "🔥", "✨", "💯", "❤", "🎉", "🚀",
    "📈", "📉", "💰", "💎", "🐂", "🐻", "🎯", "⚡",
    "✅", "❌", "⚠", "🏆", "🧠", "🕐", "📌", "🔔",
];

/// The emoji at grid index `idx`, if any.
pub fn emoji_at(idx: usize) -> Option<&'static str> {
    EMOJI.get(idx).copied()
}

/// Whether a click on the composer-area widget `action` (the id suffix after
/// `cloud_overlay:dm:`) leaves the picker open. Everything else closes it —
/// the picker is a popover, not a modal.
pub fn keeps_picker_open(action: &str) -> bool {
    matches!(action, "emoji" | "picker_bg" | "draft_input")
        || action.starts_with("picker_tab:")
        || action.starts_with("emoji_pick:")
        || action.starts_with("sticker_pick:")
}

/// Every rect the popover draws and registers, computed once.
#[derive(Clone, Copy, Debug)]
pub struct PickerLayout {
    pub panel: WidgetRect,
    pub emoji_tab: WidgetRect,
    /// `None` when the provider cannot send stickers.
    pub sticker_tab: Option<WidgetRect>,
    pub grid: WidgetRect,
}

/// Popover geometry: right edge on `anchor_right`, bottom edge
/// [`PICKER_GAP`] above `anchor_top`, never left of `min_x`.
pub fn picker_layout(anchor_right: f64, anchor_top: f64, min_x: f64, has_stickers: bool) -> PickerLayout {
    let grid_h = EMOJI_CELL * EMOJI_ROWS as f64;
    let panel_h = PAD + TAB_H + SEP_H + grid_h + PAD;
    let panel_x = (anchor_right - PICKER_W).max(min_x);
    let panel_y = anchor_top - PICKER_GAP - panel_h;
    let tab_y = panel_y + PAD;
    let inner_x = panel_x + PAD;
    PickerLayout {
        panel: WidgetRect::new(panel_x, panel_y, PICKER_W, panel_h),
        emoji_tab: WidgetRect::new(inner_x, tab_y, TAB_W, TAB_H),
        sticker_tab: has_stickers.then(|| WidgetRect::new(inner_x + TAB_W, tab_y, TAB_W, TAB_H)),
        grid: WidgetRect::new(inner_x, tab_y + TAB_H + SEP_H, PICKER_W - PAD * 2.0, grid_h),
    }
}

fn emoji_cell_rect(grid: &WidgetRect, idx: usize) -> WidgetRect {
    let col = (idx % EMOJI_COLS) as f64;
    let row = (idx / EMOJI_COLS) as f64;
    WidgetRect::new(grid.x + col * EMOJI_CELL, grid.y + row * EMOJI_CELL, EMOJI_CELL, EMOJI_CELL)
}

fn sticker_cell_rect(grid: &WidgetRect, idx: usize) -> WidgetRect {
    let cell_w = grid.width / STICKER_COLS as f64;
    let cell_h = grid.height / STICKER_ROWS as f64;
    let col = (idx % STICKER_COLS) as f64;
    let row = (idx / STICKER_COLS) as f64;
    WidgetRect::new(grid.x + col * cell_w, grid.y + row * cell_h, cell_w, cell_h)
}

/// What the popover shows this frame.
#[derive(Clone, Copy, Debug)]
pub struct PickerView {
    pub tab: ComposerPickerTab,
    /// Provider reports `Capabilities::stickers`.
    pub has_stickers: bool,
    /// Monotonic milliseconds driving the sticker animation frames.
    pub elapsed_ms: u64,
}

/// Paint the popover and register its hit zones. Cell ids:
/// `cloud_overlay:dm:emoji_pick:{idx}`,
/// `cloud_overlay:dm:sticker_pick:{pack}:{id}`, tabs
/// `cloud_overlay:dm:picker_tab:{emoji|stickers}`.
pub fn draw_picker(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    layout: &PickerLayout,
    view: &PickerView,
    tt: &ToolbarTheme,
) {
    let p = layout.panel;
    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rounded_rect(p.x + 3.0, p.y + 3.0, p.width, p.height, RADIUS);
    fill_overlay_rounded(ctx, &tt.dropdown_bg, p.x, p.y, p.width, p.height, RADIUS);
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(p.x, p.y, p.width, p.height, RADIUS);
    // Registered FIRST so every cell below overrides it: a click on the
    // popover's own padding must not read as "clicked elsewhere".
    coord.register_on_layer("cloud_overlay:dm:picker_bg", p, Sense::CLICK | Sense::HOVER, layer_id);

    draw_tab(ctx, coord, layer_id, layout.emoji_tab, "emoji", "Emoji", view.tab == ComposerPickerTab::Emoji, tt);
    if let Some(rect) = layout.sticker_tab {
        draw_tab(ctx, coord, layer_id, rect, "stickers", "Stickers", view.tab == ComposerPickerTab::Stickers, tt);
    }

    let sep_y = layout.grid.y - SEP_H / 2.0;
    ctx.set_stroke_color(&tt.divider_soft);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(p.x + PAD - 1.0, sep_y);
    ctx.line_to(p.x + p.width - PAD + 1.0, sep_y);
    ctx.stroke();

    match view.tab {
        ComposerPickerTab::Emoji => draw_emoji_grid(ctx, coord, layer_id, &layout.grid, tt),
        ComposerPickerTab::Stickers => draw_sticker_grid(ctx, coord, layer_id, &layout.grid, view.elapsed_ms, tt),
    }
}

fn draw_tab(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    rect: WidgetRect,
    key: &str,
    label: &str,
    active: bool,
    tt: &ToolbarTheme,
) {
    let id = format!("cloud_overlay:dm:picker_tab:{key}");
    if !active && coord.is_hovered(&WidgetId::from(id.as_str())) {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 5.0);
    }
    ctx.set_font("600 11.5px sans-serif");
    ctx.set_fill_color(if active { &tt.accent } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    if active {
        ctx.set_fill_color(&tt.accent);
        ctx.fill_rect(rect.x + 10.0, rect.y + rect.height - 2.0, rect.width - 20.0, 2.0);
    }
    coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
}

fn draw_emoji_grid(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    grid: &WidgetRect,
    tt: &ToolbarTheme,
) {
    ctx.set_font("20px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    for (idx, glyph) in EMOJI.iter().enumerate() {
        let rect = emoji_cell_rect(grid, idx);
        let id = format!("cloud_overlay:dm:emoji_pick:{idx}");
        if coord.is_hovered(&WidgetId::from(id.as_str())) {
            ctx.set_fill_color(&tt.item_bg_hover);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, CELL_RADIUS);
        }
        ctx.set_fill_color(&tt.item_text);
        ctx.fill_text(glyph, rect.center_x(), rect.center_y() + 1.0);
        coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    }
}

fn draw_sticker_grid(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    grid: &WidgetRect,
    elapsed_ms: u64,
    tt: &ToolbarTheme,
) {
    for (idx, info) in sticker_pack(STICKER_PACK).iter().enumerate() {
        let rect = sticker_cell_rect(grid, idx);
        let id = format!("cloud_overlay:dm:sticker_pick:{STICKER_PACK}:{}", info.id);
        if coord.is_hovered(&WidgetId::from(id.as_str())) {
            ctx.set_fill_color(&tt.item_bg_hover);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, CELL_RADIUS);
        }
        let art = ArtRect::new(
            rect.x + STICKER_INSET,
            rect.y + STICKER_INSET,
            rect.width - STICKER_INSET * 2.0,
            rect.height - STICKER_INSET * 2.0,
        );
        if !draw_sticker(ctx, art, STICKER_PACK, info.id, elapsed_ms) {
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(info.alt, rect.center_x(), rect.center_y());
        }
        coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_table_has_no_duplicates_and_no_blank_cell() {
        for (i, a) in EMOJI.iter().enumerate() {
            assert!(!a.is_empty(), "cell {i} is blank");
            assert!(EMOJI.iter().skip(i + 1).all(|b| b != a), "{a} appears twice");
        }
    }

    #[test]
    fn emoji_at_is_bounded_by_the_table() {
        assert_eq!(emoji_at(0), Some("😀"));
        assert_eq!(emoji_at(EMOJI.len()), None);
    }

    #[test]
    fn the_sticker_pack_fits_its_grid() {
        assert!(sticker_pack(STICKER_PACK).len() <= STICKER_COLS * STICKER_ROWS);
    }

    #[test]
    fn panel_hangs_above_the_anchor_with_its_right_edge_on_the_anchor() {
        let l = picker_layout(900.0, 700.0, 300.0, true);
        assert_eq!(l.panel.x + l.panel.width, 900.0);
        assert_eq!(l.panel.y + l.panel.height, 700.0 - PICKER_GAP);
    }

    #[test]
    fn panel_never_crosses_the_left_bound() {
        let l = picker_layout(320.0, 700.0, 300.0, true);
        assert_eq!(l.panel.x, 300.0);
    }

    #[test]
    fn the_sticker_tab_exists_only_with_the_capability() {
        assert!(picker_layout(900.0, 700.0, 0.0, true).sticker_tab.is_some());
        assert!(picker_layout(900.0, 700.0, 0.0, false).sticker_tab.is_none());
    }

    #[test]
    fn grid_cells_stay_inside_the_grid() {
        let l = picker_layout(900.0, 700.0, 0.0, true);
        let last = emoji_cell_rect(&l.grid, EMOJI.len() - 1);
        assert!(last.x + last.width <= l.grid.x + l.grid.width + 1e-9);
        assert!(last.y + last.height <= l.grid.y + l.grid.height + 1e-9);
        let last_sticker = sticker_cell_rect(&l.grid, STICKER_COLS * STICKER_ROWS - 1);
        assert!(last_sticker.x + last_sticker.width <= l.grid.x + l.grid.width + 1e-9);
        assert!(last_sticker.y + last_sticker.height <= l.grid.y + l.grid.height + 1e-9);
    }

    #[test]
    fn only_picker_owned_widgets_keep_the_popover_open() {
        for keep in ["emoji", "picker_bg", "draft_input", "picker_tab:stickers", "emoji_pick:3", "sticker_pick:mlc-hat:wave"] {
            assert!(keeps_picker_open(keep), "{keep}");
        }
        for close in ["send", "back", "conversation:4", "attach", "load_older"] {
            assert!(!keeps_picker_open(close), "{close}");
        }
    }
}
