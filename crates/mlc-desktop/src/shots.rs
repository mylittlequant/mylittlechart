//! Desktop screenshots (1280×800, 1× pixels) written when `MLC_SHOTS_DIR` is set:
//! `MLC_SHOTS_DIR=dir cargo test -p mlc-desktop shots`.

use uzor::render::{Painter, ShapeHelpers};
use uzor_render_tiny_skia::TinySkiaCpuRenderContext;

use crate::tests::{click_id, desktop_sized, move_to, paste, wheel, Host};
use mlc_chart_view::i18n::{set_lang, Lang};

fn save(h: &mut Host, win: uzor_framework::WindowId, w: u32, hgt: u32, dir: &str, name: &str) {
    h.advance(0.017);
    let mut ctx = TinySkiaCpuRenderContext::new(w, hgt, 1.0);
    ctx.set_fill_color("#141414");
    ctx.fill_rect(0.0, 0.0, w as f64, hgt as f64);
    h.paint_window(win, 1.0, &mut ctx).expect("window");
    ctx.pixmap().save_png(format!("{dir}/{name}.png")).expect("png");
}

#[test]
fn shots() {
    let Ok(dir) = std::env::var("MLC_SHOTS_DIR") else { return };
    std::fs::create_dir_all(&dir).expect("dir");
    for lang in [Lang::Ru, Lang::En] {
        let sfx = if lang == Lang::Ru { "ru" } else { "en" };
        set_lang(lang);
        let (mut h, win) = desktop_sized(1280, 800);
        h.advance(0.5);
        save(&mut h, win, 1280, 800, &dir, &format!("01-desktop-1h-{sfx}"));
        if lang == Lang::En {
            let (mut s, sw) = desktop_sized(1024, 640);
            save(&mut s, sw, 1024, 640, &dir, "06-desktop-1024x640-en");
            continue;
        }
        let c = h.runtime().app().chart.last_rect.unwrap();
        wheel(&mut h, win, c.x + c.width * 0.6, c.y + c.height * 0.4, 3.0);
        save(&mut h, win, 1280, 800, &dir, "02-wheel-zoom-crosshair");
        // A screenshot lays the tree out with real glyph widths, the headless frames
        // with none: take the hit rects of a headless frame before clicking.
        move_to(&mut h, win, 600.0, 20.0);
        click_id(&mut h, win, "tf:4");
        move_to(&mut h, win, c.x + c.width * 0.3, c.y + c.height * 0.5);
        save(&mut h, win, 1280, 800, &dir, "03-tf-4h");
        click_id(&mut h, win, crate::screen::SEARCH_ID);
        paste(&mut h, win, "sol");
        save(&mut h, win, 1280, 800, &dir, "04-search-sol");
        click_id(&mut h, win, "wl:row:1");
        save(&mut h, win, 1280, 800, &dir, "05-opened-sol-bybit");
    }
    set_lang(Lang::Ru);
}
