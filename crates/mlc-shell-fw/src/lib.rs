//! The existing mylittlechart app hosted in a uzor-framework [`App`].
//!
//! Step 1 of the in-place migration (no rewrite): uzor-framework owns the window, the
//! event loop and the render backend; `ChartApp` is the same object the desktop shell
//! runs. Input the framework does not take itself (all of it: the panel registers no
//! widgets yet) reaches [`App::intent`] as `Intent::Unhandled`, is turned into mlc's
//! platform-neutral `ShellEvent`s and goes through the same
//! `mlc_app_input_shell::process_event` the winit shell calls. Each frame pumps the
//! app's clock and paints it through [`ctx::ChartCtx`] onto the panel's paint target
//! with the shell's own paint order ([`frame::paint_window`]).
//!
//! The next steps replace one hand-made piece at a time (a modal, a toolbar) with
//! uzor widgets / DOM inside this same app.

pub mod ctx;
pub mod dom;
pub mod frame;
mod input;

#[cfg(test)]
mod tests;

use std::cell::RefCell;

use mlc_app_input_shell::{chrome, DirtyFlags, DispatchOut, InputState, TooltipState};
use uzor::layout::docking::DockPanel;
use uzor::render::TickRate;
use uzor::tokens::{BuiltinSet, Tokens};
use uzor_framework::{
    App, AppCommand, ChromeModel, ClipboardCmd, DockTarget, Intent, IntentCx, LayoutCmd, OverlayCx, OverlayModel, PanelCx, PanelHome,
    RuntimeConfig, SizePx, Spec, WindowCmd, WindowId, WindowIntent, WindowSpec,
};

pub use input::shell_event;

/// The one panel: the whole window is the existing app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root;
impl DockPanel for Root {
    fn title(&self) -> &str {
        "mylittlechart"
    }
    fn type_id(&self) -> &'static str {
        "mlc"
    }
    fn closable(&self) -> bool {
        false
    }
    fn shows_header(&self) -> bool {
        false
    }
}

/// No framework overlays yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ov {}
/// No framework actions yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Act {}

/// The app vocabulary.
pub struct HostSpec;
impl Spec for HostSpec {
    type Panel = Root;
    type Overlay = Ov;
    type Action = Act;
    fn decode_panel(_h: PanelHome, _t: &str) -> Option<Root> {
        Some(Root)
    }
}

/// A window operation the dispatcher asked for, run as a framework command.
#[derive(Clone, Debug, PartialEq)]
pub enum PlatformCmd {
    /// Minimise or restore.
    Minimize(bool),
    /// Maximise or restore.
    Maximize(bool),
    /// Write text to the clipboard.
    Clipboard(String),
}

/// mlc's `Platform` over the framework: window operations become commands; what the
/// framework has no app-side command for yet (cursor shape, window drag from app
/// chrome, clipboard read) is recorded in `unsupported` (uzor gap G15).
#[derive(Default)]
pub struct FwPlatform {
    size: std::cell::Cell<(u32, u32)>,
    maximized: std::cell::Cell<bool>,
    cmds: RefCell<Vec<PlatformCmd>>,
    /// Calls the framework cannot carry out yet, by name (for tests and logs).
    pub unsupported: RefCell<Vec<&'static str>>,
}

impl mlc_app_input_shell::Platform for FwPlatform {
    fn set_cursor(&self, _style: mlc_core::CursorStyle) {
        self.unsupported.borrow_mut().push("set_cursor");
    }
    fn set_cursor_visible(&self, _visible: bool) {
        self.unsupported.borrow_mut().push("set_cursor_visible");
    }
    fn drag_window(&self) {
        self.unsupported.borrow_mut().push("drag_window");
    }
    fn drag_resize_window(&self, _dir: mlc_app_input_shell::ResizeDir) {
        self.unsupported.borrow_mut().push("drag_resize_window");
    }
    fn set_minimized(&self, v: bool) {
        self.cmds.borrow_mut().push(PlatformCmd::Minimize(v));
    }
    fn set_maximized(&self, v: bool) {
        self.maximized.set(v);
        self.cmds.borrow_mut().push(PlatformCmd::Maximize(v));
    }
    fn is_maximized(&self) -> bool {
        self.maximized.get()
    }
    fn window_size(&self) -> (u32, u32) {
        self.size.get()
    }
    fn clipboard_write(&self, text: &str) {
        self.cmds.borrow_mut().push(PlatformCmd::Clipboard(text.to_string()));
    }
    fn clipboard_read(&self) -> Option<String> {
        self.unsupported.borrow_mut().push("clipboard_read");
        None
    }
    fn request_redraw(&self) {}
}

