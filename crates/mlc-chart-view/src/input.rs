//! Fingers and the mouse → chart operations.
//!
//! Touch: one finger pans, two fingers pinch-zoom around their midpoint, a long
//! press shows the crosshair (and the held finger moves it), a double tap resets the
//! view, a tap hides the crosshair. Mouse: drag pans, the wheel zooms at the
//! pointer, hover moves the crosshair, a double click resets.
//!
//! The chart is a DOM paint slot (`El::paint_slot`) with a uzor `SlotPainter`
//! ([`crate::ChartSlot`]): it captures its fingers, so the gestures and the mouse
//! arrive as its `SlotEvent`s ([`ChartInput::event`]) and two fingers never scroll
//! anything else; the long-pressed finger drags the crosshair and double taps come
//! from uzor's recogniser. Without a DOM, [`ChartInput::update`] reads the same from
//! the view (the mouse with [`Mouse::read`]).

use uzor::ui::dom::SlotEvent;
use uzor::{Rect, WidgetId};
use uzor_framework::types::gesture::Gesture;
use uzor_framework::{Spec, VisualView};

/// One change to the chart, in plot-local pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChartOp {
    /// Move the bars by `dx` px (positive: to the right).
    Pan(f64),
    /// Zoom by `factor` (> 1 in) around plot x `anchor`.
    Zoom { factor: f64, anchor: f64 },
    /// Show the crosshair at plot `(x, y)`, or hide it.
    Crosshair(Option<(f64, f64)>),
    /// Back to the default view.
    Reset,
}

/// Double tap / double click window, seconds, and how far apart the two may land.
pub const DOUBLE_S: f64 = 0.3;
pub const DOUBLE_SLOP: f64 = 24.0;

/// Gesture → op state for one chart.
#[derive(Clone, Debug, Default)]
pub struct Touch {
    panning: bool,
    pinch_scale: Option<f64>,
    /// The crosshair follows the held finger.
    held: bool,
    crosshair: bool,
}

fn inside(r: Rect, x: f64, y: f64) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

impl Touch {
    /// Translate this frame's touch gestures over `plot` (window coordinates).
    pub fn ops(&mut self, gestures: &[Gesture], plot: Rect) -> Vec<ChartOp> {
        let local = |x: f64, y: f64| (x - plot.x, y - plot.y);
        let mut out = Vec::new();
        for g in gestures {
            match *g {
                Gesture::PanStart { origin, pos, .. } if inside(plot, origin.x, origin.y) && self.pinch_scale.is_none() => {
                    if self.held {
                        out.push(ChartOp::Crosshair(Some(local(pos.x, pos.y))));
                    } else {
                        self.panning = true;
                        out.push(ChartOp::Pan(pos.x - origin.x));
                    }
                }
                Gesture::Pan { pos, delta, .. } => {
                    if self.held {
                        out.push(ChartOp::Crosshair(Some(local(pos.x, pos.y))));
                    } else if self.panning {
                        out.push(ChartOp::Pan(delta.0));
                    }
                }
                Gesture::PanEnd { .. } => {
                    self.panning = false;
                    self.held = false;
                }
                Gesture::Pinch { center, scale } => {
                    if self.pinch_scale.is_none() && !inside(plot, center.x, center.y) {
                        continue;
                    }
                    self.panning = false;
                    self.held = false;
                    let prev = self.pinch_scale.replace(scale).unwrap_or(1.0);
                    if prev > 0.0 && scale > 0.0 && (scale / prev - 1.0).abs() > 1e-6 {
                        out.push(ChartOp::Zoom { factor: scale / prev, anchor: center.x - plot.x });
                    }
                }
                Gesture::PinchEnd { .. } => self.pinch_scale = None,
                Gesture::LongPress { pos } if inside(plot, pos.x, pos.y) => {
                    self.held = true;
                    self.panning = false;
                    self.crosshair = true;
                    out.push(ChartOp::Crosshair(Some(local(pos.x, pos.y))));
                }
                Gesture::LongPressDrag { pos, .. } if self.held => out.push(ChartOp::Crosshair(Some(local(pos.x, pos.y)))),
                Gesture::LongPressEnd { .. } => self.held = false,
                Gesture::PinchPan { delta, .. } if self.pinch_scale.is_some() => out.push(ChartOp::Pan(delta.0)),
                Gesture::DoubleTap { pos } if inside(plot, pos.x, pos.y) => {
                    self.held = false;
                    self.crosshair = false;
                    out.push(ChartOp::Reset);
                }
                // Taps arrive as clicks; swipes and the rest are not the chart's.
                _ => {}
            }
        }
        out
    }

