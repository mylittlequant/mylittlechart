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
    /// The app's cursor over content (`None` = the widget under the pointer decides).
    Cursor(Option<uzor::CursorIcon>),
    /// Show or hide the cursor.
    CursorVisible(bool),
    /// Start an OS window move (a press on the app's own caption or tab strip).
    DragMove,
    /// Start an OS window resize from the app's own edge.
    DragResize(uzor::ResizeDirection),
}

/// mlc's cursor as the framework's (`Default` hands the cursor back).
fn cursor_icon(c: mlc_core::CursorStyle) -> Option<uzor::CursorIcon> {
    use mlc_core::CursorStyle as S;
    use uzor::CursorIcon as I;
    Some(match c {
        S::Default => return None,
        S::Pointer => I::PointingHand,
        S::Grab => I::Grab,
        S::Grabbing => I::Grabbing,
        S::Move => I::Move,
        S::NsResize => I::ResizeVertical,
        S::EwResize => I::ResizeHorizontal,
        S::NeswResize => I::ResizeNeSw,
        S::NwseResize => I::ResizeNwSe,
        S::Crosshair => I::Crosshair,
        S::NotAllowed => I::NotAllowed,
        S::None => I::None,
    })
}

fn resize_dir(d: mlc_app_input_shell::ResizeDir) -> uzor::ResizeDirection {
    use mlc_app_input_shell::ResizeDir as D;
    use uzor::ResizeDirection as R;
    match d {
        D::North => R::North,
        D::South => R::South,
        D::East => R::East,
        D::West => R::West,
        D::NorthEast => R::NorthEast,
        D::NorthWest => R::NorthWest,
        D::SouthEast => R::SouthEast,
        D::SouthWest => R::SouthWest,
    }
}

/// mlc's `Platform` over the framework: window operations, the cursor and window
/// drags from the app's own chrome (uzor 1.5.37 `WindowCmd::{SetCursor,
/// SetCursorVisible, DragMove, DragResize}`) become commands; what has no
/// cross-platform app-side command (a synchronous clipboard read) is recorded in
/// `unsupported`.
#[derive(Default)]
pub struct FwPlatform {
    size: std::cell::Cell<(u32, u32)>,
    maximized: std::cell::Cell<bool>,
    cmds: RefCell<Vec<PlatformCmd>>,
    /// The cursor last asked for (repeats are not sent again).
    cursor: std::cell::Cell<Option<Option<uzor::CursorIcon>>>,
    /// Calls the framework cannot carry out yet, by name (for tests and logs).
    pub unsupported: RefCell<Vec<&'static str>>,
}