/// The existing app in a uzor-framework window.
pub struct HostedApp {
    /// The app itself, as the desktop shell runs it.
    pub chart: mlc_app::ChartApp,
    /// The window chrome strip's state.
    pub chrome: chrome::ChromeState,
    input: InputState,
    dirty: DirtyFlags,
    tooltip: TooltipState,
    screenshot_pending: bool,
    out: DispatchOut,
    /// The platform face the dispatcher talks to.
    pub platform: FwPlatform,
    started: web_time::Instant,
    size: (u32, u32),
    win: Option<WindowId>,
    /// Frames painted (tests wait on it).
    pub frames: u64,
    /// The preset-name dialog's field while it is open.
    pub preset_name: Option<String>,
    sheet: Option<(u64, String, uzor::ui::style::Sheet)>,
    /// The DOM tree's hit rects of the last frame, by id (tests click them).
    pub last_hits: Vec<(String, uzor::types::Rect)>,
}

impl HostedApp {
    /// Host `chart`.
    pub fn new(chart: mlc_app::ChartApp) -> Self {
        HostedApp {
            chart,
            chrome: chrome::ChromeState::new("chart-app-vello"),
            input: InputState::default(),
            dirty: DirtyFlags::default(),
            tooltip: TooltipState::with_delay(700.0),
            screenshot_pending: false,
            out: DispatchOut::default(),
            platform: FwPlatform::default(),
            started: web_time::Instant::now(),
            size: (0, 0),
            win: None,
            frames: 0,
            preset_name: None,
            sheet: None,
            last_hits: Vec::new(),
        }
    }

    /// Host `chart` with the pieces already ported to uzor DOM taken over from it.
    pub fn with_dom(mut chart: mlc_app::ChartApp) -> Self {
        chart.panel_app.dom_hosted.preset_name = true;
        Self::new(chart)
    }

    fn sheet(&mut self, css: String) -> uzor::ui::style::Sheet {
        let gen = uzor::ui::media::generation();
        if self.sheet.as_ref().is_none_or(|(g, c, _)| *g != gen || *c != css) {
            let s = uzor::ui::domkit::sheet_with(&css).expect("dom css parses");
            self.sheet = Some((gen, css, s));
        }
        self.sheet.as_ref().expect("sheet").2.clone()
    }

    fn preset_name_open(&self) -> bool {
        let p = &self.chart.panel_app;
        p.dom_hosted.preset_name && p.preset_name_input.is_open && p.preset_name_input.mode != mlc_app_layout::ui::modal_settings::PresetNameInputMode::CreateIndicatorSet
    }

    /// The DOM pieces over the app's own frame.
    fn dom_layer(&mut self, cx: &mut PanelCx<'_, HostSpec>) {
        use dom::preset_name as pn;
        if !self.preset_name_open() {
            self.preset_name = None;
            self.last_hits.clear();
            return;
        }
        if self.preset_name.is_none() {
            self.preset_name = Some(self.chart.preset_name_text());
            if let Some(win) = self.win {
                cx.out.command(AppCommand::Focus(uzor_framework::FocusCmd::Focus { win, id: uzor::WidgetId::from(pn::FIELD) }));
            }
        }
        let look = pn::Look::of(&self.chart);
        let sheet = self.sheet(look.css());
        let st = &self.chart.panel_app.preset_name_input;
        let at = pn::frame(cx.rect, chrome::CHROME_HEIGHT, st.position);
        let name = self.preset_name.clone().unwrap_or_default();
        let tree = pn::tree(st.mode, &name, at);
        let resp = uzor_framework::widgets::kit::dom(cx.rect, &sheet, tree).build(cx);
        let mut name = name;
        for (id, v) in &resp.edited {
            if id == pn::FIELD {
                name = v.clone();
            }
        }
        self.preset_name = Some(name.clone());
        self.last_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        match pn::outcome(resp.clicked.as_deref(), &name) {
            Some(pn::Outcome::Commit(n)) => self.chart.commit_preset_name(&n),
            Some(pn::Outcome::Close) => self.chart.close_preset_name_input(),
            None => {}
        }
    }

