//! One mylittlechart chart as a uzor-framework [`App`]: the phone screen of
//! [`screen`] with the chart painted into its slot through uzor's `RenderContext`
//! ([`paint`]), driven by fingers and the mouse ([`input`]) over the view state of
//! [`model`]. Bars come from a [`source::Source`]: demo candles, or (feature `live`)
//! one exchange through mlc's data bridge.
//!
//! Shells: `mlc-shell-web2` (browser, uzor-framework/web) and `mlc-shell-mobile`
//! (Android/iOS through uzor-window-mobile). Like hq-uzor: this crate is the
//! host-agnostic app; shells add only the host entry and I/O.

pub mod demo;
pub mod i18n;
pub mod input;
#[cfg(feature = "live")]
pub mod live;
pub mod model;
pub mod paint;
pub mod screen;
pub mod source;

#[cfg(test)]
mod shots;

use uzor::input::Sense;
use uzor::layout::docking::DockPanel;
use uzor::render::TickRate;
use uzor::tokens::{BuiltinSet, Tokens};
use uzor::{Rect, WidgetId};
use uzor_framework::widgets::kit;
use uzor_framework::{
    App, AppCommand, ChromeModel, DockTarget, InitCx, Intent, IntentCx, LayoutCmd, OverlayCx, OverlayModel, PanelCx, PanelHome, RuntimeConfig, SizePx, Spec, WindowId,
    WindowIntent, WindowSpec,
};

use crate::demo::TIMEFRAMES;
use crate::i18n::{t, K};
use crate::input::{ChartInput, ChartOp};
use crate::model::ChartModel;
use crate::source::{DemoSource, Feed, Source};

/// The canvas element id the web shell draws into.
pub const CANVAS_ID: &str = "mlc-canvas";
/// Timeframe shown first (`1h`).
pub const DEFAULT_TF: usize = 3;

/// The single panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChartPanel;
impl DockPanel for ChartPanel {
    fn title(&self) -> &str {
        "Chart"
    }
    fn type_id(&self) -> &'static str {
        "chart"
    }
    fn closable(&self) -> bool {
        false
    }
    fn shows_header(&self) -> bool {
        false
    }
}

/// No overlays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChartOverlay {}
/// No actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChartAction {}

/// The app's vocabulary.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartSpec;
impl Spec for ChartSpec {
    type Panel = ChartPanel;
    type Overlay = ChartOverlay;
    type Action = ChartAction;
    fn decode_panel(_home: PanelHome, _type_id: &str) -> Option<ChartPanel> {
        Some(ChartPanel)
    }
}

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

/// The chart app.
pub struct ChartViewApp {
    pub model: ChartModel,
    pub input: ChartInput,
    pub tf: usize,
    pub shown: Shown,
    source: Box<dyn Source>,
    live_source: bool,
    requested_at: Option<f64>,
    sheet: Option<(u64, uzor::ui::style::Sheet)>,
    win: Option<WindowId>,
    waker_win: Option<std::sync::Arc<std::sync::Mutex<Option<WindowId>>>>,
    /// Hit targets and the chart rect of the last frame (tests, dev control).
    pub last_hits: Vec<(String, Rect)>,
    pub last_chart: Option<Rect>,
    demo_end: i64,
}

impl ChartViewApp {
    /// Demo candles ending at `end` (unix seconds).
    pub fn demo(end: i64) -> Self {
        Self::with_source(Box::new(DemoSource::new(end)), false, end)
    }

    /// Bars from `source`; `live` says it is a network source (demo fallback on timeout).
    pub fn with_source(source: Box<dyn Source>, live: bool, demo_end: i64) -> Self {
        let mut app = Self {
            model: ChartModel::default(),
            input: ChartInput::default(),
            tf: DEFAULT_TF,
            shown: Shown::Loading,
            source,
            live_source: live,
            requested_at: None,
            sheet: None,
            win: None,
            waker_win: None,
            last_hits: Vec::new(),
            last_chart: None,
            demo_end,
        };
        app.source.request(TIMEFRAMES[DEFAULT_TF].0);
        app
    }

    /// Live Binance BTC/USDT (feature `live`), demo bars if it does not answer.
    #[cfg(feature = "live")]
    pub fn live(demo_end: i64) -> Self {
        Self::with_source(Box::new(live::LiveSource::new()), true, demo_end)
    }

    /// One phone-sized window (390×844 logical px where the host sizes it), dark,
    /// painted on demand.
    pub fn runtime_config() -> RuntimeConfig {
        let mut spec = WindowSpec::new("main", "mylittlechart".to_string(), SizePx::new(390, 844));
        spec.decorations = false;
        RuntimeConfig { windows: vec![spec], tokens: Tokens::builtin(BuiltinSet::Dark), dark: true, tick: TickRate::Dirty, ..RuntimeConfig::default() }
    }

    fn set_tf(&mut self, tf: usize, now: f64) {
        if tf != self.tf || self.shown == Shown::Offline {
            self.tf = tf;
            self.requested_at = Some(now);
            self.source.request(TIMEFRAMES[tf].0);
        }
    }