    /// One touch event of the chart's slot ([`SlotEvent`], slot-local; the slot's
    /// content box is `rect`), over `plot` (window coordinates).
    pub fn event(&mut self, ev: &SlotEvent, rect: Rect, plot: Rect) -> Vec<ChartOp> {
        let win = |x: f64, y: f64| (rect.x + x, rect.y + y);
        let local = |(x, y): (f64, f64)| (x - plot.x, y - plot.y);
        let mut out = Vec::new();
        match *ev {
            // A pan's first step starts it (the slot gets no separate start).
            SlotEvent::Pan { x, y, dx, .. } if self.pinch_scale.is_none() => {
                let p = win(x, y);
                if self.held {
                    out.push(ChartOp::Crosshair(Some(local(p))));
                } else if self.panning || inside(plot, p.0 - dx, p.1) {
                    self.panning = true;
                    out.push(ChartOp::Pan(dx));
                }
            }
            SlotEvent::PanEnd { .. } => {
                self.panning = false;
                self.held = false;
            }
            SlotEvent::Pinch { x, y, scale } => {
                let c = win(x, y);
                if self.pinch_scale.is_some() || inside(plot, c.0, c.1) {
                    self.panning = false;
                    self.held = false;
                    let prev = self.pinch_scale.replace(scale).unwrap_or(1.0);
                    if prev > 0.0 && scale > 0.0 && (scale / prev - 1.0).abs() > 1e-6 {
                        out.push(ChartOp::Zoom { factor: scale / prev, anchor: c.0 - plot.x });
                    }
                }
            }
            SlotEvent::PinchEnd { .. } => self.pinch_scale = None,
            SlotEvent::PinchPan { dx, .. } if self.pinch_scale.is_some() => out.push(ChartOp::Pan(dx)),
            SlotEvent::LongPress { x, y } => {
                let p = win(x, y);
                if inside(plot, p.0, p.1) {
                    self.held = true;
                    self.panning = false;
                    self.crosshair = true;
                    out.push(ChartOp::Crosshair(Some(local(p))));
                }
            }
            SlotEvent::DoubleTap { x, y } => {
                let p = win(x, y);
                if inside(plot, p.0, p.1) {
                    self.held = false;
                    self.crosshair = false;
                    out.push(ChartOp::Reset);
                }
            }
            _ => {}
        }
        out
    }

    /// The crosshair is shown by a long press (a tap hides it).
    pub fn crosshair_shown(&self) -> bool {
        self.crosshair
    }
}

/// Everything for one chart widget: touch, mouse, clicks.
#[derive(Clone, Debug, Default)]
pub struct ChartInput {
    pub touch: Touch,
    last_click: Option<(f64, (f64, f64))>,
    mouse_last: Option<(f64, f64)>,
}

impl ChartInput {
    /// Feed a click at window `pos` at time `now`: a second one within
    /// [`DOUBLE_S`] / [`DOUBLE_SLOP`] is a reset, a single one hides a shown crosshair.
    pub fn click(&mut self, pos: (f64, f64), now: f64, coarse: bool) -> Option<ChartOp> {
        let double = self.last_click.is_some_and(|(t, p)| now - t <= DOUBLE_S && (pos.0 - p.0).hypot(pos.1 - p.1) <= DOUBLE_SLOP);
        if double {
            self.last_click = None;
            self.touch.crosshair = false;
            return Some(ChartOp::Reset);
        }
        self.last_click = Some((now, pos));
        if coarse && self.touch.crosshair {
            self.touch.crosshair = false;
            return Some(ChartOp::Crosshair(None));
        }
        None
    }

