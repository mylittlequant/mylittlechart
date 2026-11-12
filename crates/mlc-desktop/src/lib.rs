//! mylittlechart as one uzor-framework [`App`] — phase 1 of moving the desktop app
//! off its hand-drawn UI and `InputCoordinator` loop (docs/uzor-framework-migration.md).
//!
//! One window: a DOM toolbar, the chart in a paint slot ([`mlc_chart_view::ChartSlot`],
//! the same chart the phone app shows) and a watchlist column on uzor's desktop kit.
//! Host-agnostic: `mlc-shell-native` runs it on the desktop; the web and phone shells
//! move onto it in later phases. The old app (`mlc-app*`) is untouched.

pub mod i18n;
pub mod screen;
pub mod watchlist;

#[cfg(test)]
mod shots;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

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

use mlc_chart_view::source::{DemoSource, Source};
use mlc_chart_view::{ChartSlot, Shown};

use crate::i18n::{t, K};
use crate::screen::Sym;
use crate::watchlist::Watchlist;

/// The window's one panel: the whole screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MainPanel;
impl DockPanel for MainPanel {
    fn title(&self) -> &str {
        "mylittlechart"
    }
    fn type_id(&self) -> &'static str {
        "main"
    }
    fn closable(&self) -> bool {
        false
    }
    fn shows_header(&self) -> bool {
        false
    }
}

/// No overlays yet (modals arrive in phase 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DesktopOverlay {}
/// No actions yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DesktopAction {}

/// The app's vocabulary.
#[derive(Clone, Copy, Debug, Default)]
pub struct DesktopSpec;
impl Spec for DesktopSpec {
    type Panel = MainPanel;
    type Overlay = DesktopOverlay;
    type Action = DesktopAction;
    fn decode_panel(_home: PanelHome, _type_id: &str) -> Option<MainPanel> {
        Some(MainPanel)
    }
}

/// Makes the bar source of a watchlist symbol: `(source, is_live)`.
pub type SourceFactory = Box<dyn Fn(&Sym, i64) -> (Box<dyn Source>, bool)>;

/// Demo bars for any symbol.
pub fn demo_sources() -> SourceFactory {
    Box::new(|s: &Sym, end: i64| (Box::new(DemoSource::for_symbol(&s.symbol, &s.venue, end)) as Box<dyn Source>, false))
}

/// The desktop app.
pub struct DesktopApp {
    /// The chart ([`ChartSlot`], under [`screen::CHART_SLOT`]): the tree's slot
    /// painters.
    pub slots: SlotRegistry,
    pub watch: Watchlist,
    sources: SourceFactory,
    demo_end: i64,
    sheet: Option<(u64, uzor::ui::style::Sheet)>,
    win: Option<WindowId>,
    waker_win: Arc<Mutex<Option<WindowId>>>,
    /// Hit targets of the last frame (tests, dev control).
    pub last_hits: Vec<(String, Rect)>,
}

impl DesktopApp {
    /// Demo bars ending at `end` (unix seconds) for the default watchlist.
    pub fn demo(end: i64) -> Self {
        Self::new(Watchlist::default_symbols(), demo_sources(), end)
    }

    /// `symbols` in the watchlist, the chart on the first one, bars from `sources`.
    pub fn new(symbols: Vec<Sym>, sources: SourceFactory, demo_end: i64) -> Self {
        let first = symbols.first().cloned().unwrap_or(Sym { symbol: "BTC/USDT".into(), venue: "Binance".into() });
        let (src, live) = sources(&first, demo_end);
        let mut slots = SlotRegistry::new();
        slots.register(screen::CHART_SLOT, ChartSlot::new(src, live, demo_end));
        Self {
            slots,
            watch: Watchlist::new(symbols),
            sources,
            demo_end,
            sheet: None,
            win: None,
            waker_win: Arc::new(Mutex::new(None)),
            last_hits: Vec::new(),
        }
    }

    /// One desktop window, 1280×800, dark, painted on demand.
    pub fn runtime_config() -> RuntimeConfig {
        Self::runtime_config_sized(1280, 800)
    }

    /// One desktop window of `w`×`h` logical px.
    pub fn runtime_config_sized(w: u32, h: u32) -> RuntimeConfig {
        let spec = WindowSpec::new("main", "mylittlechart".to_string(), SizePx::new(w, h));
        RuntimeConfig { windows: vec![spec], tokens: Tokens::builtin(BuiltinSet::Dark), dark: true, tick: TickRate::Dirty, ..RuntimeConfig::default() }
    }

    /// Put watchlist entry `i` (into the whole list) on the chart.
    pub fn open_symbol(&mut self, i: usize, now: f64) {
        let Some(sym) = self.watch.all.get(i).cloned() else { return };
        if self.watch.current == Some(i) {
            return;
        }
        let (src, live) = (self.sources)(&sym, self.demo_end);
        self.chart_mut().set_source(src, live, now);
        self.watch.current = Some(i);
    }

