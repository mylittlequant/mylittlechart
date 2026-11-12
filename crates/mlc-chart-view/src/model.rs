//! The chart's view state: which bars are on screen and how they map to pixels.
//! Pure math, no rendering and no input: pan, zoom around a point, reset, the
//! crosshair, and the price range of what is visible.

use mlc_core::Bar;

/// Bars visible after a reset.
pub const DEFAULT_BARS: f64 = 80.0;
/// Fewest bars a zoom-in may show.
pub const MIN_BARS: f64 = 12.0;
/// Most bars a zoom-out may show.
pub const MAX_BARS: f64 = 600.0;
/// Empty bars kept right of the last bar after a reset.
pub const RIGHT_MARGIN: f64 = 3.0;

/// The visible window over a list of bars.
///
/// `right` is the bar index (fractional) at the right edge of the plot, `count`
/// how many bars fit across it. Bar `i` is centred at `x(i)`.
#[derive(Clone, Debug)]
pub struct ChartModel {
    pub bars: Vec<Bar>,
    pub right: f64,
    pub count: f64,
    /// Crosshair position in plot coordinates (px from the plot's left/top).
    pub crosshair: Option<(f64, f64)>,
    /// The view follows new bars (true until the user pans away from the end).
    pub follow: bool,
}

impl Default for ChartModel {
    fn default() -> Self {
        Self { bars: Vec::new(), right: RIGHT_MARGIN, count: DEFAULT_BARS, crosshair: None, follow: true }
    }
}

impl ChartModel {
    pub fn new(bars: Vec<Bar>) -> Self {
        let mut m = Self { bars, ..Self::default() };
        m.reset();
        m
    }

    /// Replace the bars (a new symbol or timeframe): back to the default view.
    pub fn set_bars(&mut self, bars: Vec<Bar>) {
        self.bars = bars;
        self.reset();
    }

    /// A live update: the last bar changed or a new one opened. A following view
    /// moves with it.
    pub fn push_bar(&mut self, bar: Bar) {
        match self.bars.last_mut() {
            Some(last) if last.timestamp == bar.timestamp => *last = bar,
            Some(last) if last.timestamp > bar.timestamp => {}
            _ => {
                self.bars.push(bar);
                if self.follow {
                    self.right += 1.0;
                }
            }
        }
    }

    /// The default view: [`DEFAULT_BARS`] wide, the last bar [`RIGHT_MARGIN`] bars
    /// from the right edge, no crosshair.
    pub fn reset(&mut self) {
        self.count = DEFAULT_BARS;
        self.right = self.bars.len() as f64 - 1.0 + RIGHT_MARGIN;
        self.crosshair = None;
        self.follow = true;
    }

    /// Width of one bar slot in a plot `width` px wide.
    pub fn bar_w(&self, width: f64) -> f64 {
        width / self.count.max(1.0)
    }

    /// Centre x of bar `i` in a plot `width` px wide.
    pub fn x(&self, i: f64, width: f64) -> f64 {
        let w = self.bar_w(width);
        width - (self.right - i) * w - w / 2.0
    }

    /// The (fractional) bar index at plot x.
    pub fn index_at(&self, x: f64, width: f64) -> f64 {
        let w = self.bar_w(width);
        self.right - (width - x - w / 2.0) / w
    }

    /// Indices of the bars at least partly visible.
    pub fn visible(&self) -> std::ops::Range<usize> {
        let n = self.bars.len();
        let lo = (self.right - self.count).floor().max(0.0) as usize;
        let hi = (self.right.ceil() + 1.0).max(0.0) as usize;
        lo.min(n)..hi.min(n)
    }

    /// Low and high of the visible bars, padded by 8 % (a flat range opens to ±1 %).
    pub fn price_range(&self) -> Option<(f64, f64)> {
        let r = self.visible();
        let vis = &self.bars[r];
        if vis.is_empty() {
            return None;
        }
        let lo = vis.iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
        let hi = vis.iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
        let span = hi - lo;
        let pad = if span > 0.0 { span * 0.08 } else { hi.abs().max(1.0) * 0.01 };
        Some((lo - pad, hi + pad))
    }

    /// Pan by `dx` px (positive: drag to the right, older bars come in).
    pub fn pan(&mut self, dx: f64, width: f64) {
        if width <= 0.0 {
            return;
        }
        self.right -= dx / self.bar_w(width);
        self.clamp();
        self.follow = self.right >= self.bars.len() as f64 - 1.0;
    }

    /// Zoom by `factor` (> 1: in, fewer bars) keeping the bar under plot x `anchor` put.
    pub fn zoom(&mut self, factor: f64, anchor: f64, width: f64) {
        if width <= 0.0 || !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let at = self.index_at(anchor, width);
        self.count = (self.count / factor).clamp(MIN_BARS, MAX_BARS);
        // Put `at` back under the anchor.
        let w = self.bar_w(width);
        self.right = at + (width - anchor - w / 2.0) / w;
        self.clamp();
    }

