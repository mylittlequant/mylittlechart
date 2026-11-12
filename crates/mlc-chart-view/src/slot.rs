//! The chart as one reusable paint slot: where its bars come from ([`Source`]), its
//! view state ([`ChartModel`]), fingers and mouse ([`ChartInput`]) and its painter.
//! It is a uzor [`SlotPainter`]: register it in a [`uzor::ui::dom::SlotRegistry`] under
//! the key of a DOM `El::paint_slot` and hand the registry to the tree
//! (`DomBuilder::slot_registry`); it paints there and takes the slot's input as
//! [`SlotEvent`]s. The phone app ([`crate::ChartViewApp`]) and the desktop app
//! (`mlc-desktop`) both do.

use std::sync::Arc;

use uzor::render::RenderContext;
use uzor::ui::dom::{SlotAction, SlotEvent, SlotPaint, SlotPainter};
use uzor::Rect;

use crate::demo::{self, TIMEFRAMES};
use crate::i18n::{t, K};
use crate::input::{ChartInput, ChartOp};
use crate::model::ChartModel;
use crate::paint;
use crate::source::{Feed, Source};

/// Timeframe shown first (`1h`).
pub const DEFAULT_TF: usize = 3;

/// Where the shown bars came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Waiting for the first bars.
    Loading,
    Demo,
    /// Live source failed or timed out: demo bars, said so.
    Offline,
    Live,
}

/// Seconds a live source may take before the chart falls back to demo bars.
#[cfg(feature = "live")]
const TIMEOUT_S: f64 = crate::live::TIMEOUT_S;
#[cfg(not(feature = "live"))]
const TIMEOUT_S: f64 = 12.0;

/// One chart: source, view, input, painter.
pub struct ChartSlot {
    pub model: ChartModel,
    pub input: ChartInput,
    /// Timeframe index into [`TIMEFRAMES`].
    pub tf: usize,
    pub shown: Shown,
    source: Box<dyn Source>,
    live_source: bool,
    requested_at: Option<f64>,
    demo_end: i64,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The slot's content box on the last frame (tests, dev control).
    pub last_rect: Option<Rect>,
    /// The pointer is a finger or a pen (set by the host each frame): no hover
    /// crosshair, no wheel zoom.
    pub coarse: bool,
    /// The time of the last [`ChartSlot::pump`] (double clicks are timed with it).
    now: f64,
}

/// Apply one chart op to `model` for a plot `width` px wide.
pub fn apply_op(model: &mut ChartModel, op: ChartOp, width: f64) {
    match op {
        ChartOp::Pan(dx) => model.pan(dx, width),
        ChartOp::Zoom { factor, anchor } => model.zoom(factor, anchor, width),
        ChartOp::Crosshair(c) => model.crosshair = c,
        ChartOp::Reset => model.reset(),
    }
}

impl ChartSlot {
    /// Bars from `source`; `live` says it is a network source (demo fallback on
    /// timeout); demo bars end at `demo_end` (unix seconds).
    pub fn new(source: Box<dyn Source>, live: bool, demo_end: i64) -> Self {
        let mut s = Self {
            model: ChartModel::default(),
            input: ChartInput::default(),
            tf: DEFAULT_TF,
            shown: Shown::Loading,
            source,
            live_source: live,
            requested_at: None,
            demo_end,
            waker: None,
            last_rect: None,
            coarse: false,
            now: 0.0,
        };
        s.source.request(TIMEFRAMES[DEFAULT_TF].0);
        s
    }

    /// The source (symbol and venue for the header).
    pub fn source(&self) -> &dyn Source {
        self.source.as_ref()
    }

    /// Show another source (another symbol) at the current timeframe; the view
    /// starts over on its bars.
    pub fn set_source(&mut self, source: Box<dyn Source>, live: bool, now: f64) {
        self.source = source;
        self.live_source = live;
        self.shown = Shown::Loading;
        self.model = ChartModel::default();
        self.input = ChartInput::default();
        if let Some(w) = &self.waker {
            let w = w.clone();
            self.source.set_waker(Box::new(move || w()));
        }
        self.requested_at = Some(now);
        self.source.request(TIMEFRAMES[self.tf].0);
    }

    /// How to wake the app when the source delivers off-frame.
    pub fn set_waker(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        let w = wake.clone();
        self.source.set_waker(Box::new(move || w()));
        self.waker = Some(wake);
    }

