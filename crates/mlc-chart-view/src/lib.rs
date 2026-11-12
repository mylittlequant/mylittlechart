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
pub mod slot;
pub mod source;

pub use slot::{apply_op, ChartSlot, Shown, DEFAULT_TF};

#[cfg(test)]
mod shots;

use uzor::input::Sense;
use uzor::layout::docking::DockPanel;
use uzor::render::TickRate;
use uzor::ui::dom::SlotRegistry;
use uzor::tokens::{BuiltinSet, Tokens};
use uzor::{Rect, WidgetId};
use uzor_framework::widgets::kit;
use uzor_framework::{
    App, AppCommand, ChromeModel, DockTarget, InitCx, Intent, IntentCx, LayoutCmd, OverlayCx, OverlayModel, PanelCx, PanelHome, RuntimeConfig, SizePx, Spec, WindowId,
    WindowIntent, WindowSpec,
};

#[cfg(feature = "dev-control")]
use crate::demo::TIMEFRAMES;
use crate::source::{DemoSource, Source};

/// The canvas element id the web shell draws into.
pub const CANVAS_ID: &str = "mlc-canvas";

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

/// The chart app.
pub struct ChartViewApp {
    /// The chart ([`ChartSlot`], under [`screen::CHART_SLOT`]): the tree's slot
    /// painters.
    pub slots: SlotRegistry,
    sheet: Option<(u64, uzor::ui::style::Sheet)>,
    win: Option<WindowId>,
    waker_win: std::sync::Arc<std::sync::Mutex<Option<WindowId>>>,
    /// Hit targets of the last frame (tests, dev control).
    pub last_hits: Vec<(String, Rect)>,
}

impl ChartViewApp {
    /// Demo candles ending at `end` (unix seconds).
    pub fn demo(end: i64) -> Self {
        Self::with_source(Box::new(DemoSource::new(end)), false, end)
    }

    /// Bars from `source`; `live` says it is a network source (demo fallback on timeout).
    pub fn with_source(source: Box<dyn Source>, live: bool, demo_end: i64) -> Self {
        let mut slots = SlotRegistry::new();
        slots.register(screen::CHART_SLOT, ChartSlot::new(source, live, demo_end));
        Self { slots, sheet: None, win: None, waker_win: Default::default(), last_hits: Vec::new() }
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

    /// The chart: source, view, input, painter.
    pub fn chart(&self) -> &ChartSlot {
        self.slots.get(screen::CHART_SLOT).expect("the chart is registered")
    }

    /// The chart, mutable.
    pub fn chart_mut(&mut self) -> &mut ChartSlot {
        self.slots.get_mut(screen::CHART_SLOT).expect("the chart is registered")
    }

    /// The chart's content box on the last frame.
    pub fn last_chart(&self) -> Option<Rect> {
        self.chart().last_rect
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
        let coarse = cx.view.coarse_pointer();
        let chart = self.chart_mut();
        chart.pump(now);
        chart.coarse = coarse;
        let tree = {
            let chart = self.chart();
            let src = chart.source();
            let header = screen::Header { symbol: src.symbol(), venue: src.venue(), live: chart.shown == Shown::Live };
            screen::tree(&header, chart.tf).css(format!("height: {}px", cx.rect.height))
        };
        let rect = cx.rect;
        let sheet = self.sheet().clone();
        let id = WidgetId::from(screen::CHART_SLOT);
        // The chart paints itself in its slot and takes the slot's input there.
        let resp = kit::dom(rect, &sheet, tree).slot_registry(&mut self.slots).build(cx);
        match resp.clicked.as_deref() {
            Some(screen::RESET_ID) => self.chart_mut().model.reset(),
            Some(id) => {
                if let Some(tf) = screen::picked_tf(id) {
                    self.chart_mut().set_tf(tf, now);
                }
            }
            None => {}
        }
        self.last_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        if let Some(r) = self.chart_mut().last_rect {
            // The DOM registers the slot for clicks, hover and its fingers; the chart
            // also takes the mouse drag and the wheel.
            cx.widgets.coord.register(id, r, Sense::CLICK_AND_DRAG.union(Sense::SCROLL).with_touch().with_gesture());
        }
        // A requested series arrives on a later frame (demo: the next one; live: the
        // waker, or the timeout while loading); a click changed what this frame's tree
        // showed.
        if resp.clicked.is_some() || self.chart_mut().wants_frame() {
            cx.out.request_frame();
        }
    }
}

impl App for ChartViewApp {
    type Spec = ChartSpec;

    fn init(&mut self, cx: &mut InitCx<'_, ChartSpec>) {
        let h = cx.handle();
        let win = self.waker_win.clone();
        self.chart_mut().set_waker(std::sync::Arc::new(move || {
            if let Some(w) = *win.lock().unwrap_or_else(|e| e.into_inner()) {
                let _ = h.request_repaint(w);
            }
        }));
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
            *self.waker_win.lock().unwrap_or_else(|e| e.into_inner()) = Some(win);
            cx.command(AppCommand::Layout(LayoutCmd::SetChrome { win, visible: false, height: 0.0 }));
            cx.command(AppCommand::Layout(LayoutCmd::OpenPanel { win, panel: ChartPanel, at: DockTarget::Root }));
        }
    }

    #[cfg(feature = "dev-control")]
    fn automation_state(&self) -> Option<String> {
        Some(
            serde_json::json!({
                "tf": TIMEFRAMES[self.chart().tf].0,
                "shown": format!("{:?}", self.chart().shown),
                "bars": self.chart().model.bars.len(),
                "count": self.chart().model.count,
                "right": self.chart().model.right,
                "crosshair": self.chart().model.crosshair,
                "follow": self.chart().model.follow,
            })
            .to_string(),
        )
    }
}

#[cfg(test)]
mod tests;
