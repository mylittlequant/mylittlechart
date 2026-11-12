//! The app in a headless host on a 390×844 phone with a notch and a home
//! indicator: fingers pan, pinch, long-press and double-tap the chart; the
//! timeframe buttons switch the bars.

use super::*;
use crate::source::Feed;
use uzor_framework::types::bus::{InputEvent, TouchInput, WindowInput};
use uzor_framework::types::window::{Insets, Point};
use uzor_framework::HeadlessHost;

pub(crate) const END: i64 = 1_762_905_600;

pub(crate) type Host = HeadlessHost<ChartSpec, ChartViewApp>;

pub(crate) fn phone() -> (Host, WindowId) {
    crate::i18n::set_lang(crate::i18n::Lang::Ru);
    let app = ChartViewApp::demo(END);
    let (mut h, _) = HeadlessHost::new(app, ChartViewApp::runtime_config());
    h.advance(0.017);
    let win = h.snapshot().windows[0].id;
    h.input(win, InputEvent::Window(WindowInput::Insets { safe: Insets::new(47.0, 0.0, 34.0, 0.0), keyboard: 0.0 }));
    for _ in 0..4 {
        h.advance(0.017);
    }
    (h, win)
}

pub(crate) fn touch(h: &mut Host, win: WindowId, t: TouchInput) {
    h.input(win, InputEvent::Touch(t));
    h.advance(0.017);
}

fn app(h: &Host) -> &ChartViewApp {
    h.runtime().app()
}

fn chart(h: &Host) -> Rect {
    app(h).last_chart().expect("chart laid out")
}

fn hit(h: &Host, id: &str) -> Rect {
    app(h).last_hits.iter().find(|(k, _)| k == id).map(|(_, r)| *r).unwrap_or_else(|| panic!("{id} not laid out"))
}

pub(crate) fn tap(h: &mut Host, win: WindowId, x: f64, y: f64, id: u64) {
    touch(h, win, TouchInput::Start { id, pos: Point::new(x, y) });
    touch(h, win, TouchInput::End { id, pos: Point::new(x, y) });
    h.advance(0.017);
}

#[test]
fn demo_bars_fill_the_chart_under_the_notch() {
    let (h, _) = phone();
    let a = app(&h);
    assert_eq!(a.chart().shown, Shown::Demo);
    assert_eq!(a.chart().model.bars.len(), 500);
    let c = chart(&h);
    assert!(c.y >= 47.0 + 44.0, "chart under the top bar: {c:?}");
    assert!(c.y + c.height <= 844.0 - 34.0, "chart above the home indicator: {c:?}");
    assert!(c.height > 450.0);
}

#[test]
fn one_finger_pans_the_bars() {
    let (mut h, win) = phone();
    let c = chart(&h);
    let r0 = app(&h).chart().model.right;
    let y = c.y + c.height / 2.0;
    touch(&mut h, win, TouchInput::Start { id: 1, pos: Point::new(150.0, y) });
    for i in 1..=8 {
        touch(&mut h, win, TouchInput::Move { id: 1, pos: Point::new(150.0 + i as f64 * 15.0, y) });
    }
    touch(&mut h, win, TouchInput::End { id: 1, pos: Point::new(270.0, y) });
    let r1 = app(&h).chart().model.right;
    assert!(r1 < r0 - 5.0, "dragging right brings older bars in: {r0} -> {r1}");
    assert!(!app(&h).chart().model.follow);
}

#[test]
fn two_fingers_pinch_zoom() {
    let (mut h, win) = phone();
    let c = chart(&h);
    let y = c.y + c.height / 2.0;
    let n0 = app(&h).chart().model.count;
    touch(&mut h, win, TouchInput::Start { id: 1, pos: Point::new(150.0, y) });
    touch(&mut h, win, TouchInput::Start { id: 2, pos: Point::new(200.0, y) });
    for i in 1..=6 {
        let d = i as f64 * 12.0;
        touch(&mut h, win, TouchInput::Move { id: 1, pos: Point::new(150.0 - d, y) });
        touch(&mut h, win, TouchInput::Move { id: 2, pos: Point::new(200.0 + d, y) });
    }
    touch(&mut h, win, TouchInput::End { id: 2, pos: Point::new(272.0, y) });
    touch(&mut h, win, TouchInput::End { id: 1, pos: Point::new(78.0, y) });
    let n1 = app(&h).chart().model.count;
    assert!(n1 < n0 * 0.5, "spreading the fingers zooms in: {n0} -> {n1}");
}

#[test]
fn long_press_shows_the_crosshair_and_double_tap_resets() {
    let (mut h, win) = phone();
    let c = chart(&h);
    let (x, y) = (120.0, c.y + 200.0);
    touch(&mut h, win, TouchInput::Start { id: 1, pos: Point::new(x, y) });
    for _ in 0..40 {
        h.advance(0.017);
    }
    let ch = app(&h).chart().model.crosshair.expect("crosshair after a long press");
    assert!((ch.0 - (x - c.x)).abs() < 1.0 && (ch.1 - (y - c.y)).abs() < 1.0, "at the finger: {ch:?}");
    touch(&mut h, win, TouchInput::End { id: 1, pos: Point::new(x, y) });
    // Zoom away, then a double tap restores the default view.
    h.runtime_mut().app_mut().chart_mut().model.zoom(3.0, 100.0, 300.0);
    tap(&mut h, win, 200.0, c.y + 300.0, 2);
    tap(&mut h, win, 202.0, c.y + 302.0, 3);
    let m = &app(&h).chart().model;
    assert_eq!(m.count, crate::model::DEFAULT_BARS, "double tap resets the zoom");
    assert!(m.crosshair.is_none());
}

#[test]
fn timeframe_buttons_switch_the_bars_and_reset_button_resets() {
    let (mut h, win) = phone();
    let b = hit(&h, "tf:0");
    tap(&mut h, win, b.x + b.width / 2.0, b.y + b.height / 2.0, 1);
    for _ in 0..3 {
        h.advance(0.017);
    }
    let a = app(&h);
    assert_eq!(a.chart().tf, 0);
    assert_eq!(a.chart().model.bars[1].timestamp - a.chart().model.bars[0].timestamp, 60);
    h.runtime_mut().app_mut().chart_mut().model.zoom(2.0, 100.0, 300.0);
    h.advance(0.5);
    let r = hit(&h, screen::RESET_ID);
    tap(&mut h, win, r.x + r.width / 2.0, r.y + r.height / 2.0, 2);
    assert_eq!(app(&h).chart().model.count, crate::model::DEFAULT_BARS);
}

#[test]
fn live_source_falls_back_to_labelled_demo_data() {
    struct Silent;
    impl Source for Silent {
        fn venue(&self) -> &str {
            "Binance"
        }
        fn symbol(&self) -> &str {
            "BTC/USDT"
        }
        fn request(&mut self, _: &str) {}
        fn poll(&mut self) -> Vec<Feed> {
            Vec::new()
        }
    }
    let mut a = ChartViewApp::with_source(Box::new(Silent), true, END);
    let c = a.chart_mut();
    c.pump(0.0);
    assert_eq!(c.shown, Shown::Loading);
    c.pump(20.0);
    assert_eq!(c.shown, Shown::Offline);
    assert_eq!(c.model.bars.len(), 500);
    assert_eq!(c.plot_label(), Some("Нет сети: демо-данные"));
}
