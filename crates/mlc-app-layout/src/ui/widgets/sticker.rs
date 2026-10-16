//! Messenger sticker pack `mlc-hat` — first sticker pack for `m.sticker`
//! events (`url mlc-sticker://mlc-hat/<id>`, resolved from bundled assets
//! and never fetched; the messenger wiring itself is a separate, later
//! task — this module only ships the drawable pack).
//!
//! Frames are pre-baked static SVGs — `assets/mascot/stickers/mlc-hat/
//! <id>/frame_00.svg` … `frame_15.svg`, generated from the animated
//! sources (`assets/mascot/stickers/mlc-hat/src/*.svg`) by that same
//! directory's `build_frames.py` into the SVG dialect `uzor`'s hand-
//! written `draw_svg_multicolor` parser actually renders (no `<use>`, no
//! per-element `transform`, no `radialGradient`/`objectBoundingBox`
//! gradients, no `<ellipse>` — see that script's own doc comment for the
//! full parser-support walk; it is the same dialect
//! [`super::loading_hat`]'s spinner frames already target). Every sticker
//! bakes to 16 frames; only the per-frame duration differs (each source's
//! own natural loop length divided by 16 — `pack.json` in that same
//! directory is the canonical record, this table mirrors it).
//!
//! Baked-frame `viewBox` differs per sticker (`turntable`'s cel frames are
//! 240x180; every other sticker is square 512x512) — [`draw_sticker`]
//! reads it off the frame text itself at draw time rather than assuming
//! one fixed aspect ratio ([`loading_hat`](super::loading_hat) can assume
//! a single fixed aspect because it only ever draws one animation).

use mlc_core::state::toolbar_geometry::WidgetRect;
use uzor::render::{draw_svg_multicolor, RenderContext};

/// One sticker's picker-facing metadata — no frame data (callers draw
/// frames only through [`draw_sticker`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StickerInfo {
    pub id: &'static str,
    pub alt: &'static str,
}

struct StickerDef {
    info: StickerInfo,
    frame_ms: u64,
    frames: &'static [&'static str],
}

const STICKERS: &[StickerDef] = &[
    StickerDef {
        info: StickerInfo { id: "bounce", alt: "Bounce" },
        frame_ms: 150,
        frames: &[
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_00.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_01.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_02.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_03.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_04.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_05.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_06.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_07.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_08.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_09.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_10.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_11.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_12.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_13.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_14.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/bounce/frame_15.svg"),
        ],
    },
    StickerDef {
        info: StickerInfo { id: "spin", alt: "Spin" },
        frame_ms: 150,
        frames: &[
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_00.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_01.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_02.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_03.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_04.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_05.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_06.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_07.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_08.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_09.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_10.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_11.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_12.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_13.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_14.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/spin/frame_15.svg"),
        ],
    },
    StickerDef {
        info: StickerInfo { id: "wave", alt: "Wave" },
        frame_ms: 150,
        frames: &[
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_00.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_01.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_02.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_03.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_04.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_05.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_06.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_07.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_08.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_09.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_10.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_11.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_12.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_13.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_14.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/wave/frame_15.svg"),
        ],
    },
    StickerDef {
        info: StickerInfo { id: "twinkle", alt: "Twinkle" },
        frame_ms: 175,
        frames: &[
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_00.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_01.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_02.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_03.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_04.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_05.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_06.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_07.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_08.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_09.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_10.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_11.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_12.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_13.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_14.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/twinkle/frame_15.svg"),
        ],
    },
    StickerDef {
        info: StickerInfo { id: "flip", alt: "Flip" },
        frame_ms: 200,
        frames: &[
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_00.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_01.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_02.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_03.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_04.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_05.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_06.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_07.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_08.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_09.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_10.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_11.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_12.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_13.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_14.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/flip/frame_15.svg"),
        ],
    },
    StickerDef {
        info: StickerInfo { id: "turntable", alt: "Turntable" },
        frame_ms: 150,
        frames: &[
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_00.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_01.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_02.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_03.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_04.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_05.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_06.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_07.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_08.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_09.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_10.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_11.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_12.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_13.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_14.svg"),
            include_str!("../../../assets/mascot/stickers/mlc-hat/turntable/frame_15.svg"),
        ],
    },
];

