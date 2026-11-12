use mlc_app_input_shell::{LogicalKey, MouseBtn, NormalizedModifiers, PhysicalKey, ShellEvent, UNITS_PER_NOTCH};
use uzor::input::keyboard::KeyCode;
use uzor_framework::{KeyInput, KeyState, PointerInput, UnhandledInput, WheelDelta, WheelInput};

use crate::input::shell_events;

fn none() -> NormalizedModifiers {
    NormalizedModifiers::default()
}

fn win() -> uzor_framework::WindowId {
    uzor_framework::WindowId(1)
}

#[test]
fn a_click_becomes_press_and_release_at_the_same_point() {
    let pos = uzor_framework::Point { x: 10.0, y: 20.0 };
    let down = UnhandledInput::Pointer { win: win(), pointer: PointerInput::Down { pos, button: uzor::input::MouseButton::Left, mods: Default::default() } };
    let up = UnhandledInput::Pointer { win: win(), pointer: PointerInput::Up { pos, button: uzor::input::MouseButton::Left, mods: Default::default() } };
    let mut ev = shell_events(&down, none());
    ev.extend(shell_events(&up, none()));
    assert!(matches!(ev[0], ShellEvent::MouseInput { button: MouseBtn::Left, pressed: true, x, y } if x == 10.0 && y == 20.0));
    assert!(matches!(ev[1], ShellEvent::MouseInput { button: MouseBtn::Left, pressed: false, .. }));
}

#[test]
fn a_modifier_change_goes_before_the_event() {
    let mods = uzor::input::ModifierKeys { ctrl: true, ..Default::default() };
    let pos = uzor_framework::Point { x: 1.0, y: 1.0 };
    let ev = shell_events(&UnhandledInput::Pointer { win: win(), pointer: PointerInput::Moved { pos, mods } }, none());
    assert!(matches!(ev[0], ShellEvent::ModifiersChanged(m) if m.ctrl && !m.shift));
    assert!(matches!(ev[1], ShellEvent::CursorMoved { .. }));
    // Already held: no repeat.
    let held = NormalizedModifiers { ctrl: true, ..none() };
    assert_eq!(shell_events(&UnhandledInput::Pointer { win: win(), pointer: PointerInput::Moved { pos, mods } }, held).len(), 1);
}

#[test]
fn wheel_lines_are_notches_and_pixels_are_continuous() {
    let lines = WheelInput { pos: Default::default(), delta: WheelDelta::Lines { x: 0.0, y: 1.0 }, mods: Default::default() };
    let ev = shell_events(&UnhandledInput::Wheel { win: win(), wheel: lines }, none());
    assert!(matches!(ev[0], ShellEvent::Scroll { dy, continuous: false, .. } if dy == UNITS_PER_NOTCH));
    let px = WheelInput { pos: Default::default(), delta: WheelDelta::Pixels { x: 0.0, y: 50.0 }, mods: Default::default() };
    let ev = shell_events(&UnhandledInput::Wheel { win: win(), wheel: px }, none());
    assert!(matches!(ev[0], ShellEvent::Scroll { continuous: true, .. }));
}

#[test]
fn keys_map_to_the_shells_logical_and_physical_keys() {
    let k = |code, text: Option<&str>, state| KeyInput { code, text: text.map(str::to_string), state, mods: Default::default() };
    let ev = shell_events(&UnhandledInput::Key { win: win(), key: k(KeyCode::Escape, None, KeyState::Down) }, none());
    assert!(matches!(&ev[0], ShellEvent::KeyPressed { logical: LogicalKey::Escape, .. }));
    let ev = shell_events(&UnhandledInput::Key { win: win(), key: k(KeyCode::Z, Some("z"), KeyState::Down) }, none());
    assert!(matches!(&ev[0], ShellEvent::KeyPressed { logical: LogicalKey::Character(t), physical: PhysicalKey::KeyZ } if t == "z"));
    assert!(shell_events(&UnhandledInput::Key { win: win(), key: k(KeyCode::Z, None, KeyState::Up) }, none()).is_empty());
}

// ── The hosted app (a real ChartApp: run with `--ignored`, HOME at a scratch dir) ──

mod hosted {
    use crate::dom::preset_name as pn;
    use crate::{HostSpec, HostedApp};
    use uzor_framework::{HeadlessHost, InputEvent, KeyInput, KeyState, PointerInput, WindowId};

    type Host = HeadlessHost<HostSpec, HostedApp>;

    fn host() -> (Host, WindowId) {
        let app = HostedApp::with_dom(mlc_app::ChartApp::new("BTCUSDT"));
        let (mut h, _) = HeadlessHost::new(app, HostedApp::runtime_config(1280, 800));
        for _ in 0..3 {
            h.advance(0.017);
        }
        let win = h.snapshot().windows[0].id;
        (h, win)
    }

    fn hit(h: &Host, id: &str) -> uzor::types::Rect {
        let app = h.runtime().app();
        app.last_hits.iter().chain(&app.chrome_hits).find(|(k, _)| k == id).map(|(_, r)| *r).unwrap_or_else(|| panic!("no hit {id}"))
    }

    fn click(h: &mut Host, win: WindowId, id: &str) {
        let r = hit(h, id);
        let pos = uzor_framework::Point { x: r.x + r.width / 2.0, y: r.y + r.height / 2.0 };
        let b = uzor::input::MouseButton::Left;
        h.input(win, InputEvent::Pointer(PointerInput::Moved { pos, mods: Default::default() }));
        h.advance(0.017);
        h.input(win, InputEvent::Pointer(PointerInput::Down { pos, button: b, mods: Default::default() }));
        h.advance(0.017);
        h.input(win, InputEvent::Pointer(PointerInput::Up { pos, button: b, mods: Default::default() }));
        h.advance(0.017);
        h.advance(0.017);
        // The click lands in a frame's DOM pass; one more frame paints its result.
        h.advance(0.017);
    }