impl mlc_app_input_shell::Platform for FwPlatform {
    fn set_cursor(&self, style: mlc_core::CursorStyle) {
        let icon = cursor_icon(style);
        if self.cursor.replace(Some(icon)) != Some(icon) {
            self.cmds.borrow_mut().push(PlatformCmd::Cursor(icon));
        }
    }
    fn set_cursor_visible(&self, visible: bool) {
        self.cmds.borrow_mut().push(PlatformCmd::CursorVisible(visible));
    }
    fn drag_window(&self) {
        self.cmds.borrow_mut().push(PlatformCmd::DragMove);
    }
    fn drag_resize_window(&self, dir: mlc_app_input_shell::ResizeDir) {
        self.cmds.borrow_mut().push(PlatformCmd::DragResize(resize_dir(dir)));
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
    /// The toolbars painted on DOM in the last frame, by slot ("top", "left",
    /// "right", "bottom").
    pub toolbar_slots: Vec<&'static str>,
    /// The DOM toolbars' item hit rects of the last frame, by DOM id.
    pub toolbar_hits: Vec<(String, uzor::types::Rect)>,
    /// The toolbar item the DOM reported under the pointer last frame (its DOM id).
    pub toolbar_hover: Option<String>,
    toolbar_press: PressForward,
    /// The DOM chrome strip's hit rects of the last frame (empty while the app paints it).
    pub chrome_hits: Vec<(String, uzor::types::Rect)>,
    /// The strip element the DOM reported under the pointer last frame.
    chrome_hover: Option<String>,
    chrome_tab_down: bool,
    chrome_tab_forwarded: bool,
    /// The right sidebar's DOM hit rects of the last frame (empty while the app paints it).
    pub sidebar_hits: Vec<(String, uzor::types::Rect)>,
    /// The sidebar element the DOM reported under the pointer last frame.
    pub sidebar_hover: Option<String>,
    sidebar_sheet: Option<(u64, String, uzor::ui::style::Sheet)>,
    sidebar_press: PressForward,
    sidebar_shown: bool,
    /// The panel scroll offset last copied between the DOM rows and the app.
    sidebar_scroll: Option<f64>,
    chart_settings_shown: bool,
    /// The chart settings body's DOM scroll was seeded from the app's offset.
    cs_scroll_seeded: bool,
    /// The pointer stream the app needs beyond presses (see [`Self::poll_pointer`]).
    pointer: PointerPoll,
}

/// The pointer stream mlc needs beyond presses: moves, releases and leaves anywhere
/// in the window, read off each frame's input view. uzor 1.5.37's opt-in
/// `WindowCmd::UnhandledPointer { all: true }` reports only events no widget took,
/// so moves over the DOM chrome cells (whose hover is the app's) and a release over
/// a DOM element after a press the app took would be missing; the poll stays.
#[derive(Default)]
struct PointerPoll {
    /// Last position seen, window coordinates.
    last: Option<(f64, f64)>,
    /// The button of a press mlc took, until its release is seen.
    held: Option<mlc_app_input_shell::MouseBtn>,
}

/// Raw press forwarding for DOM pieces whose clicks stay the app's: the DOM lays the
/// elements out and says which one the pointer is on; the press itself goes to the
/// app as the raw press it always got (its hit test sees the same boxes), and its
/// release (seen by [`PointerPoll`]) runs the click through the app's own path.
#[derive(Default)]
struct PressForward {
    /// The element held down whose press went to the app (DOM id).
    pressed: Option<String>,
    was_pressed: bool,
}

/// What [`PressForward::step`] wants sent to the app.
#[derive(Debug, PartialEq)]
enum Forward {
    /// The press (the release follows through the pointer poll).
    Press,
    /// A whole click the app never saw (down and up between two frames).
    Click,
}

impl PressForward {
    /// One frame: `pressed`, the DOM element held down now; `clicked`, the DOM's
    /// click this frame (which arrives a frame after the release).
    fn step(&mut self, pressed: Option<String>, clicked: Option<&str>) -> Vec<Forward> {
        let mut out = Vec::new();
        if pressed.is_some() && !self.was_pressed {
            out.push(Forward::Press);
            self.pressed = pressed.clone();
        }
        if let Some(id) = clicked {
            if self.pressed.take().as_deref() != Some(id) {
                out.push(Forward::Click);
            }
        }
        self.was_pressed = pressed.is_some();
        out
    }
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
            chrome_hover: None,
            chrome_tab_down: false,
            chrome_tab_forwarded: false,
            toolbar_slots: Vec::new(),
            toolbar_hits: Vec::new(),
            toolbar_hover: None,
            toolbar_press: PressForward::default(),
            sidebar_hits: Vec::new(),
            sidebar_hover: None,
            sidebar_sheet: None,
            sidebar_press: PressForward::default(),
            sidebar_shown: false,
            sidebar_scroll: None,
            chart_settings_shown: false,
            cs_scroll_seeded: false,
            pointer: PointerPoll::default(),
        }
    }

    /// Host `chart` with the pieces already ported to uzor DOM taken over from it.
    pub fn with_dom(mut chart: mlc_app::ChartApp) -> Self {
        chart.panel_app.dom_hosted.preset_name = true;
        chart.panel_app.dom_hosted.chart_settings = true;
        chart.panel_app.dom_hosted.sidebar = true;
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
        self.toolbar_hits.clear();
        if taken.is_empty() {
            return;
        }
        let (mut hover, mut clicked, mut pressed) = (None, None, None);
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
                // The passes that only lay out and hit-test reuse the widths the
                // painting pass measured: the boxes they hit-test are the ones painted.
                let mut measure = |font: &str, s: &str| {
                    render.set_font(font);
                    let w = render.measure_text(s);
                    ctx::measured(font, s, w)
                };
                dom::toolbar::tree(t, o.width, o.height, self.toolbar_hover.as_deref(), &mut measure)
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
                } else if f.key.starts_with(dom::toolbar::CLOCK) {
                    // The app's own hover effect, in its coordinates (the pointer
                    // and the animation clock are the ones it laid the bar out with).
                    let c = &t.config;
                    let time = c.sections.iter().flat_map(|s| &s.items).find_map(|i| match i {
                        mlc_app_layout::ui::toolbar_core::ToolbarItem::Clock { time, .. } => Some(time.as_str()),
                        _ => None,
                    });
                    if let (Some(time), Some(pointer)) = (time, c.cursor) {
                        let (dx, dy) = (r.x, r.y + chrome::CHROME_HEIGHT);
                        let ir = f.rect;
                        ctx.save();
                        ctx.translate(dx, dy);
                        mlc_app_layout::ui::toolbar_core::draw_clock_glitch(
                            ctx,
                            time,
                            mlc_app_layout::ui::toolbar_core::WidgetRect::new(ir.x - dx, ir.y - dy, ir.width, ir.height),
                            &t.theme.item_text,
                            pointer,
                            c.anim_time_s,
                            c.anim_frame,
                        );
                        ctx.restore();
                    }
                } else if let Some((_, icon, color)) = own.iter().find(|(k, _, _)| k == f.key) {
                    let ir = f.rect;
                    render_icon(ctx, &IconId::new(icon), mlc_app_layout::ui::toolbar_core::WidgetRect::new(ir.x, ir.y, ir.width, ir.height), color, anim);
                }
            };
            let resp = uzor_framework::widgets::kit::dom(o, &sheet, tree).paint_slots(&mut painter).build(cx);
            hover = hover.or(resp.hovered.clone());
            clicked = clicked.or(resp.clicked);
            for (id, hr) in resp.dom.hits() {
                self.toolbar_hits.push((id.to_string(), hr));
                if cx.view.is_pressed(&uzor::WidgetId::from(id)) {
                    pressed = Some(id.to_string());
                }
            }
            self.toolbar_slots.push(t.slot);
            if let Some(e) = dom::toolbar::edge(t, o) {
                edges.push((e, sheet));
            }
        }
        for (e, sheet) in edges {
            uzor_framework::widgets::kit::dom(e, &sheet, dom::toolbar::edge_tree(e)).build(cx);
        }
        self.toolbar_hover = hover;
        // The DOM laid the items out and says which one the pointer is on; the
        // press itself goes to the app (see [`PressForward`]): it arms what it arms
        // on a press (a drawing group's drag onto the favourites arc), and its
        // release runs the click — the item's action, a double-click, a drag's end.
        let f = self.toolbar_press.step(pressed, clicked.as_deref());
        self.forward(&f);
    }

    /// Send `f` to the app at the pointer.
    fn forward(&mut self, f: &[Forward]) {
        use mlc_app_input_shell::{MouseBtn, ShellEvent};
        let (x, y) = self.pointer.last.unwrap_or(self.input.last_mouse_pos);
        for f in f {
            match f {
                Forward::Press => {
                    self.dispatch(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: true, x, y });
                    self.pointer.held = Some(MouseBtn::Left);
                }
                Forward::Click => {
                    self.dispatch(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: true, x, y });
                    self.dispatch(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: false, x, y });
                }
            }
        }
    }

    /// The right sidebar on DOM ([`dom::sidebar`]) when the app hands it over
    /// (`ChartApp::sidebar_on_dom`); else the app painted it.
    fn sidebar_layer(&mut self, cx: &mut PanelCx<'_, HostSpec>) {
        use dom::sidebar as sb;
        let was_shown = std::mem::replace(&mut self.sidebar_shown, false);
        self.sidebar_hits.clear();
        let (rect, vp, app_content_h) = match (self.chart.sidebar_on_dom(), self.chart.last_sidebar_result.as_ref()) {
            (true, Some(res)) => (res.sidebar_rect, res.viewport_rect, res.content_height),
            _ => {
                self.sidebar_hover = None;
                self.sidebar_scroll = None;
                self.sidebar_press = PressForward::default();
                return;
            }
        };
        if !was_shown {
            self.sidebar_scroll = None;
        }
        self.sidebar_shown = true;
        let r = cx.rect;
        let o = uzor::types::Rect::new(r.x + rect.x, r.y + rect.y + chrome::CHROME_HEIGHT, rect.width, rect.height);
        let theme = self.chart.panel_app.toolbar_theme_for_render();
        let data = self.chart.watchlist_dom_data();
        let sheet = cached_sheet(&mut self.sidebar_sheet, sb::css(&theme));
        // The rows scroll themselves; an offset the app set (its scrollbar drag, a
        // reveal) is pushed into them, else theirs is copied back to the app.
        let app = self.chart.sidebar_state.current_right_scroll();
        let (app_y, dragging) = (app.offset, app.is_dragging);
        let pushed = match self.sidebar_scroll {
            Some(last) if (last - app_y).abs() < 0.5 => None,
            _ => Some(app_y),
        };
        if let Some(y) = pushed {
            uzor_framework::widgets::kit::dom_scroll_to(cx, sb::VIEWPORT, y, false);
        }
        let scroll = pushed.unwrap_or_else(|| uzor_framework::widgets::kit::dom_scroll_top(cx, sb::VIEWPORT));
        let now_ms = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
        let w = sb::Sidebar {
            rect: o,
            theme: &theme,
            state: &self.chart.sidebar_state,
            columns: &data.columns,
            flags: &data.flags,
            hovered: self.sidebar_hover.as_deref(),
            viewport: uzor::types::Rect::new(r.x + vp.x, r.y + vp.y + chrome::CHROME_HEIGHT, vp.width, vp.height),
            app_content_h,
            now_ms,
            app_rect: uzor::types::Rect::new(rect.x, rect.y, rect.width, rect.height),
        };
        let tree = {
            let render = &mut *cx.render;
            let mut measure = |font: &str, s: &str| {
                render.set_font(font);
                let w = render.measure_text(s);
                ctx::measured(font, s, w)
            };
            sb::tree(&w, &mut measure)
        };
        let (content_h, view_h) = if self.chart.sidebar_state.right_panel == mlc_app_sidebar_content::state::RightSidebarPanel::Watchlist {
            (w.content_h(), w.view_h())
        } else {
            (app_content_h, vp.height)
        };
        let state = &self.chart.sidebar_state;
        let mut painter = |ctx: &mut dyn uzor::render::RenderContext, f: &uzor_framework::widgets::kit::SlotFrame<'_>| {
            use mlc_app_sidebar_content::render::draw_price_sparkline;
            let fr = f.rect;
            let spark = |ctx: &mut dyn uzor::render::RenderContext, data: &[f64], up: bool| {
                let color = if up { &theme.success } else { &theme.danger };
                let mut c = ctx::ChartCtx::new(ctx);
                // The slot box is grown by `SPARK_BLEED` for the line's caps.
                let b = sb::SPARK_BLEED;
                draw_price_sparkline(&mut c, fr.x + b, fr.y + b, fr.width - 2.0 * b, fr.height - 2.0 * b, data, color);
            };
            if f.key == sb::BLUR {
                // In the app's own coordinate space, as it made the call.
                let (dx, dy) = (r.x, r.y + chrome::CHROME_HEIGHT);
                ctx.save();
                ctx.translate(dx, dy);
                ctx.draw_blur_background(o.x - dx, o.y - dy, o.width, o.height);
                ctx.restore();
            } else if f.key == sb::TICKER_SPARK {
                if let Some(t) = &state.ticker_section {
                    spark(ctx, &t.spark, t.change_pct >= 0.0);
                }
            } else if let Some(i) = f.key.strip_prefix(sb::SPARK).and_then(|i| i.parse::<usize>().ok()) {
                if let Some(item) = state.watchlist_items.get(i) {
                    spark(ctx, &item.spark, item.change_percent >= 0.0);
                }
            } else if f.key == sb::SWATCH_X {
                // As the app strokes it, with the round caps its chart left set.
                ctx.save();
                ctx.set_stroke_color(&theme.item_text);
                ctx.set_stroke_width(1.5);
                ctx.set_line_cap("round");
                ctx.begin_path();
                ctx.move_to(fr.x + 4.0, fr.y + 4.0);
                ctx.line_to(fr.x + fr.width - 4.0, fr.y + fr.height - 4.0);
                ctx.move_to(fr.x + fr.width - 4.0, fr.y + 4.0);
                ctx.line_to(fr.x + 4.0, fr.y + fr.height - 4.0);
                ctx.stroke();
                ctx.restore();
            } else if f.key == sb::SCROLLBAR {
                use mlc_app_layout::ui::scroll_widget::{draw_scrollbar, ScrollbarConfig, ScrollbarState};
                let track = uzor::types::Rect::new(fr.x, fr.y + 4.0, fr.width, view_h.min(content_h) - 8.0);
                let mut c = ctx::ChartCtx::new(ctx);
                let wt = mlc_app_layout::ui::widgets::WidgetTheme::default();
                let st = if dragging { ScrollbarState::Dragging } else { ScrollbarState::Active };
                draw_scrollbar(&mut c, &ScrollbarConfig::new(content_h, view_h, scroll), st, track, &wt, None);
            }
        };
        let resp = uzor_framework::widgets::kit::dom(sb::frame(o), &sheet, tree).paint_slots(&mut painter).dom_scroll().build(cx);
        let y = match resp.scrolls.iter().find(|(k, _)| pushed.is_none() && k == sb::VIEWPORT) {
            Some((_, y)) => *y,
            None => scroll,
        };
        let s = self.chart.sidebar_state.current_right_scroll_mut();
        if (s.offset - y).abs() >= 0.01 && !s.is_dragging {
            s.offset = y;
            s.target = y;
        }
        self.sidebar_scroll = Some(self.chart.sidebar_state.current_right_scroll().offset);
        self.sidebar_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        let pressed = self.sidebar_hits.iter().find(|(id, _)| cx.view.is_pressed(&uzor::WidgetId::from(id.as_str()))).map(|(id, _)| id.clone());
        self.sidebar_hover = resp.hovered.clone();
        let f = self.sidebar_press.step(pressed, resp.clicked.as_deref());
        self.forward(&f);
    }

    /// The chrome strip on DOM, when it covers the state; `false` = the app paints it.
    fn chrome_layer(&mut self, cx: &mut PanelCx<'_, HostSpec>) -> bool {
        let r = cx.rect;
        let skeleton = self.chart.panel_app.user_settings_state.profile_manager_mandatory;
        if !(self.chrome_on_dom && dom::chrome::covers(&self.chrome, r.width, skeleton)) {
            // (The last DOM boxes stay: the next DOM frame resolves its hover on them.)
            self.chrome_tab_down = false;
            return false;
        }
        let sheet = cached_sheet(&mut self.chrome_sheet, dom::chrome::css(&self.chrome));
        cx.render.set_font("12px sans-serif");
        let text_w: Vec<f64> = self.chrome.tabs.iter().map(|t| ctx::measured("12px sans-serif", &t.name, cx.render.measure_text(&t.name))).collect();
        // Under the pointer now, against the boxes laid out last frame (the
        // innermost: a tab's × over the tab), as the app's own hit test resolves it.
        if let Some((px, py)) = cx.view.raw_pointer() {
            self.chrome_hover = self.chrome_hits.iter().rev().find(|(_, b)| px >= b.x && px < b.x + b.width && py >= b.y && py < b.y + b.height).map(|(k, _)| k.clone());
        }
        let hov = dom::chrome::hovered(self.chrome_hover.as_deref());
        let tree = dom::chrome::tree(&self.chrome, hov, r.width, &text_w);
        let strip = uzor::types::Rect::new(r.x, r.y, r.width, chrome::CHROME_HEIGHT);
        let resp = uzor_framework::widgets::kit::dom(strip, &sheet, tree).build(cx);
        self.chrome_hits = resp.dom.hits().map(|(k, r)| (k.to_string(), r)).collect();
        if cx.view.raw_pointer().is_none() {
            self.chrome_hover = None;
        }
        let (x, y) = self.pointer.last.unwrap_or(self.input.last_mouse_pos);
        // A tab: its press goes to the app raw, as before (it arms the drag, renames
        // on a double press, switches the preset); the release follows from the
        // pointer poll.
        let tab_down = self.chrome_hits.iter().any(|(k, _)| dom::chrome::tab_index(k).is_some() && cx.view.is_pressed(&uzor::WidgetId::from(k.as_str())));
        if tab_down && !self.chrome_tab_down {
            use mlc_app_input_shell::{MouseBtn, ShellEvent};
            self.dispatch(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: true, x, y });
            self.pointer.held = Some(MouseBtn::Left);
            self.chrome_tab_forwarded = true;
        }
        self.chrome_tab_down = tab_down;
        if let Some(id) = resp.clicked.as_deref().filter(|id| dom::chrome::tab_index(id).is_some()) {
            // Down and up between two frames: the whole press, once.
            if !std::mem::take(&mut self.chrome_tab_forwarded) {
                use mlc_app_input_shell::{MouseBtn, ShellEvent};
                let _ = id;
                self.dispatch(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: true, x, y });
                self.dispatch(ShellEvent::MouseInput { button: MouseBtn::Left, pressed: false, x, y });
            }
        }
        if let Some(hit) = resp.clicked.as_deref().and_then(dom::chrome::hit_of) {
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
        let app_scroll = st.scroll.animated_offset();
        let tree = cs::tree(st, &data.instrument, at);
        // The body scrolls itself; it opens where the app's own offset is.
        let seed = (!self.cs_scroll_seeded).then_some(app_scroll);
        if let Some(y) = seed {
            self.cs_scroll_seeded = true;
            if y > 0.0 {
                uzor_framework::widgets::kit::dom_scroll_to(cx, cs::VIEWPORT, y, false);
            }
        }
        let scroll = match seed {
            Some(y) => y,
            None => uzor_framework::widgets::kit::dom_scroll_top(cx, cs::VIEWPORT),
        };
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
        let resp = uzor_framework::widgets::kit::dom(cx.rect, &sheet, tree).paint_slots(&mut painter).dom_scroll().build(cx);
        if let Some((_, y)) = resp.scrolls.iter().find(|(k, _)| seed.is_none() && k == cs::VIEWPORT) {
            let s = &mut self.chart.panel_app.chart_settings_state.scroll;
            s.offset = *y;
            s.target = *y;
        }
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
        let was_shown = std::mem::replace(&mut self.chart_settings_shown, false);
        if !self.preset_name_open() {
            self.preset_name = None;
            self.last_hits.clear();
            if let Some(data) = self.chart_settings_dom() {
                if !was_shown {
                    self.cs_scroll_seeded = false;
                }
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

    /// A wheel step while a DOM piece is up and no DOM scroll container took it: it
    /// is swallowed (the modal is on top; the chart settings body scrolls itself).
    fn dom_wheel(&mut self, _u: &uzor_framework::UnhandledInput, _dy: f64) {}

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
                PlatformCmd::Cursor(icon) => AppCommand::Window(WindowCmd::SetCursor { win, icon }),
                PlatformCmd::CursorVisible(visible) => AppCommand::Window(WindowCmd::SetCursorVisible { win, visible }),
                PlatformCmd::DragMove => AppCommand::Window(WindowCmd::DragMove(win)),
                PlatformCmd::DragResize(dir) => AppCommand::Window(WindowCmd::DragResize { win, dir }),
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
        // After the toolbars: the app paints the sidebar over the top toolbar's rule
        // (its backdrop smears onto it).
        self.sidebar_layer(cx);
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