    /// All ops of this frame: the slot's captured `gestures`, the `pointer` over it,
    /// whether it was `clicked`, and the [`Mouse`] read from the view, over `plot`.
    pub fn ops(&mut self, gestures: &[Gesture], pointer: Option<(f64, f64)>, clicked: bool, mouse: &Mouse, plot: Rect, now: f64) -> Vec<ChartOp> {
        let mut out = self.touch.ops(gestures, plot);
        if clicked {
            let at = pointer.or(mouse.raw).unwrap_or((plot.x, plot.y));
            out.extend(self.click(at, now, mouse.coarse));
        }
        if !mouse.coarse {
            // Mouse drag pans.
            match mouse.pressed_at {
                Some(p) => {
                    if let Some(l) = self.mouse_last {
                        if p.0 != l.0 {
                            out.push(ChartOp::Pan(p.0 - l.0));
                        }
                    }
                    self.mouse_last = Some(p);
                }
                None => self.mouse_last = None,
            }
            // Hover crosshair.
            out.push(ChartOp::Crosshair(pointer.filter(|p| inside(plot, p.0, p.1)).map(|p| (p.0 - plot.x, p.1 - plot.y))));
            // Wheel zoom at the pointer (not a finger's scroll).
            if mouse.wheel != 0.0 {
                let anchor = pointer.map(|p| p.0 - plot.x).unwrap_or(plot.width / 2.0);
                out.push(ChartOp::Zoom { factor: 1.12f64.powf(mouse.wheel), anchor });
            }
        }
        out
    }

    /// The ops of one event of the chart's slot ([`SlotEvent`], slot-local; content
    /// box `rect`) over `plot` (window coordinates) at time `now`; `coarse`: the
    /// pointer is a finger or a pen (no hover crosshair, no wheel).
    pub fn event(&mut self, ev: &SlotEvent, rect: Rect, plot: Rect, now: f64, coarse: bool) -> Vec<ChartOp> {
        let win = |x: f64, y: f64| (rect.x + x, rect.y + y);
        match *ev {
            SlotEvent::Enter { x, y } | SlotEvent::Move { x, y } if !coarse => {
                let p = win(x, y);
                vec![ChartOp::Crosshair(inside(plot, p.0, p.1).then(|| (p.0 - plot.x, p.1 - plot.y)))]
            }
            SlotEvent::Leave if !coarse => vec![ChartOp::Crosshair(None)],
            // Mouse drag pans (fingers pan through `Pan`).
            SlotEvent::Drag { dx, .. } if dx != 0.0 => vec![ChartOp::Pan(dx)],
            SlotEvent::Click { x, y } => self.click(win(x, y), now, coarse).into_iter().collect(),
            // Wheel zoom at the pointer (a finger's scroll is not a wheel).
            SlotEvent::Wheel { x, dy, .. } if !coarse && dy != 0.0 => vec![ChartOp::Zoom { factor: 1.12f64.powf(dy), anchor: rect.x + x - plot.x }],
            _ => self.touch.event(ev, rect, plot),
        }
    }

    /// All ops of this frame for widget `id` drawn over `plot`, everything read from
    /// the view (a chart outside a DOM tree).
    pub fn update<S: Spec>(&mut self, view: &VisualView<S>, id: &WidgetId, plot: Rect, now: f64) -> Vec<ChartOp> {
        let mouse = Mouse::read(view, id);
        self.ops(&view.captured_gestures(id), view.pointer_over(id), view.clicked(id), &mouse, plot, now)
    }
}

/// The mouse over the chart this frame, read from the view before the tree is built.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mouse {
    /// A finger or a pen: no hover crosshair, no mouse drag, no wheel.
    pub coarse: bool,
    /// The pointer while a mouse button holds the chart.
    pub pressed_at: Option<(f64, f64)>,
    /// Wheel steps over the chart (a finger's scroll is not a wheel).
    pub wheel: f64,
    /// The raw pointer (z-order blind), for where a click landed.
    pub raw: Option<(f64, f64)>,
}

