//! Harvest-ready on/off toggle switch — the messenger concept's pill
//! switch (`.tog`/`.tog.on`, e.g. "Members can add people" in the New
//! Group dialog). Grepped `mlc-app-layout/src/ui` first, per the plan's
//! own §2 verdict: every existing on/off control in this crate is a
//! square checkbox (`ui::overlay_fill::fill_checkbox`, used by every
//! `primitive_settings_*`/`chart_settings`/`indicator_settings` row) —
//! none of that shape. This is new, not a duplicate.
//!
//! Harvest: uzor toggle
//!
//! Draw + hit-test split (a toggle carries no state of its own beyond the
//! caller's plain `bool` — no separate state type is needed). No
//! `ChartApp`/`BackendCore`/modal-state access and no MLC domain types in
//! its signatures.

use mlc_core::engine::render::RenderContext;
use uzor::types::Rect as WidgetRect;

/// Natural `(width, height)` for a toggle whose TRACK height is `h` — the
/// concept's own `.tog` proportions (30×17, roughly 1.75:1).
pub fn track_size(h: f64) -> (f64, f64) {
    (h * 30.0 / 17.0, h)
}

/// Gap between the track's own edge and the thumb circle's edge.
const THUMB_INSET: f64 = 2.0;

pub struct ToggleColors<'a> {
    pub track_off: &'a str,
    pub track_on: &'a str,
    pub thumb_off: &'a str,
    pub thumb_on: &'a str,
}

/// Paint the toggle at `rect` in state `on`.
pub fn draw_toggle(ctx: &mut dyn RenderContext, rect: WidgetRect, on: bool, colors: &ToggleColors) {
    let r = rect.height / 2.0;
    ctx.set_fill_color(if on { colors.track_on } else { colors.track_off });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, r);
    let thumb_r = (r - THUMB_INSET).max(1.0);
    let cy = rect.y + r;
    let cx = if on { rect.x + rect.width - r } else { rect.x + r };
    ctx.set_fill_color(if on { colors.thumb_on } else { colors.thumb_off });
    ctx.begin_path();
    ctx.arc(cx, cy, thumb_r, 0.0, std::f64::consts::TAU);
    ctx.fill();
}

/// Whether `(x, y)` falls inside the toggle's own clickable bounds — a
/// plain rect containment check, factored out so a caller never has to
/// re-derive "is this widget's own bounds" by hand at the click-dispatch
/// site.
pub fn hit_test(rect: WidgetRect, x: f64, y: f64) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_size_keeps_the_concepts_own_proportions() {
        let (w, h) = track_size(17.0);
        assert_eq!(h, 17.0);
        assert!((w - 30.0).abs() < 0.1);
    }

    #[test]
    fn hit_test_is_a_plain_rect_containment_check() {
        let rect = WidgetRect::new(10.0, 10.0, 30.0, 17.0);
        assert!(hit_test(rect, 20.0, 15.0));
        assert!(!hit_test(rect, 5.0, 15.0));
        assert!(!hit_test(rect, 20.0, 30.0));
    }
}
