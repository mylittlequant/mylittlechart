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
    chrome_sheet: Option<(u64, String, uzor::ui::style::Sheet)>,
    /// The chrome strip is drawn as uzor DOM ([`dom::chrome`]) where it covers the state.
    pub chrome_on_dom: bool,
    /// The toolbars are drawn as uzor DOM ([`dom::toolbar`]) where they can be.
    pub toolbars_on_dom: bool,
    toolbar_sheets: Vec<(&'static str, Option<(u64, String, uzor::ui::style::Sheet)>)>,
    /// The DOM tree's hit rects of the last frame, by id (tests click them).
    pub last_hits: Vec<(String, uzor::types::Rect)>,
    /// The toolbars painted on DOM in the last frame, by slot ("top", "left", "right").
    pub toolbar_slots: Vec<&'static str>,
    /// The DOM chrome strip's hit rects of the last frame (empty while the app paints it).
    pub chrome_hits: Vec<(String, uzor::types::Rect)>,
    chart_settings_shown: bool,
    /// The pointer stream the app needs beyond presses (see [`Self::poll_pointer`]).
    pointer: PointerPoll,
}

/// What the framework (1.5.36) does not report to an app as unhandled input: pointer
/// moves, releases and leaves (only presses come as `UnhandledInput::Pointer`). The
/// host reads them off each frame's input view instead. Goes when the framework's
/// opt-in `WindowCmd::UnhandledPointer { all: true }` (uzor `feat/app-window-cmds`)
/// is released.
#[derive(Default)]
struct PointerPoll {
    /// Last position seen, window coordinates.
    last: Option<(f64, f64)>,
    /// The button of a press mlc took, until its release is seen.
    held: Option<mlc_app_input_shell::MouseBtn>,
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
            chrome_sheet: None,
            chrome_on_dom: false,
            toolbars_on_dom: false,
            toolbar_sheets: Vec::new(),
            last_hits: Vec::new(),
            chrome_hits: Vec::new(),
            toolbar_slots: Vec::new(),
            chart_settings_shown: false,
            pointer: PointerPoll::default(),
        }
    }

    /// Host `chart` with the pieces already ported to uzor DOM taken over from it.
    pub fn with_dom(mut chart: mlc_app::ChartApp) -> Self {
        chart.panel_app.dom_hosted.preset_name = true;
        chart.panel_app.dom_hosted.chart_settings = true;
        let mut host = Self::new(chart);
        host.chrome_on_dom = true;
        host.toolbars_on_dom = true;
        host
    }

    fn sheet(&mut self, css: String) -> uzor::ui::style::Sheet {
        cached_sheet(&mut self.sheet, css)
    }

    /// The toolbars the app laid out but left for the host to paint, on DOM.
    fn toolbar_layer(&mut self, cx: &mut PanelCx<'_, HostSpec>) {
        use mlc_app_layout::ui::toolbar_core::{capture, render_icon, IconId};
        let taken = capture::take();
        self.toolbar_slots.clear();
        let mut edges = Vec::new();
        for t in &taken {
            let r = cx.rect;
            let o = uzor::types::Rect::new(r.x + t.rect.x, r.y + t.rect.y + chrome::CHROME_HEIGHT, t.rect.width, t.rect.height);
            let css = dom::toolbar::css(t);
            let slot = match self.toolbar_sheets.iter().position(|(k, _)| *k == t.slot) {
                Some(i) => i,
                None => {
                    self.toolbar_sheets.push((t.slot, None));
                    self.toolbar_sheets.len() - 1
                }
            };
            let sheet = cached_sheet(&mut self.toolbar_sheets[slot].1, css);
            let (tree, own) = {
                let render = &mut *cx.render;
                let mut measure = |s: &str| {
                    render.set_font("13px sans-serif");
                    render.measure_text(s)
                };
                dom::toolbar::tree(t, o, &mut measure)
            };
            let anim = t.config.mascot_anim;
            let mut painter = |ctx: &mut dyn uzor::render::RenderContext, f: &uzor_framework::widgets::kit::SlotFrame<'_>| {
                if let Some(rest) = f.key.strip_prefix(dom::toolbar::BLUR) {
                    // The bar always (as `draw_toolbar_with_icons` began); a lit item
                    // only on a context that blurs.
                    // In the app's own coordinate space, as it made the call: the
                    // blur reads the pixels under the device rect it resolves.
                    if rest.ends_with(":bar") || ctx.has_blur_background() {
                        let (dx, dy) = (r.x, r.y + chrome::CHROME_HEIGHT);
                        ctx.save();
                        ctx.translate(dx, dy);
                        ctx.draw_blur_background(f.rect.x - dx, f.rect.y - dy, f.rect.width, f.rect.height);
                        ctx.restore();
                    }
                } else if let Some((_, icon, color)) = own.iter().find(|(k, _, _)| k == f.key) {
                    let ir = f.rect;
                    render_icon(ctx, &IconId::new(icon), mlc_app_layout::ui::toolbar_core::WidgetRect::new(ir.x, ir.y, ir.width, ir.height), color, anim);
                }
            };
            uzor_framework::widgets::kit::dom(o, &sheet, tree).paint_slots(&mut painter).build(cx);
            self.toolbar_slots.push(t.slot);
            if let Some(e) = dom::toolbar::edge(t, o) {
                edges.push((e, sheet));
            }
        }
        for (e, sheet) in edges {
            uzor_framework::widgets::kit::dom(e, &sheet, dom::toolbar::edge_tree(e)).build(cx);
        }
    }

    /// The chrome strip on DOM, when it covers the state; `false` = the app paints it.
    fn chrome_layer(&mut self, cx: &mut PanelCx<'_, HostSpec>) -> bool {
        let r = cx.rect;
        let skeleton = self.chart.panel_app.user_settings_state.profile_manager_mandatory;
        if !(self.chrome_on_dom && dom::chrome::covers(&self.chrome, r.width, skeleton)) {
            return false;
        }
        let sheet = cached_sheet(&mut self.chrome_sheet, dom::chrome::css(&self.chrome));
        cx.render.set_font("12px sans-serif");
        let text_w: Vec<f64> = self.chrome.tabs.iter().map(|t| cx.render.measure_text(&t.name)).collect();
        let tree = dom::chrome::tree(&self.chrome, r.width, &text_w);
        let strip = uzor::types::Rect::new(r.x, r.y, r.width, chrome::CHROME_HEIGHT);
        let resp = uzor_framework::widgets::kit::dom(strip, &sheet, tree).build(cx);
        self.chrome_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        if let Some(hit) = resp.clicked.as_deref().and_then(dom::chrome::hit_of) {
            let (x, y) = self.pointer.last.unwrap_or(self.input.last_mouse_pos);
            mlc_app_input_shell::press_chrome_hit(hit, x, y, &mut self.input, &mut self.chrome, &mut self.dirty, &mut self.chart, &self.platform, &mut self.out);
            for c in self.platform_cmds() {
                cx.out.command(c);
            }
        }
        true
    }

    fn preset_name_open(&self) -> bool {
        let p = &self.chart.panel_app;
        p.dom_hosted.preset_name && p.preset_name_input.is_open && p.preset_name_input.mode != mlc_app_layout::ui::modal_settings::PresetNameInputMode::CreateIndicatorSet
    }

    /// Whether the DOM chart settings modal is up (`Some` with the app's data).
    fn chart_settings_dom(&self) -> Option<mlc_app_layout::layout::modals::chart_settings::ChartSettingsData> {
        let p = &self.chart.panel_app;
        if !(p.dom_hosted.chart_settings && p.chart_settings_state.is_open) {
            return None;
        }
        let data = self.chart.build_chart_settings_data();
        p.chart_settings_on_dom(&data).then_some(data)
    }

    /// Whether any DOM piece is up (it takes the wheel instead of the chart).
    pub fn dom_modal_open(&self) -> bool {
        self.preset_name.is_some() || self.chart_settings_shown
    }

    /// The chart settings modal on DOM.
    fn chart_settings_layer(&mut self, cx: &mut PanelCx<'_, HostSpec>, data: &mlc_app_layout::layout::modals::chart_settings::ChartSettingsData) {
        use dom::chart_settings as cs;
        let look = cs::Look::of(&self.chart);
        let sheet = self.sheet(look.css());
        let st = &self.chart.panel_app.chart_settings_state;
        let at = cs::frame(cx.rect, chrome::CHROME_HEIGHT, self.chart.chart_content_origin(), st);
        let scroll = st.scroll.animated_offset();
        let tree = cs::tree(st, &data.instrument, at, scroll);
        let mut painter = |ctx: &mut dyn uzor::render::RenderContext, f: &uzor_framework::widgets::kit::SlotFrame<'_>| {
            if f.key == cs::SCROLLBAR {
                use mlc_app_layout::ui::scroll_widget::{draw_scrollbar, ScrollbarConfig, ScrollbarState};
                let r = f.rect;
                let track = uzor::types::Rect::new(r.x, r.y + 4.0, r.width, r.height - 8.0);
                let mut c = ctx::ChartCtx::new(ctx);
                let theme = mlc_app_layout::ui::widgets::WidgetTheme::default();
                draw_scrollbar(&mut c, &ScrollbarConfig::new(cs::content_height(), cs::VIEW_H, scroll), ScrollbarState::Active, track, &theme, None);
            }
        };
        let resp = uzor_framework::widgets::kit::dom(cx.rect, &sheet, tree).paint_slots(&mut painter).build(cx);
        self.last_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        if let Some(r) = resp.dom.rect(cs::VIEWPORT) {
            self.last_hits.push((cs::VIEWPORT.to_string(), r));
        }
        if let Some(id) = resp.clicked.as_deref().and_then(cs::app_id) {
            self.chart.chart_settings_click(&id);
        }
    }

    /// The DOM pieces over the app's own frame.
    fn dom_layer(&mut self, cx: &mut PanelCx<'_, HostSpec>) {
        use dom::preset_name as pn;
        self.chart_settings_shown = false;
        if !self.preset_name_open() {
            self.preset_name = None;
            self.last_hits.clear();
            if let Some(data) = self.chart_settings_dom() {
                self.chart_settings_shown = true;
                self.chart_settings_layer(cx, &data);
            }
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

    /// Moves, releases and leaves from this frame's input view, as `ShellEvent`s
    /// (window coordinates: the panel fills the window).
    fn poll_pointer(&mut self, view: &uzor_framework::VisualView<HostSpec>) {
        use mlc_app_input_shell::ShellEvent;
        let now = view.raw_pointer();
        if now != self.pointer.last {
            match now {
                Some((x, y)) => self.dispatch(ShellEvent::CursorMoved { x, y }),
                None => self.dispatch(ShellEvent::CursorLeft),
            }
            self.pointer.last = now;
        }
        if !view.button_down() {
            if let Some(button) = self.pointer.held.take() {
                let (x, y) = self.pointer.last.unwrap_or(self.input.last_mouse_pos);
                self.dispatch(ShellEvent::MouseInput { button, pressed: false, x, y });
            }
        }
    }

    /// A wheel step while a DOM piece is up: over the chart settings body it
    /// scrolls it; anywhere else it is swallowed (the modal is on top).
    fn dom_wheel(&mut self, u: &uzor_framework::UnhandledInput, dy: f64) {
        let uzor_framework::UnhandledInput::Wheel { wheel, .. } = u else { return };
        let over = self.last_hits.iter().any(|(k, r)| k == dom::chart_settings::VIEWPORT && r.contains(wheel.pos.x, wheel.pos.y));
        if self.chart_settings_shown && over {
            self.chart.chart_settings_wheel(dy, dom::chart_settings::content_height(), dom::chart_settings::VIEW_H);
        }
    }

    fn flush_platform(&mut self, cx: &mut IntentCx<'_, HostSpec>) {
        for c in self.platform_cmds() {
            cx.command(c);
        }
    }

    /// The window operations the dispatcher asked for since the last call, as
    /// framework commands.
    fn platform_cmds(&mut self) -> Vec<AppCommand<HostSpec>> {
        let Some(win) = self.win else { return Vec::new() };
        let mut out: Vec<AppCommand<HostSpec>> = self
            .platform
            .cmds
            .borrow_mut()
            .drain(..)
            .map(|c| match c {
                PlatformCmd::Minimize(m) => AppCommand::Window(WindowCmd::Minimize { win, minimized: m }),
                PlatformCmd::Maximize(m) => AppCommand::Window(WindowCmd::Maximize { win, maximized: m }),
                PlatformCmd::Clipboard(t) => AppCommand::Clipboard(ClipboardCmd::Write(t)),
            })
            .collect();
        if std::mem::take(&mut self.out.close_requested) || std::mem::take(&mut self.out.close_window_requested) {
            out.push(AppCommand::Window(WindowCmd::Close(win)));
        }
        out
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
        self.poll_pointer(cx.view);
        let now = self.now_ms();
        frame::pump(&mut self.chart, now, r.width, (r.height - chrome::CHROME_HEIGHT).max(1.0));
        let wall = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        cx.render.save();
        cx.render.translate(r.x, r.y);
        let skeleton = self.chart.panel_app.user_settings_state.profile_manager_mandatory;
        mlc_app_layout::ui::toolbar_core::capture::set_enabled(self.toolbars_on_dom);
        {
            // The strip is painted below, after the toolbars, as the desktop shell
            // paints it after the whole app frame.
            let mut ctx = ctx::ChartCtx::new(&mut *cx.render);
            frame::paint_under(&mut self.chart, &mut self.chrome, &mut ctx, r.width, r.height, wall, frame::Layers::All, false);
        }
        cx.render.restore();
        mlc_app_layout::ui::toolbar_core::capture::set_enabled(false);
        self.toolbar_layer(cx);
        self.chrome_hits.clear();
        if !self.chrome_layer(cx) {
            // A state the DOM strip does not cover: the app paints it.
            cx.render.save();
            cx.render.translate(r.x, r.y);
            cx.render.set_fill_color(&self.chrome.colors.background);
            cx.render.fill_rect(0.0, 0.0, r.width, chrome::CHROME_HEIGHT);
            let mut ctx = ctx::ChartCtx::new(&mut *cx.render);
            chrome::render(&mut ctx, &self.chrome, r.width, skeleton, &self.chart.input_coordinator);
            drop(ctx);
            cx.render.restore();
        }
        cx.render.save();
        cx.render.translate(r.x, r.y);
        {
            let mut ctx = ctx::ChartCtx::new(&mut *cx.render);
            frame::paint_over(&mut self.chart, &self.chrome, &mut ctx, r.width, wall);
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
                    match ev {
                        mlc_app_input_shell::ShellEvent::MouseInput { button, pressed: true, x, y } => {
                            self.pointer.held = Some(button);
                            self.pointer.last = Some((x, y));
                        }
                        mlc_app_input_shell::ShellEvent::MouseInput { pressed: false, .. } => self.pointer.held = None,
                        _ => {}
                    }
                    if let mlc_app_input_shell::ShellEvent::Scroll { dy, .. } = ev {
                        if self.dom_modal_open() {
                            self.dom_wheel(&u, dy);
                            continue;
                        }
                    }
                    self.dispatch(ev);
                }
                self.flush_platform(cx);
            }
            _ => {}
        }
    }
}

/// `slot`'s sheet for `css`, parsed again only when the CSS or the media generation
/// changed.
fn cached_sheet(slot: &mut Option<(u64, String, uzor::ui::style::Sheet)>, css: String) -> uzor::ui::style::Sheet {
    let gen = uzor::ui::media::generation();
    if slot.as_ref().is_none_or(|(g, c, _)| *g != gen || *c != css) {
        let s = uzor::ui::domkit::sheet_with(&css).expect("dom css parses");
        *slot = Some((gen, css, s));
    }
    slot.as_ref().expect("sheet").2.clone()
}
