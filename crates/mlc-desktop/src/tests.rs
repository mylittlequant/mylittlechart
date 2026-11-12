//! The desktop app in a headless host: the window fits at 1280×800 and 1024×640 in
//! both languages, the mouse drags and zooms the chart, the timeframe and reset
//! buttons work, the watchlist searches, scrolls and opens a symbol.

use super::*;
use uzor::input::{ModifierKeys, MouseButton};
use uzor_framework::types::bus::{InputEvent, PointerInput, WheelDelta, WheelInput};
use uzor_framework::types::window::Point;
use uzor_framework::HeadlessHost;

use mlc_chart_view::i18n::{set_lang, Lang};

pub(crate) const END: i64 = 1_762_905_600;

pub(crate) type Host = HeadlessHost<DesktopSpec, DesktopApp>;

pub(crate) fn desktop_sized(w: u32, h: u32) -> (Host, WindowId) {
    let (mut host, _) = HeadlessHost::new(DesktopApp::demo(END), DesktopApp::runtime_config_sized(w, h));
    for _ in 0..4 {
        host.advance(0.017);
    }
    let win = host.snapshot().windows[0].id;
    (host, win)
}

pub(crate) fn desktop() -> (Host, WindowId) {
    set_lang(Lang::Ru);
    desktop_sized(1280, 800)
}

pub(crate) fn app(h: &Host) -> &DesktopApp {
    h.runtime().app()
}

pub(crate) fn hit(h: &Host, id: &str) -> Rect {
    app(h).last_hits.iter().find(|(k, _)| k == id).map(|(_, r)| *r).unwrap_or_else(|| panic!("{id} not laid out"))
}

fn mods() -> ModifierKeys {
    ModifierKeys::default()
}

pub(crate) fn move_to(h: &mut Host, win: WindowId, x: f64, y: f64) {
    h.input(win, InputEvent::Pointer(PointerInput::Moved { pos: Point::new(x, y), mods: mods() }));
    h.advance(0.017);
}

pub(crate) fn click(h: &mut Host, win: WindowId, x: f64, y: f64) {
    move_to(h, win, x, y);
    h.input(win, InputEvent::Pointer(PointerInput::Down { pos: Point::new(x, y), button: MouseButton::Left, mods: mods() }));
    h.advance(0.017);
    h.input(win, InputEvent::Pointer(PointerInput::Up { pos: Point::new(x, y), button: MouseButton::Left, mods: mods() }));
    settle(h);
}

/// A few frames: a click resolves, the app reacts, a requested series arrives.
pub(crate) fn settle(h: &mut Host) {
    for _ in 0..4 {
        h.advance(0.017);
    }
}

/// Paste `text` into the focused field and let the app see the edit.
pub(crate) fn paste(h: &mut Host, win: WindowId, text: &str) {
    h.input(win, InputEvent::Paste(text.into()));
    settle(h);
}

pub(crate) fn click_id(h: &mut Host, win: WindowId, id: &str) {
    let r = hit(h, id);
    click(h, win, r.x + r.width / 2.0, r.y + r.height / 2.0);
}

pub(crate) fn wheel(h: &mut Host, win: WindowId, x: f64, y: f64, lines: f64) {
    move_to(h, win, x, y);
    h.input(win, InputEvent::Wheel(WheelInput { pos: Point::new(x, y), delta: WheelDelta::Lines { x: 0.0, y: lines }, mods: mods() }));
    h.advance(0.017);
    h.advance(0.017);
}

fn inside(outer: Rect, r: Rect) -> bool {
    r.x >= outer.x - 0.5 && r.y >= outer.y - 0.5 && r.x + r.width <= outer.x + outer.width + 0.5 && r.y + r.height <= outer.y + outer.height + 0.5
}