    /// Switch to timeframe `tf` (index into [`TIMEFRAMES`]); asks again when the
    /// last answer was the offline fallback.
    pub fn set_tf(&mut self, tf: usize, now: f64) {
        if tf < TIMEFRAMES.len() && (tf != self.tf || self.shown == Shown::Offline) {
            self.tf = tf;
            self.requested_at = Some(now);
            self.source.request(TIMEFRAMES[tf].0);
        }
    }

    /// Apply what the source delivered; fall back to demo on timeout.
    pub fn pump(&mut self, now: f64) {
        self.now = now;
        for f in self.source.poll() {
            match f {
                Feed::Bars { tf, bars, live } => {
                    if TIMEFRAMES[self.tf].0 == tf {
                        self.model.set_bars(bars);
                        self.shown = if live { Shown::Live } else if self.live_source { Shown::Offline } else { Shown::Demo };
                        self.requested_at = None;
                    }
                }
                Feed::Bar(b) if self.shown == Shown::Live => self.model.push_bar(b),
                Feed::Bar(_) => {}
                Feed::Offline => self.fallback(),
            }
        }
        if self.live_source && self.shown != Shown::Live {
            let since = *self.requested_at.get_or_insert(now);
            if now - since > TIMEOUT_S {
                self.requested_at = None;
                if self.shown == Shown::Loading {
                    self.fallback();
                }
            }
        }
    }

    fn fallback(&mut self) {
        let (price, seed) = demo::level_for(self.source.symbol());
        self.model.set_bars(demo::series_for(self.tf, self.demo_end, price, seed));
        self.shown = Shown::Offline;
        self.requested_at = None;
    }

    /// Apply one chart op for a plot `width` px wide.
    pub fn apply(&mut self, op: ChartOp, width: f64) {
        apply_op(&mut self.model, op, width);
    }

    /// The faint note in the plot corner.
    pub fn plot_label(&self) -> Option<&'static str> {
        match self.shown {
            Shown::Demo => Some(t(K::Demo)),
            Shown::Offline => Some(t(K::Offline)),
            _ => None,
        }
    }

    /// A requested series is still on its way: keep frames coming.
    pub fn wants_frame(&self) -> bool {
        self.requested_at.is_some() || self.shown == Shown::Loading
    }

    fn title(&self) -> String {
        format!("{} · {} · {}", self.source.symbol(), self.source.venue(), TIMEFRAMES[self.tf].0)
    }
}

impl SlotPainter for ChartSlot {
    fn paint(&mut self, ctx: &mut dyn RenderContext, f: &SlotPaint<'_>) {
        paint::chart(ctx, f.rect, &self.model, self.plot_label());
        self.last_rect = Some(f.rect);
    }

    fn event(&mut self, ev: &SlotEvent, f: &SlotPaint<'_>) -> Option<SlotAction> {
        let plot = paint::areas(f.rect).plot;
        for op in self.input.event(ev, f.rect, plot, self.now, self.coarse) {
            apply_op(&mut self.model, op, plot.width);
        }
        None
    }

    fn role(&self) -> &'static str {
        "figure"
    }

    fn name(&self) -> Option<String> {
        Some(self.title())
    }

    fn wants_frame(&self) -> bool {
        ChartSlot::wants_frame(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::DemoSource;

    #[test]
    fn switching_the_source_keeps_the_timeframe_and_starts_over() {
        let mut s = ChartSlot::new(Box::new(DemoSource::new(1_762_905_600)), false, 1_762_905_600);
        s.pump(0.0);
        assert_eq!(s.shown, Shown::Demo);
        s.set_tf(1, 0.1);
        s.pump(0.1);
        s.apply(ChartOp::Pan(200.0), 300.0);
        assert!(!s.model.follow);
        s.set_source(Box::new(DemoSource::for_symbol("ETH/USDT", "Bybit", 1_762_905_600)), false, 0.2);
        assert_eq!(s.shown, Shown::Loading);
        s.pump(0.2);
        assert_eq!((s.tf, s.shown, s.source().symbol(), s.source().venue()), (1, Shown::Demo, "ETH/USDT", "Bybit"));
        assert!(s.model.follow, "a new symbol starts at its last bars");
        assert!(s.model.bars[0].close < 10_000.0, "its own price level");
        assert!(!s.wants_frame());
    }
}