/// Picker-facing metadata for the `"mlc-hat"` pack, in the same order as
/// [`STICKERS`] — kept in sync manually (each is a plain 2-field literal);
/// the `sticker_pack_info_matches_the_frame_table` test below guards
/// against the two drifting apart.
const MLC_HAT_INFO: [StickerInfo; 6] = [
    StickerInfo { id: "bounce", alt: "Bounce" },
    StickerInfo { id: "spin", alt: "Spin" },
    StickerInfo { id: "wave", alt: "Wave" },
    StickerInfo { id: "twinkle", alt: "Twinkle" },
    StickerInfo { id: "flip", alt: "Flip" },
    StickerInfo { id: "turntable", alt: "Turntable" },
];

/// Every sticker in `pack` for a picker UI — currently only `"mlc-hat"`
/// exists; any other name returns an empty slice (same "unknown name,
/// caller handles the empty/false case" contract [`draw_sticker`] uses).
pub fn sticker_pack(pack: &str) -> &'static [StickerInfo] {
    if pack == "mlc-hat" {
        &MLC_HAT_INFO
    } else {
        &[]
    }
}

fn find_sticker(pack: &str, id: &str) -> Option<&'static StickerDef> {
    if pack != "mlc-hat" {
        return None;
    }
    STICKERS.iter().find(|s| s.info.id == id)
}

/// Frame index (`0..frames.len()`) for `elapsed_ms` into `def`'s own loop
/// — same stateless, wrap-by-modulo contract as
/// [`loading_hat::hat_frame_index`](super::loading_hat).
fn frame_index(def: &StickerDef, elapsed_ms: u64) -> usize {
    ((elapsed_ms / def.frame_ms) % def.frames.len() as u64) as usize
}

/// Parse `viewBox="x y w h"` off a baked sticker frame — every sticker in
/// `mlc-hat` bakes to its own fixed aspect ratio (`turntable`'s cel frames
/// are 240x180; the rest are square 512x512), so this is read from the
/// frame text itself rather than assumed.
fn parse_viewbox(svg: &str) -> Option<(f64, f64)> {
    let rest = svg.split("viewBox=\"").nth(1)?;
    let vb = rest.split('"').next()?;
    let dims: Vec<f64> = vb.split_whitespace().filter_map(|s| s.parse().ok()).collect();
    if dims.len() == 4 && dims[2] > 0.0 && dims[3] > 0.0 {
        Some((dims[2], dims[3]))
    } else {
        None
    }
}