    /// Apply what the source delivered; fall back to demo on timeout.
    pub fn pump(&mut self, now: f64) {
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
            #[cfg(feature = "live")]
            let timeout = live::TIMEOUT_S;
            #[cfg(not(feature = "live"))]
            let timeout = 12.0;
            if now - since > timeout {
                self.requested_at = None;
                if self.shown == Shown::Loading {
                    self.fallback();
                }
            }
        }
    }

    fn fallback(&mut self) {
        self.model.set_bars(demo::series(self.tf, self.demo_end));
        self.shown = Shown::Offline;
        self.requested_at = None;
    }

    /// Apply one chart op for a plot `width` px wide.
    pub fn apply(&mut self, op: ChartOp, width: f64) {
        match op {
            ChartOp::Pan(dx) => self.model.pan(dx, width),
            ChartOp::Zoom { factor, anchor } => self.model.zoom(factor, anchor, width),
            ChartOp::Crosshair(c) => self.model.crosshair = c,
            ChartOp::Reset => self.model.reset(),
        }
    }

    /// The faint note in the plot corner.
    pub fn plot_label(&self) -> Option<&'static str> {
        match self.shown {
            Shown::Demo => Some(t(K::Demo)),
            Shown::Offline => Some(t(K::Offline)),
            _ => None,
        }
    }

    fn sheet(&mut self) -> &uzor::ui::style::Sheet {
        let gen = uzor::ui::media::generation();
        if self.sheet.as_ref().is_none_or(|(g, _)| *g != gen) {
            let s = uzor::ui::mobile::sheet_with(screen::APP_CSS).expect("app css parses");
            self.sheet = Some((gen, s));
        }
        &self.sheet.as_ref().expect("sheet").1
    }

    /// Lay out, paint and wire one frame into `cx.rect`.
    fn frame(&mut self, cx: &mut PanelCx<'_, ChartSpec>) {
        let now = cx.time.now.0;
        self.pump(now);
        let header = screen::Header { symbol: self.source.symbol(), venue: self.source.venue(), live: self.shown == Shown::Live };
        let tree = screen::tree(&header, self.tf).css(format!("height: {}px", cx.rect.height));
        let rect = cx.rect;
        let sheet = self.sheet().clone();
        let resp = kit::dom(rect, &sheet, tree).build(cx);
        match resp.clicked.as_deref() {
            Some(screen::RESET_ID) => self.model.reset(),
            Some(id) => {
                if let Some(tf) = screen::picked_tf(id) {
                    self.set_tf(tf, now);
                }
            }
            None => {}
        }
        self.last_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        let Some(chart) = resp.dom.slots().find(|(k, _)| *k == screen::CHART_SLOT).map(|(_, r)| r) else { return };
        self.last_chart = Some(chart);
        let plot = paint::areas(chart).plot;
        let id = WidgetId::from(screen::CHART_ID);
        // The chart takes clicks, drags and the wheel; `touch` + `gesture` ask uzor
        // for its fingers (touch capture, `uzor-next`).
        let mut sense = Sense::CLICK_AND_DRAG.union(Sense::SCROLL);
        sense.touch = true;
        sense.gesture = true;
        cx.widgets.coord.register(id.clone(), chart, sense);
        for op in self.input.update(cx.view, &id, plot, now) {
            self.apply(op, plot.width);
        }
        paint::chart(cx.render, chart, &self.model, self.plot_label());
        // A requested series arrives on a later frame (demo: the next one; live: the
        // waker, or the timeout below while loading).
        if self.requested_at.is_some() || self.shown == Shown::Loading {
            cx.out.request_frame();
        }
    }
}

impl App for ChartViewApp {
    type Spec = ChartSpec;

    fn init(&mut self, cx: &mut InitCx<'_, ChartSpec>) {
        let h = cx.handle();
        let win = std::sync::Arc::new(std::sync::Mutex::new(None::<WindowId>));
        let w2 = win.clone();
        self.source.set_waker(Box::new(move || {
            if let Some(w) = *w2.lock().unwrap_or_else(|e| e.into_inner()) {
                let _ = h.request_repaint(w);
            }
        }));
        self.waker_win = Some(win);
    }

    fn chrome_model(&self, _win: WindowId) -> ChromeModel {
        ChromeModel::default()
    }

    fn overlay_model(&self, _win: WindowId, id: ChartOverlay) -> OverlayModel {
        match id {}
    }

    fn panel(&mut self, cx: &mut PanelCx<'_, ChartSpec>) {
        self.frame(cx);
    }

    fn overlay_body(&mut self, cx: &mut OverlayCx<'_, ChartSpec>) {
        match cx.id {}
    }

    fn intent(&mut self, intent: Intent<ChartSpec>, cx: &mut IntentCx<'_, ChartSpec>) {
        if let Intent::Window(WindowIntent::Opened { win, .. }) = intent {
            self.win = Some(win);
            if let Some(w) = &self.waker_win {
                *w.lock().unwrap_or_else(|e| e.into_inner()) = Some(win);
            }
            cx.command(AppCommand::Layout(LayoutCmd::SetChrome { win, visible: false, height: 0.0 }));
            cx.command(AppCommand::Layout(LayoutCmd::OpenPanel { win, panel: ChartPanel, at: DockTarget::Root }));
        }
    }

    #[cfg(feature = "dev-control")]
    fn automation_state(&self) -> Option<String> {
        Some(
            serde_json::json!({
                "tf": TIMEFRAMES[self.tf].0,
                "shown": format!("{:?}", self.shown),
                "bars": self.model.bars.len(),
                "count": self.model.count,
                "right": self.model.right,
                "crosshair": self.model.crosshair,
                "follow": self.model.follow,
            })
            .to_string(),
        )
    }
}

#[cfg(test)]
mod tests;