    /// Keep at least a few bars on screen whichever way the view went.
    fn clamp(&mut self) {
        let n = self.bars.len() as f64;
        let min_right = (MIN_BARS / 2.0).min(n - 1.0).max(0.0);
        let max_right = n - 1.0 + self.count - MIN_BARS / 2.0;
        self.right = self.right.clamp(min_right, max_right.max(min_right));
    }

    /// The bar under plot x (the crosshair's), if any.
    pub fn bar_at(&self, x: f64, width: f64) -> Option<(usize, &Bar)> {
        let i = self.index_at(x, width).round();
        (i >= 0.0 && (i as usize) < self.bars.len()).then(|| (i as usize, &self.bars[i as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn bars(n: usize) -> Vec<Bar> {
        (0..n).map(|i| Bar { timestamp: 1_700_000_000 + i as i64 * 3600, open: 100.0 + i as f64, high: 102.0 + i as f64, low: 99.0 + i as f64, close: 101.0 + i as f64, volume: 1.0, ..Default::default() }).collect()
    }

    #[test]
    fn reset_shows_the_last_bars_with_a_margin() {
        let m = ChartModel::new(bars(500));
        assert_eq!(m.count, DEFAULT_BARS);
        let x_last = m.x(499.0, 400.0);
        let w = m.bar_w(400.0);
        assert!((400.0 - x_last - w / 2.0 - RIGHT_MARGIN * w).abs() < 1e-9, "3 empty slots right of the last bar");
        assert_eq!(m.visible().end, 500);
        assert!(m.visible().len() as f64 <= DEFAULT_BARS + 2.0);
    }

    #[test]
    fn index_and_x_are_inverse() {
        let m = ChartModel::new(bars(300));
        for x in [0.0, 13.5, 200.0, 399.0] {
            assert!((m.x(m.index_at(x, 400.0), 400.0) - x).abs() < 1e-9);
        }
    }

    #[test]
    fn pan_right_reveals_older_bars_and_stops_following() {
        let mut m = ChartModel::new(bars(500));
        let w = m.bar_w(400.0);
        m.pan(10.0 * w, 400.0);
        assert!((m.right - (499.0 + RIGHT_MARGIN - 10.0)).abs() < 1e-9);
        assert!(!m.follow);
        m.pan(-1e6, 400.0);
        assert!(m.right <= 499.0 + m.count, "clamped at the end");
        m.pan(1e9, 400.0);
        assert!(m.right >= MIN_BARS / 2.0 - 1e-9, "clamped at the start");
    }

    #[test]
    fn zoom_keeps_the_anchor_bar_in_place_and_clamps() {
        let mut m = ChartModel::new(bars(500));
        let anchor = 150.0;
        let before = m.index_at(anchor, 400.0);
        m.zoom(2.0, anchor, 400.0);
        assert_eq!(m.count, DEFAULT_BARS / 2.0);
        assert!((m.index_at(anchor, 400.0) - before).abs() < 1e-9);
        m.zoom(100.0, anchor, 400.0);
        assert_eq!(m.count, MIN_BARS);
        m.zoom(0.0001, anchor, 400.0);
        assert_eq!(m.count, MAX_BARS);
    }

    #[test]
    fn price_range_pads_the_visible_bars() {
        let m = ChartModel::new(bars(500));
        let (lo, hi) = m.price_range().unwrap();
        let r = m.visible();
        let vlo = m.bars[r.clone()].iter().map(|b| b.low).fold(f64::INFINITY, f64::min);
        let vhi = m.bars[r].iter().map(|b| b.high).fold(f64::NEG_INFINITY, f64::max);
        assert!(lo < vlo && hi > vhi);
        assert!(ChartModel::default().price_range().is_none());
    }

    #[test]
    fn live_bars_update_in_place_or_append_and_follow() {
        let mut m = ChartModel::new(bars(10));
        let r0 = m.right;
        let mut b = m.bars[9];
        b.close = 1.0;
        m.push_bar(b);
        assert_eq!((m.bars.len(), m.bars[9].close), (10, 1.0));
        b.timestamp += 3600;
        m.push_bar(b);
        assert_eq!(m.bars.len(), 11);
        assert_eq!(m.right, r0 + 1.0);
    }

    #[test]
    fn reset_clears_the_crosshair() {
        let mut m = ChartModel::new(bars(100));
        m.crosshair = Some((1.0, 2.0));
        m.zoom(3.0, 10.0, 400.0);
        m.reset();
        let d = ChartModel::new(bars(100));
        assert_eq!((m.right, m.count, m.crosshair, m.follow), (d.right, d.count, d.crosshair, d.follow));
    }
}
