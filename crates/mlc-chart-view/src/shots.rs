//! Phone screenshots (390×844, 2× pixels) of the chart states, written when
//! `MLC_SHOTS_DIR` is set: `MLC_SHOTS_DIR=dir cargo test -p mlc-chart-view shots`.

use uzor::render::{Painter, ShapeHelpers};
use uzor_framework::types::bus::TouchInput;
use uzor_framework::types::window::Point;
use uzor_render_tiny_skia::TinySkiaCpuRenderContext;

use crate::tests::{phone, tap, touch, Host};
use crate::i18n::{set_lang, Lang};

fn save(h: &mut Host, win: uzor_framework::WindowId, dir: &str, name: &str) {
    h.advance(0.017);
    let mut ctx = TinySkiaCpuRenderContext::new(780, 1688, 2.0);
    ctx.scale(2.0, 2.0);
    ctx.set_fill_color("#121212");
    ctx.fill_rect(0.0, 0.0, 390.0, 844.0);
    h.paint_window(win, 2.0, &mut ctx).expect("window");
    // Where the notch and the home indicator are (the host's, not the app's).
    ctx.set_fill_color("#000000");
    ctx.fill_rect(130.0, 10.0, 130.0, 30.0);
    ctx.set_fill_color("#5a5a5a");
    ctx.fill_rect(128.0, 844.0 - 13.0, 134.0, 5.0);
    ctx.pixmap().save_png(format!("{dir}/{name}.png")).expect("png");
}

#[test]
fn shots() {
    let Ok(dir) = std::env::var("MLC_SHOTS_DIR") else { return };
    std::fs::create_dir_all(&dir).expect("dir");
    for lang in [Lang::Ru, Lang::En] {
        let sfx = if lang == Lang::Ru { "ru" } else { "en" };
        let (mut h, win) = phone();
        set_lang(lang);
        h.advance(0.5);
        save(&mut h, win, &dir, &format!("01-chart-1h-{sfx}"));
        if lang == Lang::En {
            continue;
        }
        let c = h.runtime().app().last_chart.unwrap();
        // Pinch in.
        let y = c.y + c.height / 2.0;
        touch(&mut h, win, TouchInput::Start { id: 1, pos: Point::new(170.0, y) });
        touch(&mut h, win, TouchInput::Start { id: 2, pos: Point::new(210.0, y) });
        for i in 1..=6 {
            let d = i as f64 * 10.0;
            touch(&mut h, win, TouchInput::Move { id: 1, pos: Point::new(170.0 - d, y) });
            touch(&mut h, win, TouchInput::Move { id: 2, pos: Point::new(210.0 + d, y) });
        }
        touch(&mut h, win, TouchInput::End { id: 2, pos: Point::new(270.0, y) });
        touch(&mut h, win, TouchInput::End { id: 1, pos: Point::new(110.0, y) });
        save(&mut h, win, &dir, "02-pinch-zoomed-in");
        // Pan to older bars.
        touch(&mut h, win, TouchInput::Start { id: 3, pos: Point::new(100.0, y) });
        for i in 1..=10 {
            touch(&mut h, win, TouchInput::Move { id: 3, pos: Point::new(100.0 + i as f64 * 20.0, y) });
        }
        touch(&mut h, win, TouchInput::End { id: 3, pos: Point::new(300.0, y) });
        save(&mut h, win, &dir, "03-panned");
        // Long press: crosshair.
        touch(&mut h, win, TouchInput::Start { id: 4, pos: Point::new(160.0, c.y + 180.0) });
        for _ in 0..40 {
            h.advance(0.017);
        }
        save(&mut h, win, &dir, "04-long-press-crosshair");
        touch(&mut h, win, TouchInput::End { id: 4, pos: Point::new(160.0, c.y + 180.0) });
        // Double tap: back to the default view.
        tap(&mut h, win, 200.0, c.y + 300.0, 5);
        tap(&mut h, win, 201.0, c.y + 301.0, 6);
        save(&mut h, win, &dir, "05-double-tap-reset");
        // 1-minute bars.
        let b = h.runtime().app().last_hits.iter().find(|(k, _)| k == "tf:0").map(|(_, r)| *r).unwrap();
        tap(&mut h, win, b.x + b.width / 2.0, b.y + b.height / 2.0, 7);
        save(&mut h, win, &dir, "06-tf-1m");
    }
    set_lang(Lang::Ru);
}