/// The window tree laid out with a measured font (7.5 px a character, about the
/// kit's 13 px Inter) at 1280×800 and 1024×640, in both languages: nothing clipped,
/// nothing overlapping, the chart takes the rest.
#[test]
fn the_window_fits_in_both_languages_and_two_sizes() {
    use uzor::ui::dom::lay_out_in;
    use uzor::ui::media::MediaEnv;
    use uzor::ui::style::{Sheet, State};
    let all = watchlist::Watchlist::default_symbols();
    let shown: Vec<usize> = (0..all.len()).collect();
    for (w, hgt) in [(1280.0, 800.0), (1024.0, 640.0)] {
        for lang in [Lang::Ru, Lang::En] {
            set_lang(lang);
            let env = MediaEnv::desktop(w, hgt);
            let css = [uzor::ui::domkit::KIT_CSS, screen::APP_CSS].concat();
            let sheet = Sheet::parse_with(&css, &env).expect("sheet");
            let mut ctx = uzor::testing::RecordingRenderContext::with_char_width(7.5);
            let screen_r = Rect::new(0.0, 0.0, w, hgt);
            for (status, live, tf) in [(i18n::t(i18n::K::Offline), false, 3usize), (i18n::t(i18n::K::Live), true, 0)] {
                let header = screen::Header { symbol: "DOGE/USDT", venue: "Binance", status, live };
                let side = screen::Side { query: "", shown: &shown, all: &all, selected: Some(0), scroll: 0.0 };
                let root = screen::tree(&header, tf, &side).css(format!("height: {hgt}px"));
                let d = lay_out_in(&mut ctx, &sheet, &root, 0.0, 0.0, w, Some(screen_r), &|_| State::NONE);
                let chart = d.slots().find(|(k, _)| *k == screen::CHART_SLOT).map(|(_, r)| r).expect("chart slot");
                let side_r = d.rect(screen::LIST_BOX).expect("list box");
                let reset = d.rect(screen::RESET_ID).expect("reset");
                let search = d.rect(screen::SEARCH_ID).expect("search");
                let ctx_s = format!("{w}x{hgt} {lang:?}");
                assert!(inside(screen_r, reset) && inside(screen_r, search) && inside(screen_r, side_r), "{ctx_s}");
                assert!(chart.width >= w - screen::SIDE_W - 2.0 && chart.height >= hgt - 60.0, "the chart takes the rest: {chart:?} {ctx_s}");
                assert!(chart.x + chart.width <= side_r.x + 0.5 && chart.y >= reset.y + reset.height, "{ctx_s}");
                let mut right = 0.0;
                for i in 0..mlc_chart_view::demo::TIMEFRAMES.len() {
                    let b = d.rect(&format!("tf:{i}")).expect("tf");
                    assert!(inside(screen_r, b) && b.width >= 28.0 && b.x >= right - 0.5, "tf {i}: {b:?} {ctx_s}");
                    right = b.x + b.width;
                }
                assert!(right <= reset.x, "timeframes before reset {ctx_s}");
                // The status line may shorten (it clips), the symbol and buttons may not.
                for p in d.boxes.iter().filter(|p| p.text.is_some()) {
                    // Rows past the end of the list are cut by its scroll box on purpose.
                    if inside(side_r, Rect::new(p.rect.x, side_r.y, p.rect.width, 1.0)) && p.rect.y >= side_r.y {
                        continue;
                    }
                    assert!(inside(screen_r, p.rect), "{:?} clipped at {:?} {ctx_s}", p.text, p.rect);
                }
                let sym = d.boxes.iter().find(|p| p.text.as_deref() == Some("DOGE/USDT")).expect("symbol");
                let first_tf = d.rect("tf:0").unwrap();
                assert!(sym.rect.x + sym.rect.width <= first_tf.x, "symbol clear of the timeframes {ctx_s}");
            }
        }
    }
    set_lang(Lang::Ru);
}