    fn key(h: &mut Host, win: WindowId, code: uzor::input::keyboard::KeyCode, text: Option<&str>) {
        for state in [KeyState::Down, KeyState::Up] {
            h.input(win, InputEvent::Key(KeyInput { code, text: text.map(str::to_string), state, mods: Default::default() }));
        }
        h.advance(0.017);
        h.advance(0.017);
    }

    fn open_save_as(h: &mut Host) {
        h.runtime_mut().app_mut().chart.open_preset_save_as();
        h.advance(0.017);
        h.advance(0.017);
    }

    fn presets(h: &Host) -> Vec<String> {
        h.runtime().app().chart.core.presets().values().map(|p| p.name.clone()).collect()
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_dom_dialog_replaces_the_hand_drawn_one_and_saves() {
        let (mut h, win) = host();
        open_save_as(&mut h);
        assert!(h.runtime().app().preset_name.is_some(), "the DOM dialog is open");
        // The field has focus: typing appends to the default name.
        key(&mut h, win, uzor::input::keyboard::KeyCode::X, Some("x"));
        let name = h.runtime().app().preset_name.clone().unwrap();
        assert!(name.starts_with("Untitled ") && name.ends_with('x'), "{name}");
        click(&mut h, win, pn::SAVE);
        assert!(!h.runtime().app().chart.panel_app.preset_name_input.is_open);
        assert!(presets(&h).contains(&name), "saved as {name}: {:?}", presets(&h));
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn enter_saves_and_close_and_escape_do_not() {
        let (mut h, win) = host();
        open_save_as(&mut h);
        click(&mut h, win, pn::CLOSE);
        assert!(!h.runtime().app().chart.panel_app.preset_name_input.is_open, "✕ closes");
        let before = presets(&h).len();
        open_save_as(&mut h);
        key(&mut h, win, uzor::input::keyboard::KeyCode::Escape, None);
        assert!(!h.runtime().app().chart.panel_app.preset_name_input.is_open, "Escape closes");
        assert_eq!(presets(&h).len(), before);
        open_save_as(&mut h);
        key(&mut h, win, uzor::input::keyboard::KeyCode::Enter, None);
        assert!(!h.runtime().app().chart.panel_app.preset_name_input.is_open, "Enter saves");
        assert_eq!(presets(&h).len(), before + 1);
    }

    // ── Chart settings on DOM ─────────────────────────────────────────────

    fn open_chart_settings(h: &mut Host) {
        h.runtime_mut().app_mut().chart.open_settings();
        h.advance(0.017);
        h.advance(0.017);
    }

    fn cs_shown(h: &Host) -> bool {
        h.runtime().app().chart_settings_shown
    }

    fn cs_state(h: &Host) -> &mlc_app_layout::ui::modal_settings::ChartSettingsState {
        &h.runtime().app().chart.panel_app.chart_settings_state
    }

    fn body_enabled(h: &Host) -> bool {
        h.runtime().app().chart.build_chart_settings_data().instrument.body_enabled
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn chart_settings_checkbox_goes_through_the_apps_handler_and_ok_closes() {
        let (mut h, win) = host();
        open_chart_settings(&mut h);
        assert!(cs_shown(&h), "the Instrument tab is on DOM");
        let before = body_enabled(&h);
        click(&mut h, win, "cs:item:instrument:body_enabled");
        assert_eq!(body_enabled(&h), !before, "the checkbox toggled the app's setting");
        click(&mut h, win, "cs:foot:0");
        assert!(!cs_state(&h).is_open, "OK closes");
        assert!(!cs_shown(&h));
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn chart_settings_close_and_other_tabs_fall_back() {
        let (mut h, win) = host();
        open_chart_settings(&mut h);
        click(&mut h, win, "cs:tab:status_line");
        assert_eq!(cs_state(&h).active_tab, mlc_app_layout::ui::modal_settings::ChartSettingsTab::StatusLine);
        assert!(cs_state(&h).is_open && !cs_shown(&h), "another tab is still the app's renderer");
        h.runtime_mut().app_mut().chart.panel_app.chart_settings_state.active_tab = Default::default();
        h.advance(0.017);
        h.advance(0.017);
        assert!(cs_shown(&h), "back on the Instrument tab");
        click(&mut h, win, "cs:item:dropdown_menu:candle_style");
        assert!(cs_state(&h).active_dropdown.is_some(), "the chevron opens the app's list");
        assert!(!cs_shown(&h), "an open list is drawn by the app");
        h.runtime_mut().app_mut().chart.panel_app.chart_settings_state.active_dropdown = None;
        h.advance(0.017);
        h.advance(0.017);
        click(&mut h, win, "cs:close");
        assert!(!cs_state(&h).is_open, "the close button closes");
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_wheel_scrolls_the_body_and_not_the_chart() {
        let (mut h, win) = host();
        open_chart_settings(&mut h);
        let r = hit(&h, crate::dom::chart_settings::VIEWPORT);
        let pos = uzor_framework::Point { x: r.x + r.width / 2.0, y: r.y + r.height / 2.0 };
        let wheel = |y: f64| InputEvent::Wheel(uzor_framework::WheelInput { pos, delta: uzor_framework::WheelDelta::Lines { x: 0.0, y }, mods: Default::default() });
        h.input(win, wheel(-3.0));
        for _ in 0..30 {
            h.advance(0.017);
        }
        let off = cs_state(&h).scroll.target;
        assert!(off > 0.0, "scrolled down: {off}");
        // Outside the body the wheel is swallowed: the body stays where it is.
        let pos = uzor_framework::Point { x: 10.0, y: 400.0 };
        h.input(win, InputEvent::Wheel(uzor_framework::WheelInput { pos, delta: uzor_framework::WheelDelta::Lines { x: 0.0, y: 3.0 }, mods: Default::default() }));
        for _ in 0..30 {
            h.advance(0.017);
        }
        assert_eq!(cs_state(&h).scroll.target, off);
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_body_reopens_where_the_app_left_it() {
        let (mut h, win) = host();
        open_chart_settings(&mut h);
        let r = hit(&h, crate::dom::chart_settings::VIEWPORT);
        let pos = uzor_framework::Point { x: r.x + r.width / 2.0, y: r.y + r.height / 2.0 };
        h.input(win, InputEvent::Wheel(uzor_framework::WheelInput { pos, delta: uzor_framework::WheelDelta::Lines { x: 0.0, y: -2.0 }, mods: Default::default() }));
        for _ in 0..30 {
            h.advance(0.017);
        }
        let off = cs_state(&h).scroll.target;
        assert!(off > 0.0);
        let row = |h: &Host| hit(h, "cs:item:instrument:body_enabled");
        let shifted = row(&h).y;
        click(&mut h, win, "cs:foot:0");
        assert!(!cs_shown(&h));
        open_chart_settings(&mut h);
        assert!(cs_shown(&h));
        assert_eq!(cs_state(&h).scroll.target, off, "the app keeps the offset");
        assert_eq!(row(&h).y, shifted, "the DOM body starts there again");
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn moves_and_releases_reach_the_app_not_only_presses() {
        let (mut h, win) = host();
        let at = |x: f64, y: f64| uzor_framework::Point { x, y };
        let b = uzor::input::MouseButton::Left;
        h.input(win, InputEvent::Pointer(PointerInput::Moved { pos: at(600.0, 400.0), mods: Default::default() }));
        h.advance(0.017);
        h.advance(0.017);
        assert_eq!(h.runtime().app().input.last_mouse_pos, (600.0, 400.0), "a move over the chart reaches mlc");
        h.input(win, InputEvent::Pointer(PointerInput::Down { pos: at(600.0, 400.0), button: b, mods: Default::default() }));
        h.advance(0.017);
        h.advance(0.017);
        assert!(h.runtime().app().input.mouse_pressed, "the press reached mlc");
        h.input(win, InputEvent::Pointer(PointerInput::Moved { pos: at(560.0, 410.0), mods: Default::default() }));
        h.advance(0.017);
        h.advance(0.017);
        assert_eq!(h.runtime().app().input.last_mouse_pos, (560.0, 410.0), "the drag moves reach mlc");
        h.input(win, InputEvent::Pointer(PointerInput::Up { pos: at(560.0, 410.0), button: b, mods: Default::default() }));
        h.advance(0.017);
        h.advance(0.017);
        assert!(!h.runtime().app().input.mouse_pressed, "the release reached mlc");
    }

    fn move_to(h: &mut Host, win: WindowId, x: f64, y: f64) {
        let pos = uzor_framework::Point { x, y };
        h.input(win, InputEvent::Pointer(PointerInput::Moved { pos, mods: Default::default() }));
        h.advance(0.017);
        h.advance(0.017);
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_chrome_strip_is_dom_and_its_buttons_run_the_apps_actions() {
        let (mut h, win) = host();
        let ids: Vec<String> = h.runtime().app().chrome_hits.iter().map(|(k, _)| k.clone()).collect();
        for id in ["chrome:new_tab", "chrome:new_window", "chrome:menu", "chrome:close_window", "chrome:minimize", "chrome:maximize", "chrome:close"] {
            assert!(ids.iter().any(|k| k == id), "{id} in {ids:?}");
        }
        assert_eq!(hit(&h, "chrome:close").x, 1280.0 - 46.0, "the cells keep the old geometry");
        click(&mut h, win, "chrome:new_tab");
        assert_eq!(h.runtime().app().chart.panel_app.toolbar_state.open_dropdown_id.as_deref(), Some("new_tab_menu"), "+ opens the new-tab menu");
        click(&mut h, win, "chrome:new_tab");
        assert_eq!(h.runtime().app().chart.panel_app.toolbar_state.open_dropdown_id, None, "and closes it again");
        click(&mut h, win, "chrome:menu");
        assert!(h.runtime().app().chart.panel_app.user_settings_state.is_open, "the gear opens user settings");
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn chrome_hover_is_the_doms_and_tab_and_caption_presses_still_go_to_the_app() {
        use mlc_app_input_shell::chrome::ChromeHit;
        let (mut h, win) = host();
        strip_pixels(&mut h, win, true);
        move_to(&mut h, win, 20.0, 16.0);
        assert_eq!(crate::dom::chrome::hovered(h.runtime().app().chrome_hover.as_deref()), ChromeHit::Tab(0), "the DOM's hover");
        assert_eq!(h.runtime().app().chrome.hovered, ChromeHit::Tab(0), "and the app's agree");
        assert!(h.runtime().app().chrome_hits.iter().any(|(k, _)| k == "chrome:tab_close:0"), "the hovered tab shows its ×");
        // A double press on a tab: the DOM tab hands both presses to the app, which
        // starts renaming it (and paints the strip itself while it does).
        press_at(&mut h, win, 30.0, 16.0);
        press_at(&mut h, win, 30.0, 16.0);
        assert_eq!(h.runtime().app().chrome.renaming_tab, Some(0), "a double press renames the tab");
        h.runtime_mut().app_mut().chrome.renaming_tab = None;
        // A press on the empty strip has no DOM cell: the app's own hit test makes it
        // a window drag, sent to the host as an OS window move.
        let pos = uzor_framework::Point { x: 600.0, y: 16.0 };
        let b = uzor::input::MouseButton::Left;
        move_to(&mut h, win, pos.x, pos.y);
        h.input(win, InputEvent::Pointer(PointerInput::Down { pos, button: b, mods: Default::default() }));
        h.advance(0.017);
        h.input(win, InputEvent::Pointer(PointerInput::Up { pos, button: b, mods: Default::default() }));
        h.advance(0.017);
        assert!(h.command_log().iter().any(|(w, c)| *w == win && matches!(c, uzor_framework::WindowCommand::DragWindow)), "{:?}", h.command_log());
        assert!(h.runtime().app().platform.unsupported.borrow().is_empty());
    }

    /// The strip rows of one frame, painted with the DOM strip on or off.
    fn strip_pixels(h: &mut Host, win: WindowId, dom: bool) -> Vec<u8> {
        use uzor::render::{Painter, ShapeHelpers};
        h.runtime_mut().app_mut().chrome_on_dom = dom;
        let mut ctx = uzor_render_tiny_skia::TinySkiaCpuRenderContext::new(1280, 800, 1.0);
        ctx.set_fill_color("#131722");
        ctx.fill_rect(0.0, 0.0, 1280.0, 800.0);
        h.paint_window(win, 1.0, &mut ctx).expect("window");
        ctx.pixmap().data()[..1280 * 32 * 4].to_vec()
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_dom_strip_paints_what_the_old_strip_painted_in_every_hover_state() {
        use mlc_app_input_shell::chrome::ChromeHit;
        let (mut h, win) = host();
        let spots: [(f64, f64, ChromeHit); 9] = [
            (20.0, 16.0, ChromeHit::Tab(0)),
            (72.0, 16.0, ChromeHit::TabClose(0)),
            (90.0, 16.0, ChromeHit::NewTabButton),
            (1060.0, 16.0, ChromeHit::NewWindowButton),
            (1090.0, 16.0, ChromeHit::MenuButton),
            (1120.0, 16.0, ChromeHit::CloseWindowButton),
            (1160.0, 16.0, ChromeHit::MinimizeButton),
            (1210.0, 16.0, ChromeHit::MaximizeButton),
            (1260.0, 16.0, ChromeHit::CloseButton),
        ];
        strip_pixels(&mut h, win, true);
        for maximized in [false, true] {
            for (x, y, want) in spots {
                move_to(&mut h, win, x, y);
                h.runtime_mut().app_mut().chrome.is_maximized = maximized;
                assert_eq!(h.runtime().app().chrome.hovered, want, "hover at {x},{y}");
                assert_eq!(crate::dom::chrome::hovered(h.runtime().app().chrome_hover.as_deref()), want, "DOM hover at {x},{y}");
                let old = strip_pixels(&mut h, win, false);
                let new = strip_pixels(&mut h, win, true);
                assert!(!h.runtime().app().chrome_hits.is_empty(), "the DOM strip drew {want:?}");
                let off = old.chunks_exact(4).zip(new.chunks_exact(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(p, q)| p.abs_diff(*q) > 8)).count();
                if let Some(dir) = std::env::var_os("MLC_TB_DUMP") {
                    for (n, px) in [("old", &old), ("new", &new)] {
                        let path = std::path::Path::new(&dir).join(format!("strip-{n}-{x}-{maximized}.png"));
                        let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
                        let mut enc = png::Encoder::new(f, 1280, 32);
                        enc.set_color(png::ColorType::Rgba);
                        enc.set_depth(png::BitDepth::Eight);
                        enc.write_header().unwrap().write_image_data(px).unwrap();
                    }
                }
                if off > 4 {
                    let xs: Vec<usize> = old.chunks_exact(4).zip(new.chunks_exact(4)).enumerate().filter(|(_, (a, b))| a.iter().zip(b.iter()).any(|(p, q)| p.abs_diff(*q) > 8)).map(|(i, _)| i).collect();
                    let (x0, x1) = (xs.iter().map(|i| i % 1280).min().unwrap(), xs.iter().map(|i| i % 1280).max().unwrap());
                    let (y0, y1) = (xs.iter().map(|i| i / 1280).min().unwrap(), xs.iter().map(|i| i / 1280).max().unwrap());
                    let i = xs[0] * 4;
                    panic!("{want:?} (maximized {maximized}): {off} pixels differ in x {x0}..={x1}, y {y0}..={y1}; first old {:?} new {:?}", &old[i..i + 4], &new[i..i + 4]);
                }
            }
        }
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_toolbars_are_dom() {
        let (mut h, _win) = host();
        h.advance(0.017);
        let slots = h.runtime().app().toolbar_slots.clone();
        for s in ["top", "left", "right"] {
            assert!(slots.contains(&s), "{s} in {slots:?}");
        }
    }

    /// The window painted with the DOM toolbars on or off.
    fn window_pixels(h: &mut Host, win: WindowId, dom: bool) -> Vec<u8> {
        use uzor::render::{Painter, ShapeHelpers};
        h.runtime_mut().app_mut().toolbars_on_dom = dom;
        let mut ctx = uzor_render_tiny_skia::TinySkiaCpuRenderContext::new(1280, 800, 1.0);
        ctx.set_fill_color("#131722");
        ctx.fill_rect(0.0, 0.0, 1280.0, 800.0);
        h.paint_window(win, 1.0, &mut ctx).expect("window");
        ctx.pixmap().data().to_vec()
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_dom_toolbars_paint_what_the_old_ones_painted_under_the_pointer() {
        let (mut h, win) = host();
        // Top: symbol dropdown, timeframe button, an icon button; left: the active
        // crosshair, a split tool, a plain tool; right: two rail buttons, the mascot.
        let spots = [(60.0, 52.0), (210.0, 52.0), (310.0, 52.0), (24.0, 96.0), (24.0, 142.0), (24.0, 420.0), (1254.0, 96.0), (1254.0, 140.0), (1254.0, 325.0)];
        let in_toolbars = |x: usize, y: usize| (32..72).contains(&y) || (y >= 72 && (x < 48 || x >= 1228));
        // A painted frame first, as a window has: the passes that only hit-test
        // reuse the label widths it measured.
        window_pixels(&mut h, win, true);
        for (x, y) in spots {
            move_to(&mut h, win, x, y);
            let old = window_pixels(&mut h, win, false);
            let new = window_pixels(&mut h, win, true);
            assert_eq!(h.runtime().app().toolbar_slots.len(), 3, "all three on DOM at {x},{y}");
            let off: Vec<usize> = old
                .chunks_exact(4)
                .zip(new.chunks_exact(4))
                .enumerate()
                .filter(|(i, (a, b))| in_toolbars(i % 1280, i / 1280) && a.iter().zip(b.iter()).any(|(p, q)| p.abs_diff(*q) > 8))
                .map(|(i, _)| i)
                .collect();
            if let Some(dir) = std::env::var_os("MLC_TB_DUMP") {
                for (n, px) in [("old", &old), ("new", &new)] {
                    let path = std::path::Path::new(&dir).join(format!("tb-{n}-{x}-{y}.png"));
                    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
                    let mut enc = png::Encoder::new(f, 1280, 800);
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_depth(png::BitDepth::Eight);
                    enc.write_header().unwrap().write_image_data(px).unwrap();
                }
            }
            if off.len() > 40 {
                let (x0, x1) = (off.iter().map(|i| i % 1280).min().unwrap(), off.iter().map(|i| i % 1280).max().unwrap());
                let (y0, y1) = (off.iter().map(|i| i / 1280).min().unwrap(), off.iter().map(|i| i / 1280).max().unwrap());
                panic!("hover at {x},{y}: {} pixels differ in x {x0}..={x1}, y {y0}..={y1}", off.len());
            }
        }
    }

    fn press_at(h: &mut Host, win: WindowId, x: f64, y: f64) {
        let pos = uzor_framework::Point { x, y };
        let b = uzor::input::MouseButton::Left;
        move_to(h, win, x, y);
        h.input(win, InputEvent::Pointer(PointerInput::Down { pos, button: b, mods: Default::default() }));
        h.advance(0.017);
        h.input(win, InputEvent::Pointer(PointerInput::Up { pos, button: b, mods: Default::default() }));
        h.advance(0.017);
        h.advance(0.017);
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn toolbar_presses_reach_the_app_as_before() {
        // The DOM toolbars take the presses and hand them to the app as its raw
        // press and release: the same presses with the app's own toolbars must leave
        // the same state (a dropdown, a rail panel, the active drawing tool).
        let (mut dom, wd) = host();
        let (mut old, wo) = host();
        old.runtime_mut().app_mut().toolbars_on_dom = false;
        window_pixels(&mut dom, wd, true);
        window_pixels(&mut old, wo, false);
        let mut opened = 0;
        for (x, y) in [(210.0, 52.0), (60.0, 52.0), (1254.0, 96.0), (24.0, 142.0)] {
            press_at(&mut dom, wd, x, y);
            press_at(&mut old, wo, x, y);
            let a = dom.runtime().app().chart.panel_app.toolbar_state.open_dropdown_id.clone();
            let b = old.runtime().app().chart.panel_app.toolbar_state.open_dropdown_id.clone();
            assert_eq!(a, b, "press at {x},{y}");
            let ra = dom.runtime().app().chart.panel_app.toolbar_state.active_right_toolbar_id.clone();
            let rb = old.runtime().app().chart.panel_app.toolbar_state.active_right_toolbar_id.clone();
            assert_eq!(ra, rb, "press at {x},{y}");
            let ta = dom.runtime().app().chart.panel_app.toolbar_state.active_tool_id.clone();
            let tb = old.runtime().app().chart.panel_app.toolbar_state.active_tool_id.clone();
            assert_eq!(ta, tb, "press at {x},{y}");
            opened += usize::from(a.is_some()) + usize::from(ra.is_some());
            press_at(&mut dom, wd, 700.0, 400.0);
            press_at(&mut old, wo, 700.0, 400.0);
        }
        assert!(opened > 0, "some press opened something");
        assert_eq!(dom.runtime().app().toolbar_slots.len(), 3);
    }

    fn open_watchlist(h: &mut Host, rows: usize) {
        let app = &mut h.runtime_mut().app_mut().chart;
        app.sidebar_state.toggle_right_panel(mlc_app::ChartApp::str_to_panel("watchlist"));
        app.sidebar_data_dirty = true;
        for _ in 0..3 {
            h.advance(0.017);
        }
        let items = &mut h.runtime_mut().app_mut().chart.sidebar_state.watchlist_items;
        for i in 0..rows {
            items.push(mlc_app_sidebar_content::types::WatchlistItem {
                symbol: format!("SYM{i}USDT"),
                exchange: "binance".into(),
                last_price: 100.0 + i as f64 * 7.25,
                change_percent: if i % 2 == 0 { 1.5 } else { -2.25 },
                high_24h: 120.0,
                low_24h: 90.0,
                volume_24h: 1.5e6,
                spark: (0..20).map(|k| ((k * (i + 3)) % 11) as f64).collect(),
                ..Default::default()
            });
        }
        h.advance(0.017);
        h.advance(0.017);
    }

    /// The window painted with the DOM sidebar on or off.
    fn sidebar_pixels(h: &mut Host, win: WindowId, dom: bool) -> Vec<u8> {
        use uzor::render::{Painter, ShapeHelpers};
        h.runtime_mut().app_mut().chart.panel_app.dom_hosted.sidebar = dom;
        h.advance(0.017);
        let mut ctx = uzor_render_tiny_skia::TinySkiaCpuRenderContext::new(1280, 800, 1.0);
        ctx.set_fill_color("#131722");
        ctx.fill_rect(0.0, 0.0, 1280.0, 800.0);
        h.paint_window(win, 1.0, &mut ctx).expect("window");
        // Hover is the DOM's from the frame before: paint once more.
        let mut ctx2 = uzor_render_tiny_skia::TinySkiaCpuRenderContext::new(1280, 800, 1.0);
        ctx2.set_fill_color("#131722");
        ctx2.fill_rect(0.0, 0.0, 1280.0, 800.0);
        h.paint_window(win, 1.0, &mut ctx2).expect("window");
        ctx2.pixmap().data().to_vec()
    }

    fn sb_hit(h: &Host, id: &str) -> uzor::types::Rect {
        h.runtime().app().sidebar_hits.iter().find(|(k, _)| k == id).map(|(_, r)| *r).unwrap_or_else(|| panic!("no sidebar hit {id}"))
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_dom_watchlist_paints_what_the_app_painted_under_the_pointer() {
        let (mut h, win) = host();
        open_watchlist(&mut h, 6);
        sidebar_pixels(&mut h, win, true);
        assert!(!h.runtime().app().sidebar_hits.is_empty(), "the watchlist is on DOM");
        let ids = ["right_sidebar_close", "watchlist_column_config", "watchlist_open_modal", "watchlist_sort_color", "watchlist_1", "watchlist_flag_2", "watchlist_metrics_sep"];
        let spots: Vec<(String, f64, f64)> = ids
            .iter()
            .map(|id| {
                let r = sb_hit(&h, id);
                (id.to_string(), r.x + r.width / 2.0, r.y + r.height / 2.0)
            })
            .chain([("rows".to_string(), 700.0, 400.0)])
            .collect();
        let sb = sb_hit(&h, "right_sidebar_close");
        let left = (sb.x + sb.width + 2.0 - 341.0) as usize;
        for (id, x, y) in spots {
            move_to(&mut h, win, x, y);
            let old = sidebar_pixels(&mut h, win, false);
            let new = sidebar_pixels(&mut h, win, true);
            if id != "rows" {
                assert_eq!(h.runtime().app().sidebar_hover.as_deref(), Some(id.as_str()), "DOM hover at {id}");
            }
            let off: Vec<usize> = old
                .chunks_exact(4)
                .zip(new.chunks_exact(4))
                .enumerate()
                .filter(|(i, (a, b))| (i % 1280) >= left && (i % 1280) < 1228 && (72..760).contains(&(i / 1280)) && a.iter().zip(b.iter()).any(|(p, q)| p.abs_diff(*q) > 8))
                .map(|(i, _)| i)
                .collect();
            if let Some(dir) = std::env::var_os("MLC_SB_DUMP") {
                for (n, px) in [("old", &old), ("new", &new)] {
                    let path = std::path::Path::new(&dir).join(format!("sb-{n}-{id}.png"));
                    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
                    let mut enc = png::Encoder::new(f, 1280, 800);
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_depth(png::BitDepth::Eight);
                    enc.write_header().unwrap().write_image_data(px).unwrap();
                }
            }
            if off.len() > 40 {
                let (x0, x1) = (off.iter().map(|i| i % 1280).min().unwrap(), off.iter().map(|i| i % 1280).max().unwrap());
                let (y0, y1) = (off.iter().map(|i| i / 1280).min().unwrap(), off.iter().map(|i| i / 1280).max().unwrap());
                panic!("hover at {id}: {} pixels differ in x {x0}..={x1}, y {y0}..={y1}", off.len());
            }
        }
    }

    /// Opens right panel `key` and fills it with a few rows.
    fn open_panel(h: &mut Host, key: &str) {
        use mlc_core::alert::{AlertCondition, AlertItem, AlertSource, AlertStatus};
        let app = &mut h.runtime_mut().app_mut().chart;
        app.sidebar_state.toggle_right_panel(mlc_app::ChartApp::str_to_panel(key));
        app.sidebar_data_dirty = true;
        for _ in 0..3 {
            h.advance(0.017);
        }
        let s = &mut h.runtime_mut().app_mut().chart.sidebar_state;
        match key {
            "alerts" => {
                let mut a = AlertItem::new(7, AlertSource::Price { symbol: "BTCUSDT".into() }, "", 71234.5, AlertCondition::CrossingUp, AlertStatus::Active);
                a.timeframe = "1h".into();
                let b = AlertItem::new(8, AlertSource::Drawing { primitive_id: 3, label: "Trend line".into() }, "My line", 70000.0, AlertCondition::GreaterThan, AlertStatus::Triggered);
                s.alert_items = vec![a, b];
            }
            "events" => {
                let app = &mut h.runtime_mut().app_mut().chart;
                let log = app.system_events_mut();
                let first = log.push(mlc_core::SystemEventKind::Tier, "Pro", "Renewed.", false).id;
                log.push(mlc_core::SystemEventKind::Account, "Signed in", "Welcome back. Your settings were synced from the cloud and  your layouts restored.", false);
                log.mark_read(first);
                app.sidebar_data_dirty = true;
            }
            "signals" => {
                use mlc_app_sidebar_content::types::{IndicatorSignalGroup, IndicatorSignalRow};
                let rows = (0..3).map(|i| IndicatorSignalRow { bar_index: 100 + i, signal_type: format!("Cross {i}"), price: 70000.0 + i as f64, strength: 1.0, direction: i as i32 - 1 }).collect();
                s.indicator_signals.groups = vec![IndicatorSignalGroup { instance_id: 4, indicator_name: "RSI".into(), collapsed: false, signals: rows }];
                s.indicator_signals.total_count = 3;
            }
            "screener" => {
                s.screener_items = (0..3)
                    .map(|i| mlc_app_sidebar_content::types::ScreenerItem {
                        symbol: format!("SYM{i}USDT"),
                        exchange: "binance".into(),
                        account_type: "S".into(),
                        last_price: 10.0 + i as f64,
                        change_percent: i as f64 - 1.0,
                        volume_24h_quote: 2.5e6,
                        rsi14: (i > 0).then_some(25.0 + 30.0 * i as f64),
                        ..Default::default()
                    })
                    .collect();
            }
            _ => {}
        }
        h.advance(0.017);
        h.advance(0.017);
    }

    /// DOM vs app pixels of the sidebar with the pointer on each of `ids`.
    fn panel_matches(h: &mut Host, win: WindowId, ids: &[&str]) {
        sidebar_pixels(h, win, true);
        let sb = sb_hit(h, "right_sidebar_close");
        let left = (sb.x + sb.width + 2.0 - 341.0) as usize;
        let spots: Vec<(String, f64, f64)> = ids
            .iter()
            .map(|id| {
                let r = sb_hit(h, id);
                (id.to_string(), r.x + r.width / 2.0, r.y + r.height / 2.0)
            })
            .chain([("away".to_string(), 700.0, 400.0)])
            .collect();
        for (id, x, y) in spots {
            move_to(h, win, x, y);
            let old = sidebar_pixels(h, win, false);
            let new = sidebar_pixels(h, win, true);
            let off: Vec<usize> = old
                .chunks_exact(4)
                .zip(new.chunks_exact(4))
                .enumerate()
                .filter(|(i, (a, b))| (i % 1280) >= left && (i % 1280) < 1228 && (72..760).contains(&(i / 1280)) && a.iter().zip(b.iter()).any(|(p, q)| p.abs_diff(*q) > 8))
                .map(|(i, _)| i)
                .collect();
            if let Some(dir) = std::env::var_os("MLC_SB_DUMP") {
                for (n, px) in [("old", &old), ("new", &new)] {
                    let path = std::path::Path::new(&dir).join(format!("p-{n}-{}.png", id.replace(':', "_")));
                    let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
                    let mut enc = png::Encoder::new(f, 1280, 800);
                    enc.set_color(png::ColorType::Rgba);
                    enc.set_depth(png::BitDepth::Eight);
                    enc.write_header().unwrap().write_image_data(px).unwrap();
                }
            }
            let any = old.chunks_exact(4).zip(new.chunks_exact(4)).enumerate().filter(|(i, (a, b))| (i % 1280) >= left && (i % 1280) < 1228 && (72..760).contains(&(i / 1280)) && a != b).count();
            eprintln!("panel_matches {id}: {any} pixels differ at all, {} by more than 8", off.len());
            if off.len() > 40 {
                let (x0, x1) = (off.iter().map(|i| i % 1280).min().unwrap(), off.iter().map(|i| i % 1280).max().unwrap());
                let (y0, y1) = (off.iter().map(|i| i / 1280).min().unwrap(), off.iter().map(|i| i / 1280).max().unwrap());
                panic!("pointer at {id}: {} pixels differ in x {x0}..={x1}, y {y0}..={y1}", off.len());
            }
        }
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_other_dom_panels_paint_what_the_app_painted() {
        for (key, ids) in [
            ("alerts", &["alert_7", "alert_delete_8", "alert_add_button"][..]),
            ("events", &["events:row:1", "events_mark_all_read"][..]),
            ("signals", &["signal_group_4", "signal_4_101"][..]),
            ("screener", &["screener_row:SYM1USDT:binance:S", "screener_filter_rsi_above", "screener_scan"][..]),
            ("object_tree", &["right_sidebar_close"][..]),
            ("performance", &["monitoring_section:connectors", "perf:backend"][..]),
        ] {
            let (mut h, win) = host();
            open_panel(&mut h, key);
            sidebar_pixels(&mut h, win, true);
            let newest = h.runtime().app().chart.sidebar_state.system_event_items.first().map(|e| format!("events:row:{}", e.id));
            let ids: Vec<&str> = ids.iter().map(|i| if *i == "events:row:1" { newest.as_deref().unwrap_or(i) } else { *i }).collect();
            assert!(!h.runtime().app().sidebar_hits.is_empty(), "{key} is on DOM");
            panel_matches(&mut h, win, &ids);
        }
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_watchlist_fallback_states_paint_what_the_app_painted() {
        // The states the host used to leave to the app: failing rows, a row drag,
        // the colour picker, the column menu and the market counters.
        for state in ["status", "drag", "picker", "colmenu", "counters"] {
            let (mut h, win) = host();
            open_watchlist(&mut h, 5);
            sidebar_pixels(&mut h, win, true);
            let ids: &[&str] = match state {
                "status" => {
                    let items = &mut h.runtime_mut().app_mut().chart.sidebar_state.watchlist_items;
                    items[1].status = Some("Venue unreachable".into());
                    items[3].status = Some("Symbol not listed on this venue any more".into());
                    &["watchlist_1", "watchlist_3"]
                }
                "drag" => {
                    let s = &mut h.runtime_mut().app_mut().chart.sidebar_state;
                    s.watchlist_drag_index = Some(1);
                    s.watchlist_drop_index = Some(3);
                    &["watchlist_0"]
                }
                "picker" => {
                    let r = sb_hit(&h, "watchlist_flag_1");
                    press_at(&mut h, win, r.x + r.width / 2.0, r.y + r.height / 2.0);
                    assert!(h.runtime().app().chart.sidebar_state.watchlist_color_picker_open.is_some(), "the picker opened");
                    &["watchlist_color_1_0", "watchlist_color_1_7"]
                }
                "colmenu" => {
                    h.runtime_mut().app_mut().chart.sidebar_state.watchlist_config_dropdown_open = true;
                    &["watchlist_cfg:show_spark", "watchlist_cfg:show_counters"]
                }
                _ => {
                    let app = &mut h.runtime_mut().app_mut().chart;
                    let id = app.watchlist_ui.active_list_id;
                    app.watchlist_ui.column_config_mut(id).show_market_counters = true;
                    // Pinned rows (the app rebuilds them from live counters only
                    // when its sidebar data goes dirty).
                    app.sidebar_state.counter_rows = (0..3)
                        .map(|k| mlc_app_sidebar_content::types::CounterRowData {
                            venue: ["binance", "bybit", "okx"][k].into(),
                            funding_rate: Some(0.0001 * (k as f64 - 1.0)),
                            countdown_secs: (k != 2).then_some(3725 + k as i64 * 600),
                            d1h_pct: (k != 1).then_some(1.25 - k as f64),
                            d24h_pct: Some(-3.5 + k as f64 * 2.0),
                        })
                        .collect();
                    h.advance(0.017);
                    assert!(!h.runtime().app().chart.sidebar_state.counter_rows.is_empty(), "counter rows");
                    &["watchlist_metrics_sep"]
                }
            };
            h.advance(0.017);
            sidebar_pixels(&mut h, win, true);
            assert!(h.runtime().app().chart.sidebar_on_dom(), "{state} is on DOM");
            panel_matches(&mut h, win, ids);
        }
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn watchlist_presses_reach_the_app_as_before() {
        let (mut dom, wd) = host();
        let (mut old, wo) = host();
        old.runtime_mut().app_mut().chart.panel_app.dom_hosted.sidebar = false;
        for (h, w) in [(&mut dom, wd), (&mut old, wo)] {
            open_watchlist(h, 4);
            sidebar_pixels(h, w, h.runtime().app().chart.panel_app.dom_hosted.sidebar);
        }
        let spot = |h: &Host, id: &str| {
            let r = sb_hit(h, id);
            (r.x + r.width / 2.0, r.y + r.height / 2.0)
        };
        let state = |h: &Host| {
            let s = &h.runtime().app().chart.sidebar_state;
            (s.is_right_open(), s.watchlist_config_dropdown_open, s.watchlist_sort_mode)
        };
        let ids = ["watchlist_sort_color", "watchlist_sort_color", "watchlist_column_config", "watchlist_column_config", "right_sidebar_close"];
        // Where the DOM put them (the column menu, once open, is the app's again).
        let spots: Vec<(f64, f64)> = ids.iter().map(|id| spot(&dom, id)).collect();
        for (id, (x, y)) in ids.iter().zip(spots) {
            press_at(&mut dom, wd, x, y);
            press_at(&mut old, wo, x, y);
            assert_eq!(state(&dom), state(&old), "press on {id}");
            dom.advance(0.017);
        }
        assert!(!state(&dom).0, "the close button closed it");
        assert!(dom.runtime().app().sidebar_hits.is_empty(), "closed: nothing on DOM");
    }

    #[test]
    #[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
    fn the_wheel_scrolls_the_watchlist_rows_into_the_apps_scroll_state() {
        let (mut h, win) = host();
        open_watchlist(&mut h, 40);
        sidebar_pixels(&mut h, win, true);
        let r = sb_hit(&h, "watchlist_10");
        let pos = uzor_framework::Point { x: r.x + 40.0, y: r.y + 10.0 };
        h.input(win, InputEvent::Wheel(uzor_framework::WheelInput { pos, delta: uzor_framework::WheelDelta::Lines { x: 0.0, y: -3.0 }, mods: Default::default() }));
        for _ in 0..30 {
            h.advance(0.017);
        }
        let off = h.runtime().app().chart.sidebar_state.current_right_scroll().offset;
        assert!(off > 50.0, "the rows scrolled ({off})");
        let r2 = sb_hit(&h, "watchlist_10");
        assert!((r.y - r2.y - off).abs() < 1.0, "row 10 moved up by the offset: {} -> {} ({off})", r.y, r2.y);
    }
}
