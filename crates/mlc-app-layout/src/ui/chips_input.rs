//! Harvest-ready chips input — the messenger concept's member picker
//! (`.chips`/`.chip`, New Group/New Channel dialogs): a row of removable
//! chips, each with a small avatar (reuses [`crate::ui::avatar_widget`],
//! not a private copy) and a label, wrapping onto new rows as needed.
//! Draw + hit-test split; no `ChartApp`/`BackendCore`/modal-state access —
//! a "chip" is caller-supplied label text + initials, never a `Member`.
//!
//! Harvest: uzor chips-input

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use crate::ui::avatar_widget::{draw_avatar, AvatarColors, AvatarShape, CornerMarker};

pub struct Chip<'a> {
    pub label: &'a str,
    /// Empty string draws no avatar for this chip.
    pub avatar_initials: &'a str,
}

pub struct ChipsInputColors<'a> {
    pub chip_background: &'a str,
    pub chip_border: &'a str,
    pub chip_text: &'a str,
    pub avatar_background: &'a str,
    /// The colour the avatar's own ring reads against — the chip's own
    /// background (see [`crate::ui::avatar_widget::AvatarColors::ring`]'s
    /// own doc).
    pub avatar_ring: &'a str,
}

const CHIP_H: f64 = 24.0;
const CHIP_GAP: f64 = 5.0;
const CHIP_PAD_H: f64 = 8.0;
const CHIP_REMOVE_W: f64 = 16.0;
const CHIP_AVATAR_GAP: f64 = 6.0;

/// One chip's own layout, returned alongside its paint so the caller can
/// register the remove button for click dispatch without re-deriving the
/// row-wrap math.
pub struct ChipLayout {
    pub chip_rect: WidgetRect,
    pub remove_rect: WidgetRect,
}

/// Lay out and paint every chip left-to-right, wrapping to a new row past
/// `max_w`. Returns each chip's own layout (draw order == `chips` order)
/// and the total height consumed, for the caller's own row bookkeeping.
pub fn draw_chips(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    max_w: f64,
    chips: &[Chip],
    colors: &ChipsInputColors,
) -> (Vec<ChipLayout>, f64) {
    let mut cx = x;
    let mut cy = y;
    let mut out = Vec::with_capacity(chips.len());
    for chip in chips {
        ctx.set_font("600 11px sans-serif");
        let label_w = ctx.measure_text(chip.label);
        let has_avatar = !chip.avatar_initials.is_empty();
        let avatar_w = if has_avatar { CHIP_H - 6.0 + CHIP_AVATAR_GAP } else { 0.0 };
        let chip_w = CHIP_PAD_H + avatar_w + label_w + 6.0 + CHIP_REMOVE_W;
        if cx + chip_w > x + max_w && cx > x {
            cx = x;
            cy += CHIP_H + CHIP_GAP;
        }
        let chip_rect = WidgetRect::new(cx, cy, chip_w, CHIP_H);
        ctx.set_fill_color(colors.chip_background);
        ctx.fill_rounded_rect(chip_rect.x, chip_rect.y, chip_rect.width, chip_rect.height, CHIP_H / 2.0);
        ctx.set_stroke_color(colors.chip_border);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(chip_rect.x, chip_rect.y, chip_rect.width, chip_rect.height, CHIP_H / 2.0);

        let mut label_x = chip_rect.x + CHIP_PAD_H;
        if has_avatar {
            let ar = (CHIP_H - 6.0) / 2.0;
            let acx = chip_rect.x + CHIP_PAD_H + ar;
            let acy = chip_rect.center_y();
            draw_avatar(
                ctx, acx, acy, ar, AvatarShape::Circle, chip.avatar_initials,
                &AvatarColors { background: colors.avatar_background, text: colors.chip_text, ring: colors.avatar_ring },
                CornerMarker::None,
            );
            label_x = acx + ar + CHIP_AVATAR_GAP;
        }

        ctx.set_font("600 11px sans-serif");
        ctx.set_fill_color(colors.chip_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(chip.label, label_x, chip_rect.center_y() + 0.5);

        let remove_rect = WidgetRect::new(chip_rect.x + chip_rect.width - CHIP_REMOVE_W, chip_rect.y, CHIP_REMOVE_W, CHIP_H);
        ctx.set_fill_color(colors.chip_text);
        ctx.set_text_align(TextAlign::Center);
        ctx.fill_text("x", remove_rect.center_x(), remove_rect.center_y() + 0.5);

        out.push(ChipLayout { chip_rect, remove_rect });
        cx += chip_w + CHIP_GAP;
    }
    let total_h = if chips.is_empty() { 0.0 } else { (cy - y) + CHIP_H };
    (out, total_h)
}

/// Which chip's remove button, if any, contains `(x, y)` — the index into
/// the SAME `chips`/`layout` slice `draw_chips` was called with.
pub fn hit_test_remove(layout: &[ChipLayout], x: f64, y: f64) -> Option<usize> {
    layout.iter().position(|c| {
        x >= c.remove_rect.x
            && x <= c.remove_rect.x + c.remove_rect.width
            && y >= c.remove_rect.y
            && y <= c.remove_rect.y + c.remove_rect.height
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_test_remove_finds_the_right_chip_by_index() {
        let layout = vec![
            ChipLayout { chip_rect: WidgetRect::new(0.0, 0.0, 60.0, 24.0), remove_rect: WidgetRect::new(44.0, 0.0, 16.0, 24.0) },
            ChipLayout { chip_rect: WidgetRect::new(65.0, 0.0, 60.0, 24.0), remove_rect: WidgetRect::new(109.0, 0.0, 16.0, 24.0) },
        ];
        assert_eq!(hit_test_remove(&layout, 50.0, 12.0), Some(0));
        assert_eq!(hit_test_remove(&layout, 115.0, 12.0), Some(1));
        assert_eq!(hit_test_remove(&layout, 30.0, 12.0), None);
    }
}
