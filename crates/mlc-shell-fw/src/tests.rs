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
        h.runtime().app().last_hits.iter().find(|(k, _)| k == id).map(|(_, r)| *r).unwrap_or_else(|| panic!("no hit {id}"))
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
}