impl Mouse {
    /// Read the mouse over widget `id` from `view`.
    pub fn read<S: Spec>(view: &VisualView<S>, id: &WidgetId) -> Self {
        let coarse = view.coarse_pointer();
        let raw = view.raw_pointer();
        let (_, dy) = view.scroll_over(id);
        Mouse {
            coarse,
            pressed_at: if view.is_pressed(id) { raw } else { None },
            wheel: if view.scroll_touch() { 0.0 } else { dy },
            raw,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uzor_framework::types::window::Point;

    const PLOT: Rect = Rect { x: 0.0, y: 100.0, width: 328.0, height: 500.0 };

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn one_finger_pans_from_inside_the_plot_only() {
        let mut t = Touch::default();
        let g = [
            Gesture::PanStart { origin: p(100.0, 300.0), pos: p(110.0, 300.0), axis: uzor_framework::types::gesture::Axis::Horizontal, edge: None },
            Gesture::Pan { pos: p(130.0, 302.0), delta: (20.0, 2.0), total: (30.0, 2.0), edge: None },
            Gesture::PanEnd { pos: p(130.0, 302.0), velocity: (0.0, 0.0), edge: None },
            Gesture::Pan { pos: p(140.0, 302.0), delta: (10.0, 0.0), total: (40.0, 0.0), edge: None },
        ];
        assert_eq!(t.ops(&g, PLOT), [ChartOp::Pan(10.0), ChartOp::Pan(20.0)]);
        let outside = [Gesture::PanStart { origin: p(100.0, 50.0), pos: p(120.0, 50.0), axis: uzor_framework::types::gesture::Axis::Horizontal, edge: None }, Gesture::Pan { pos: p(130.0, 50.0), delta: (10.0, 0.0), total: (30.0, 0.0), edge: None }];
        assert!(t.ops(&outside, PLOT).is_empty(), "a pan from the top bar is not the chart's");
    }

    #[test]
    fn pinch_zooms_by_the_scale_step_around_the_midpoint() {
        let mut t = Touch::default();
        let g = [Gesture::Pinch { center: p(200.0, 300.0), scale: 1.5 }, Gesture::Pinch { center: p(200.0, 300.0), scale: 3.0 }, Gesture::PinchEnd { scale: 3.0 }];
        let ops = t.ops(&g, PLOT);
        assert_eq!(ops, [ChartOp::Zoom { factor: 1.5, anchor: 200.0 }, ChartOp::Zoom { factor: 2.0, anchor: 200.0 }]);
        // A new pinch starts from scale 1 again.
        assert_eq!(t.ops(&[Gesture::Pinch { center: p(100.0, 300.0), scale: 0.5 }], PLOT), [ChartOp::Zoom { factor: 0.5, anchor: 100.0 }]);
    }

    #[test]
    fn long_press_shows_the_crosshair_and_the_held_finger_moves_it() {
        let mut t = Touch::default();
        let ops = t.ops(&[Gesture::LongPress { pos: p(50.0, 200.0) }], PLOT);
        assert_eq!(ops, [ChartOp::Crosshair(Some((50.0, 100.0)))]);
        assert!(t.crosshair_shown());
        let ops = t.ops(&[Gesture::Pan { pos: p(60.0, 210.0), delta: (10.0, 10.0), total: (10.0, 10.0), edge: None }], PLOT);
        assert_eq!(ops, [ChartOp::Crosshair(Some((60.0, 110.0)))], "held: the finger moves the crosshair, not the bars");
        t.ops(&[Gesture::PanEnd { pos: p(60.0, 210.0), velocity: (0.0, 0.0), edge: None }], PLOT);
        let ops = t.ops(&[Gesture::Pan { pos: p(70.0, 210.0), delta: (10.0, 0.0), total: (10.0, 0.0), edge: None }], PLOT);
        assert!(ops.is_empty(), "after the lift a pan needs its own start");
    }

    #[test]
    fn double_click_resets_single_tap_hides_the_crosshair() {
        let mut i = ChartInput::default();
        assert_eq!(i.click((10.0, 10.0), 1.0, true), None);
        assert_eq!(i.click((14.0, 12.0), 1.2, true), Some(ChartOp::Reset));
        assert_eq!(i.click((14.0, 12.0), 1.3, true), None, "a third tap starts a new pair");
        assert_eq!(i.click((14.0, 12.0), 2.0, true), None, "too slow");
        assert_eq!(i.click((90.0, 12.0), 2.1, true), None, "too far");
        i.touch.ops(&[Gesture::LongPress { pos: p(50.0, 200.0) }], PLOT);
        assert_eq!(i.click((200.0, 300.0), 5.0, true), Some(ChartOp::Crosshair(None)));
    }

    #[test]
    fn captured_gestures_double_tap_and_long_press_drag() {
        let mut t = Touch::default();
        assert_eq!(t.ops(&[Gesture::DoubleTap { pos: p(30.0, 300.0) }], PLOT), [ChartOp::Reset]);
        assert!(t.ops(&[Gesture::DoubleTap { pos: p(30.0, 50.0) }], PLOT).is_empty(), "not over the plot");
        t.ops(&[Gesture::LongPress { pos: p(50.0, 200.0) }], PLOT);
        assert_eq!(t.ops(&[Gesture::LongPressDrag { pos: p(80.0, 220.0), delta: (30.0, 20.0) }], PLOT), [ChartOp::Crosshair(Some((80.0, 120.0)))]);
        t.ops(&[Gesture::LongPressEnd { pos: p(80.0, 220.0) }], PLOT);
        assert!(t.ops(&[Gesture::LongPressDrag { pos: p(90.0, 220.0), delta: (10.0, 0.0) }], PLOT).is_empty(), "released: the crosshair stays put");
        assert!(t.crosshair_shown(), "and stays shown until a tap");
    }

    #[test]
    fn two_finger_pan_moves_the_bars_while_pinching() {
        let mut t = Touch::default();
        t.ops(&[Gesture::Pinch { center: p(200.0, 300.0), scale: 1.0 }], PLOT);
        assert_eq!(t.ops(&[Gesture::PinchPan { center: p(212.0, 300.0), delta: (12.0, 0.0), total: (12.0, 0.0) }], PLOT), [ChartOp::Pan(12.0)]);
        t.ops(&[Gesture::PinchEnd { scale: 1.0 }], PLOT);
        assert!(t.ops(&[Gesture::PinchPan { center: p(220.0, 300.0), delta: (8.0, 0.0), total: (20.0, 0.0) }], PLOT).is_empty());
    }

    #[test]
    fn mouse_drag_wheel_and_hover() {
        let mut i = ChartInput::default();
        let m = |pressed_at, wheel| Mouse { coarse: false, pressed_at, wheel, raw: pressed_at };
        let ops = i.ops(&[], Some((100.0, 200.0)), false, &m(Some((100.0, 200.0)), 0.0), PLOT, 0.0);
        assert_eq!(ops, [ChartOp::Crosshair(Some((100.0, 100.0)))]);
        let ops = i.ops(&[], Some((130.0, 200.0)), false, &m(Some((130.0, 200.0)), 1.0), PLOT, 0.1);
        assert_eq!(ops[0], ChartOp::Pan(30.0));
        assert!(matches!(ops[2], ChartOp::Zoom { anchor, .. } if anchor == 130.0));
        let ops = i.ops(&[], None, false, &m(None, 0.0), PLOT, 0.2);
        assert_eq!(ops, [ChartOp::Crosshair(None)], "left: the crosshair hides");
        let coarse = Mouse { coarse: true, ..m(Some((10.0, 200.0)), 2.0) };
        assert!(i.ops(&[], Some((10.0, 200.0)), false, &coarse, PLOT, 0.3).is_empty(), "a finger is not a mouse");
    }
}