#[test]
fn mouse_wheel_zooms_and_drag_pans_the_chart() {
    let (mut h, win) = desktop();
    let c = app(&h).chart.last_rect.unwrap();
    let (x, y) = (c.x + c.width / 2.0, c.y + c.height / 2.0);
    let n0 = app(&h).chart.model.count;
    wheel(&mut h, win, x, y, 2.0);
    let n1 = app(&h).chart.model.count;
    assert!(n1 < n0 * 0.9, "wheel up zooms in: {n0} -> {n1}");
    assert!(app(&h).chart.model.crosshair.is_some(), "hover shows the crosshair");
    let r0 = app(&h).chart.model.right;
    h.input(win, InputEvent::Pointer(PointerInput::Down { pos: Point::new(x, y), button: MouseButton::Left, mods: mods() }));
    h.advance(0.017);
    for i in 1..=10 {
        move_to(&mut h, win, x + i as f64 * 20.0, y);
    }
    h.input(win, InputEvent::Pointer(PointerInput::Up { pos: Point::new(x + 200.0, y), button: MouseButton::Left, mods: mods() }));
    h.advance(0.017);
    let r1 = app(&h).chart.model.right;
    assert!(r1 < r0 - 5.0, "dragging right brings older bars in: {r0} -> {r1}");
    assert!(!app(&h).chart.model.follow);
    // Out of the chart: no crosshair; the wheel over the toolbar does not zoom.
    let reset = hit(&h, screen::RESET_ID);
    let n2 = app(&h).chart.model.count;
    wheel(&mut h, win, reset.x - 300.0, reset.y + 4.0, 3.0);
    assert_eq!(app(&h).chart.model.count, n2);
    assert!(app(&h).chart.model.crosshair.is_none());
}

#[test]
fn timeframe_and_reset_buttons() {
    let (mut h, win) = desktop();
    click_id(&mut h, win, "tf:0");
    assert_eq!(app(&h).chart.tf, 0);
    let b = &app(&h).chart.model.bars;
    assert_eq!(b[1].timestamp - b[0].timestamp, 60);
    h.runtime_mut().app_mut().chart.model.zoom(3.0, 100.0, 300.0);
    h.advance(0.1);
    click_id(&mut h, win, screen::RESET_ID);
    assert_eq!(app(&h).chart.model.count, mlc_chart_view::model::DEFAULT_BARS);
}

#[test]
fn watchlist_search_scroll_and_open() {
    let (mut h, win) = desktop();
    assert_eq!(app(&h).chart.source().symbol(), "BTC/USDT");
    // The list scrolls under the wheel.
    let lb = hit(&h, screen::LIST_BOX);
    wheel(&mut h, win, lb.x + 40.0, lb.y + 40.0, -3.0);
    let s = app(&h).watch.scroll;
    if app(&h).watch.max_scroll() > 0.0 {
        assert!(s > 0.0, "scrolled");
    }
    // Type a search: the field is the framework's text field.
    click_id(&mut h, win, screen::SEARCH_ID);
    paste(&mut h, win, "eth");
    assert_eq!(app(&h).watch.query, "eth");
    assert_eq!(app(&h).watch.scroll, 0.0, "a new search starts at the top");
    assert_eq!(app(&h).watch.shown().0.len(), 3);
    click_id(&mut h, win, "wl:row:1");
    let src = app(&h).chart.source();
    assert_eq!(src.symbol(), "ETH/USDT");
    assert_ne!(src.venue(), "Binance", "row 1 of the ETH hits is the second venue");
    assert_eq!(app(&h).chart.shown, Shown::Demo);
    assert!(app(&h).chart.model.bars.last().unwrap().close < 10_000.0);
    // Nothing matches: the list says so.
    click_id(&mut h, win, screen::SEARCH_ID);
    paste(&mut h, win, "zz");
    assert!(app(&h).watch.shown().0.is_empty());
    assert!(app(&h).last_hits.iter().all(|(k, _)| !k.starts_with("wl:row:")));
}

