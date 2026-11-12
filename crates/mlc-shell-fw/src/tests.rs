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
