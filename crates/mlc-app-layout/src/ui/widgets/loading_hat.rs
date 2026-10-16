//! Shared "magic hat" loading spinner — owner order 2026-09-24: every
//! loading spinner in the app (native vello shell and wasm web shell share
//! this one renderer) becomes the same spinning magic hat the web boot
//! splash already shows, replacing the old rotating-arc ring.
//!
//! Frames are pre-baked static SVGs — `assets/mascot/spinner/
//! hat_turntable_00.svg` … `_15.svg`, one 150ms step of a 2.4s turntable
//! loop — generated from the animated source (`assets/mascot/spinner/
//! boot-hat-source.svg`) by `assets/mascot/spinner/build_frames.py` into
//! the SVG dialect `uzor`'s hand-written `draw_svg_multicolor` parser
//! actually renders (no `<use>`, no per-element `transform`, no
//! `radialGradient`/`objectBoundingBox` gradients — see that script's own
//! doc comment for the full parser-support walk). The hat carries its own
//! baked art colors, same as the toolbar's mirrored hat
//! (`ui::toolbar_core::draw_hat_mirrored`) — no theme colors involved.

use mlc_core::state::toolbar_geometry::WidgetRect;
use uzor::render::{draw_svg_multicolor, RenderContext};

/// Native aspect ratio of every turntable frame (`viewBox="0 0 240 180"`).
const FRAME_ASPECT: f64 = 240.0 / 180.0;

const FRAME_COUNT: usize = 16;
const MS_PER_FRAME: u64 = 150;

const FRAMES: [&str; FRAME_COUNT] = [
    include_str!("../../../assets/mascot/spinner/hat_turntable_00.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_01.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_02.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_03.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_04.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_05.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_06.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_07.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_08.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_09.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_10.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_11.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_12.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_13.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_14.svg"),
    include_str!("../../../assets/mascot/spinner/hat_turntable_15.svg"),
];

/// Frame index (`0..16`) for `elapsed_ms` into the loop — 150ms per frame,
/// wraps every 2.4s. `elapsed_ms` may be any monotonic millisecond clock
/// reading (wall-clock `now_ms` works exactly as well as "time since the
/// spinner appeared" — the animation is a pure period-2400ms loop, so only
/// `elapsed_ms % 2400` ever matters), which keeps every call site stateless.
fn hat_frame_index(elapsed_ms: u64) -> usize {
    ((elapsed_ms / MS_PER_FRAME) % FRAME_COUNT as u64) as usize
}

/// Draw the spinning magic-hat loading indicator, fitted and centred in
/// `rect` at the frames' native 240:180 aspect ratio.
///
/// Callers keep animating it exactly while it is visible by re-deriving
/// `elapsed_ms` from their own redraw pump each frame (same contract the
/// ring spinner this replaces already had) — this function is otherwise
/// stateless (SWC: no timer, no cached frame).
pub fn draw_loading_hat(ctx: &mut dyn RenderContext, rect: WidgetRect, elapsed_ms: u64) {
    let frame = FRAMES[hat_frame_index(elapsed_ms)];
    let (w, h) = if rect.width / rect.height > FRAME_ASPECT {
        (rect.height * FRAME_ASPECT, rect.height)
    } else {
        (rect.width, rect.width / FRAME_ASPECT)
    };
    let x = rect.x + (rect.width - w) / 2.0;
    let y = rect.y + (rect.height - h) / 2.0;
    draw_svg_multicolor(ctx, frame, x, y, w, h);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_index_wraps_at_the_2_4s_loop() {
        assert_eq!(hat_frame_index(0), 0);
        assert_eq!(hat_frame_index(149), 0);
        assert_eq!(hat_frame_index(150), 1);
        assert_eq!(hat_frame_index(2_399), 15);
        assert_eq!(hat_frame_index(2_400), 0);
        assert_eq!(hat_frame_index(2_400 + 150), 1);
    }

    #[test]
    fn every_frame_svg_parses_to_a_non_empty_viewbox() {
        for (i, svg) in FRAMES.iter().enumerate() {
            let vb = svg
                .split("viewBox=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_default();
            assert!(!vb.is_empty(), "frame {i} has no viewBox");
            let dims: Vec<f64> = vb.split_whitespace().filter_map(|s| s.parse().ok()).collect();
            assert_eq!(dims.len(), 4, "frame {i} viewBox malformed: {vb:?}");
            assert!(dims[2] > 0.0 && dims[3] > 0.0, "frame {i} viewBox has zero size: {vb:?}");
        }
    }
}
