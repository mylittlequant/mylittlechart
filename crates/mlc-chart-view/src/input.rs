//! Fingers and the mouse → chart operations.
//!
//! Touch: one finger pans, two fingers pinch-zoom around their midpoint, a long
//! press shows the crosshair (and the held finger moves it), a double tap resets the
//! view, a tap hides the crosshair. Mouse: drag pans, the wheel zooms at the
//! pointer, hover moves the crosshair, a double click resets.
//!
//! With uzor 1.5.35 (crates.io) the chart reads the window's gestures
//! (`VisualView::gestures`) inside its rect. With the `uzor-next` feature it reads
//! the gestures its touch-capturing widget owns (`VisualView::captured_gestures`,
//! uzor's touch-capture branch): two fingers then never scroll anything else, the
//! long-pressed finger drags the crosshair and double taps come from the recogniser.
//! Everything below the [`Touch`] translation is the same in both.

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
                #[cfg(feature = "uzor-next")]
                Gesture::LongPressDrag { pos, .. } if self.held => out.push(ChartOp::Crosshair(Some(local(pos.x, pos.y)))),
                #[cfg(feature = "uzor-next")]
                Gesture::LongPressEnd { .. } => self.held = false,
                #[cfg(feature = "uzor-next")]
                Gesture::PinchPan { delta, .. } if self.pinch_scale.is_some() => out.push(ChartOp::Pan(delta.0)),
                #[cfg(feature = "uzor-next")]
                Gesture::DoubleTap { pos } if inside(plot, pos.x, pos.y) => out.push(ChartOp::Reset),
                _ => {}
            }
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

    /// All ops of this frame for the widget `id` drawn over `plot`.
    pub fn update<S: Spec>(&mut self, view: &VisualView<S>, id: &WidgetId, plot: Rect, now: f64) -> Vec<ChartOp> {
        #[cfg(feature = "uzor-next")]
        let gestures = view.captured_gestures(id);
        #[cfg(not(feature = "uzor-next"))]
        let gestures = view.gestures().to_vec();
        let mut out = self.touch.ops(&gestures, plot);
        let coarse = view.coarse_pointer();
        let over = view.pointer_over(id);
        if view.clicked(id) {
            let at = over.or(view.raw_pointer()).unwrap_or((plot.x, plot.y));
            out.extend(self.click(at, now, coarse));
        }
        if !coarse {
            // Mouse drag pans.
            if view.is_pressed(id) {
                if let Some(p) = view.raw_pointer() {
                    if let Some(l) = self.mouse_last {
                        if p.0 != l.0 {
                            out.push(ChartOp::Pan(p.0 - l.0));
                        }
                    }
                    self.mouse_last = Some(p);
                }
            } else {
                self.mouse_last = None;
            }
            // Hover crosshair.
            out.push(ChartOp::Crosshair(over.filter(|p| inside(plot, p.0, p.1)).map(|p| (p.0 - plot.x, p.1 - plot.y))));
            // Wheel zoom at the pointer (not a finger's scroll).
            let (_, dy) = view.scroll_over(id);
            if dy != 0.0 && !view.scroll_touch() {
                let anchor = over.map(|p| p.0 - plot.x).unwrap_or(plot.width / 2.0);
                out.push(ChartOp::Zoom { factor: 1.12f64.powf(dy), anchor });
            }
        }
        out
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

    #[cfg(feature = "uzor-next")]
    #[test]
    fn captured_gestures_double_tap_and_long_press_drag() {
        let mut t = Touch::default();
        assert_eq!(t.ops(&[Gesture::DoubleTap { pos: p(30.0, 300.0) }], PLOT), [ChartOp::Reset]);
        t.ops(&[Gesture::LongPress { pos: p(50.0, 200.0) }], PLOT);
        assert_eq!(t.ops(&[Gesture::LongPressDrag { pos: p(80.0, 220.0), delta: (30.0, 20.0) }], PLOT), [ChartOp::Crosshair(Some((80.0, 120.0)))]);
    }
}