    fn status(&self) -> &'static str {
        match self.chart().shown {
            Shown::Loading => t(K::Loading),
            Shown::Demo => t(K::Demo),
            Shown::Offline => t(K::Offline),
            Shown::Live => t(K::Live),
        }
    }

    fn sheet(&mut self) -> uzor::ui::style::Sheet {
        let gen = uzor::ui::media::generation();
        if self.sheet.as_ref().is_none_or(|(g, _)| *g != gen) {
            let s = uzor::ui::domkit::sheet_with(screen::APP_CSS).expect("app css parses");
            self.sheet = Some((gen, s));
        }
        self.sheet.as_ref().expect("sheet").1.clone()
    }

    /// The chart: source, view, input, painter.
    pub fn chart(&self) -> &ChartSlot {
        self.slots.get(screen::CHART_SLOT).expect("the chart is registered")
    }

    /// The chart, mutable.
    pub fn chart_mut(&mut self) -> &mut ChartSlot {
        self.slots.get_mut(screen::CHART_SLOT).expect("the chart is registered")
    }

    /// Lay out, paint and wire one frame into `cx.rect`.
    fn frame(&mut self, cx: &mut PanelCx<'_, DesktopSpec>) {
        let now = cx.time.now.0;
        let coarse = cx.view.coarse_pointer();
        let chart = self.chart_mut();
        chart.pump(now);
        chart.coarse = coarse;
        let id = WidgetId::from(screen::CHART_SLOT);
        // The wheel over the list, read from the view before the tree is built (the
        // view reports the last frame's targets).
        let scroll0 = self.watch.scroll;
        self.watch.wheel(cx.view.raw_pointer(), cx.view.scroll());
        let status = self.status();
        let sheet = self.sheet();
        let tree = {
            let chart = self.chart();
            let src = chart.source();
            let header = screen::Header { symbol: src.symbol(), venue: src.venue(), status, live: chart.shown == Shown::Live };
            let (shown, selected) = self.watch.shown();
            let side = screen::Side { query: &self.watch.query, shown: &shown, all: &self.watch.all, selected, scroll: self.watch.scroll };
            screen::tree(&header, chart.tf, &side).css(format!("height: {}px", cx.rect.height))
        };
        // The chart paints itself in its slot and takes the slot's input there.
        let resp = kit::dom(cx.rect, &sheet, tree).slot_registry(&mut self.slots).build(cx);
        for (fid, value) in &resp.edited {
            if fid == screen::SEARCH_ID {
                self.watch.set_query(value);
            }
        }
        match resp.clicked.as_deref() {
            Some(screen::RESET_ID) => self.chart_mut().model.reset(),
            Some(hit) => {
                if let Some(tf) = screen::picked_tf(hit) {
                    self.chart_mut().set_tf(tf, now);
                } else if let Some(row) = screen::picked_row(hit) {
                    if let Some(i) = self.watch.shown().0.get(row).copied() {
                        self.open_symbol(i, now);
                    }
                }
            }
            None => {}
        }
        // The list's viewport is its scroll box, not its (taller) content.
        self.watch.list_rect = resp.dom.rect(screen::LIST_BOX);
        self.last_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        if let Some(r) = self.chart_mut().last_rect {
            // uzor 1.5.36 registers a DOM paint slot for clicks, hover and fingers only;
            // the desktop chart also takes the mouse drag and the wheel (uzor branch
            // fix/paint-slot-mouse puts both into the slot frame).
            cx.widgets.coord.register(id, r, Sense::CLICK_AND_DRAG.union(Sense::SCROLL).with_touch().with_gesture());
        }
        // The tree of this frame was built before the clicks, edits and the wheel
        // changed the state: show the change on the next one.
        let changed = resp.clicked.is_some() || !resp.edited.is_empty() || self.watch.scroll != scroll0;
        if changed || self.chart_mut().wants_frame() {
            cx.out.request_frame();
        }
    }
}

impl App for DesktopApp {
    type Spec = DesktopSpec;

    fn init(&mut self, cx: &mut InitCx<'_, DesktopSpec>) {
        let h = cx.handle();
        let win = self.waker_win.clone();
        self.chart_mut().set_waker(Arc::new(move || {
            if let Some(w) = *win.lock().unwrap_or_else(|e| e.into_inner()) {
                let _ = h.request_repaint(w);
            }
        }));
    }

    fn chrome_model(&self, _win: WindowId) -> ChromeModel {
        ChromeModel::default()
    }

    fn overlay_model(&self, _win: WindowId, id: DesktopOverlay) -> OverlayModel {
        match id {}
    }

    fn panel(&mut self, cx: &mut PanelCx<'_, DesktopSpec>) {
        self.frame(cx);
    }

    fn overlay_body(&mut self, cx: &mut OverlayCx<'_, DesktopSpec>) {
        match cx.id {}
    }

    fn intent(&mut self, intent: Intent<DesktopSpec>, cx: &mut IntentCx<'_, DesktopSpec>) {
        if let Intent::Window(WindowIntent::Opened { win, .. }) = intent {
            self.win = Some(win);
            *self.waker_win.lock().unwrap_or_else(|e| e.into_inner()) = Some(win);
            cx.command(AppCommand::Layout(LayoutCmd::OpenPanel { win, panel: MainPanel, at: DockTarget::Root }));
        }
    }

    #[cfg(feature = "dev-control")]
    fn automation_state(&self) -> Option<String> {
        let src = self.chart().source();
        Some(
            serde_json::json!({
                "symbol": src.symbol(),
                "venue": src.venue(),
                "tf": mlc_chart_view::demo::TIMEFRAMES[self.chart().tf].0,
                "shown": format!("{:?}", self.chart().shown),
                "bars": self.chart().model.bars.len(),
                "count": self.chart().model.count,
                "right": self.chart().model.right,
                "crosshair": self.chart().model.crosshair,
                "query": self.watch.query,
                "watchlist": self.watch.shown().0.len(),
            })
            .to_string(),
        )
    }
}