/// Draw sticker `id` from `pack` into `rect`, fitted and centred at that
/// sticker's own baked aspect ratio, animating by `elapsed_ms` the same
/// way [`draw_loading_hat`](super::draw_loading_hat) animates its frames —
/// the caller re-derives `elapsed_ms` from its own redraw pump each frame
/// and keeps requesting a redraw for as long as any sticker stays visible;
/// stateless otherwise (SWC: no timer, no cached frame).
///
/// Returns `false` when `pack`/`id` name no known sticker — the caller
/// then draws the alt text (or whatever placeholder it prefers) instead.
pub fn draw_sticker(ctx: &mut dyn RenderContext, rect: WidgetRect, pack: &str, id: &str, elapsed_ms: u64) -> bool {
    let Some(def) = find_sticker(pack, id) else { return false };
    let frame = def.frames[frame_index(def, elapsed_ms)];
    let (vb_w, vb_h) = parse_viewbox(frame).unwrap_or((512.0, 512.0));
    let aspect = vb_w / vb_h;
    let (w, h) = if rect.width / rect.height > aspect {
        (rect.height * aspect, rect.height)
    } else {
        (rect.width, rect.width / aspect)
    };
    let x = rect.x + (rect.width - w) / 2.0;
    let y = rect.y + (rect.height - h) / 2.0;
    draw_svg_multicolor(ctx, frame, x, y, w, h);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal no-op `RenderContext` — `draw_sticker` never reaches an
    // actual draw call for an unknown id, but the trait object still has
    // to type-check; same boilerplate-minimal `MockContext`/`NoImageContext`
    // pattern already used across uzor's own widget test modules.
    struct NoopContext;

    impl uzor::render::Painter for NoopContext {
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _x: f64, _y: f64) {}
        fn rotate(&mut self, _angle: f64) {}
        fn scale(&mut self, _x: f64, _y: f64) {}
        fn set_fill_color(&mut self, _color: &str) {}
        fn set_global_alpha(&mut self, _alpha: f64) {}
        fn set_stroke_color(&mut self, _color: &str) {}
        fn set_stroke_width(&mut self, _width: f64) {}
        fn set_line_dash(&mut self, _pattern: &[f64]) {}
        fn set_line_cap(&mut self, _cap: &str) {}
        fn set_line_join(&mut self, _join: &str) {}
        fn begin_path(&mut self) {}
        fn move_to(&mut self, _x: f64, _y: f64) {}
        fn line_to(&mut self, _x: f64, _y: f64) {}
        fn close_path(&mut self) {}
        fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn arc(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64) {}
        fn ellipse(&mut self, _cx: f64, _cy: f64, _rx: f64, _ry: f64, _rot: f64, _s: f64, _e: f64) {}
        fn quadratic_curve_to(&mut self, _cpx: f64, _cpy: f64, _x: f64, _y: f64) {}
        fn bezier_curve_to(&mut self, _cp1x: f64, _cp1y: f64, _cp2x: f64, _cp2y: f64, _x: f64, _y: f64) {}
        fn stroke(&mut self) {}
        fn fill(&mut self) {}
    }
    impl uzor::render::TextRenderer for NoopContext {
        fn set_font(&mut self, _font: &str) {}
        fn set_text_align(&mut self, _align: uzor::render::TextAlign) {}
        fn set_text_baseline(&mut self, _baseline: uzor::render::TextBaseline) {}
        fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
        fn stroke_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    }
    impl uzor::render::TextMetrics for NoopContext {
        fn measure_text(&self, _text: &str) -> f64 {
            0.0
        }
        fn text_bounds(&self, _text: &str, _font: &str) -> uzor::render::TextBounds {
            uzor::render::TextBounds { x: 0.0, y: 0.0, w: 0.0, h: 0.0, ascent: 0.0, descent: 0.0 }
        }
    }
    impl uzor::render::Masking for NoopContext {
        fn clip(&mut self) {}
    }
    impl uzor::render::Effects for NoopContext {}
    impl uzor::render::ShapeHelpers for NoopContext {
        fn fill_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn stroke_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    }
    impl uzor::render::GradientPainter for NoopContext {}
    impl uzor::render::UiEffectHelpers for NoopContext {}
    impl uzor::render::BatchPainter for NoopContext {}
    impl RenderContext for NoopContext {
        fn dpr(&self) -> f64 {
            1.0
        }
    }

    #[test]
    fn every_frame_parses_to_a_non_empty_viewbox() {
        for def in STICKERS {
            for (i, frame) in def.frames.iter().enumerate() {
                let (w, h) = parse_viewbox(frame)
                    .unwrap_or_else(|| panic!("sticker {:?} frame {i} has no viewBox", def.info.id));
                assert!(w > 0.0 && h > 0.0, "sticker {:?} frame {i} viewBox has zero size", def.info.id);
            }
        }
    }

    #[test]
    fn frame_index_wraps_at_the_loop() {
        let def = find_sticker("mlc-hat", "bounce").expect("bounce must exist in the mlc-hat pack");
        assert_eq!(frame_index(def, 0), 0);
        assert_eq!(frame_index(def, 149), 0);
        assert_eq!(frame_index(def, 150), 1);
        let loop_ms = def.frame_ms * def.frames.len() as u64;
        assert_eq!(frame_index(def, loop_ms - 1), def.frames.len() - 1);
        assert_eq!(frame_index(def, loop_ms), 0);
    }

    #[test]
    fn unknown_sticker_id_returns_false() {
        let mut ctx = NoopContext;
        let rect = WidgetRect::new(0.0, 0.0, 96.0, 96.0);
        assert!(!draw_sticker(&mut ctx, rect, "mlc-hat", "no-such-sticker", 0));
        assert!(!draw_sticker(&mut ctx, rect, "no-such-pack", "bounce", 0));
    }

    #[test]
    fn sticker_pack_info_matches_the_frame_table() {
        let infos = sticker_pack("mlc-hat");
        assert_eq!(infos.len(), STICKERS.len());
        for (info, def) in infos.iter().zip(STICKERS) {
            assert_eq!(*info, def.info);
        }
        assert!(sticker_pack("no-such-pack").is_empty());
    }
}