    /// One frameless window of `w`×`h` (the app draws its own chrome strip).
    pub fn runtime_config(w: u32, h: u32) -> RuntimeConfig {
        let mut spec = WindowSpec::new("main", "mylittlechart".to_string(), SizePx::new(w, h));
        spec.decorations = false;
        RuntimeConfig { windows: vec![spec], tokens: Tokens::builtin(BuiltinSet::Dark), dark: true, tick: TickRate::Dirty, ..RuntimeConfig::default() }
    }

    /// Feed one platform-neutral event through mlc's dispatcher.
    pub fn dispatch(&mut self, ev: mlc_app_input_shell::ShellEvent) {
        mlc_app_input_shell::process_event(
            &mut self.input,
            &mut self.chrome,
            &mut self.dirty,
            &mut self.chart,
            &self.platform,
            ev,
            &mut self.tooltip,
            &mut self.screenshot_pending,
            &mut self.out,
        );
    }

    /// The app clock in ms since start (what `tick` takes).
    pub fn now_ms(&self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn flush_platform(&mut self, cx: &mut IntentCx<'_, HostSpec>) {
        let Some(win) = self.win else { return };
        for c in self.platform.cmds.borrow_mut().drain(..) {
            match c {
                PlatformCmd::Minimize(m) => cx.command(AppCommand::Window(WindowCmd::Minimize { win, minimized: m })),
                PlatformCmd::Maximize(m) => cx.command(AppCommand::Window(WindowCmd::Maximize { win, maximized: m })),
                PlatformCmd::Clipboard(t) => cx.command(AppCommand::Clipboard(ClipboardCmd::Write(t))),
            }
        }
        if std::mem::take(&mut self.out.close_requested) || std::mem::take(&mut self.out.close_window_requested) {
            cx.command(AppCommand::Window(WindowCmd::Close(win)));
        }
    }
}

impl App for HostedApp {
    type Spec = HostSpec;

    fn chrome_model(&self, _w: WindowId) -> ChromeModel {
        ChromeModel::default()
    }
    fn overlay_model(&self, _w: WindowId, id: Ov) -> OverlayModel {
        match id {}
    }
    fn overlay_body(&mut self, cx: &mut OverlayCx<'_, HostSpec>) {
        match cx.id {}
    }

    fn panel(&mut self, cx: &mut PanelCx<'_, HostSpec>) {
        let r = cx.rect;
        let size = (r.width.round().max(1.0) as u32, r.height.round().max(1.0) as u32);
        if size != self.size {
            self.size = size;
            self.platform.size.set(size);
            self.dispatch(mlc_app_input_shell::ShellEvent::Resized { w: size.0, h: size.1 });
        }
        let now = self.now_ms();
        frame::pump(&mut self.chart, now, r.width, (r.height - chrome::CHROME_HEIGHT).max(1.0));
        let wall = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        cx.render.save();
        cx.render.translate(r.x, r.y);
        {
            let mut ctx = ctx::ChartCtx::new(&mut *cx.render);
            frame::paint_window(&mut self.chart, &mut self.chrome, &mut ctx, r.width, r.height, wall, frame::Layers::All);
        }
        cx.render.restore();
        self.dom_layer(cx);
        self.frames += 1;
        // The app animates (live bars, spinners, the clock): it runs every frame, as the
        // winit shell's redraw loop does.
        cx.out.request_frame();
    }

    fn intent(&mut self, intent: Intent<HostSpec>, cx: &mut IntentCx<'_, HostSpec>) {
        match intent {
            Intent::Window(WindowIntent::Opened { win, .. }) => {
                self.win = Some(win);
                cx.command(AppCommand::Layout(LayoutCmd::SetChrome { win, visible: false, height: 0.0 }));
                cx.command(AppCommand::Layout(LayoutCmd::OpenPanel { win, panel: Root, at: DockTarget::Root }));
            }
            Intent::Text(uzor_framework::TextIntent::Submitted { field, .. }) if field.as_str() == dom::preset_name::FIELD => {
                if self.preset_name_open() {
                    let name = self.preset_name.clone().unwrap_or_default();
                    self.chart.commit_preset_name(&name);
                }
            }
            Intent::Unhandled(u) => {
                let mods = self.input.modifiers;
                for ev in input::shell_events(&u, mods) {
                    self.dispatch(ev);
                }
                self.flush_platform(cx);
            }
            _ => {}
        }
    }
}
