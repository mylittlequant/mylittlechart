//! uzor-framework input → mlc's platform-neutral `ShellEvent`s (the conversions the
//! winit shell's `window/event_handler.rs` makes from winit events).

use mlc_app_input_shell::{pixels_to_notch_units, LogicalKey, MouseBtn, NormalizedModifiers, PhysicalKey, ShellEvent, UNITS_PER_NOTCH};
use uzor::input::keyboard::KeyCode;
use uzor_framework::{KeyState, PointerInput, UnhandledInput, WheelDelta};

fn mods_of(m: &uzor::input::ModifierKeys) -> NormalizedModifiers {
    NormalizedModifiers { ctrl: m.ctrl || m.meta, shift: m.shift, alt: m.alt }
}

fn same(a: NormalizedModifiers, b: NormalizedModifiers) -> bool {
    a.ctrl == b.ctrl && a.shift == b.shift && a.alt == b.alt
}

/// The `ShellEvent` for one key press (`None` for a release).
pub fn shell_event(key: &uzor_framework::KeyInput) -> Option<ShellEvent> {
    if key.state == KeyState::Up {
        return None;
    }
    let logical = match key.code {
        KeyCode::Escape => LogicalKey::Escape,
        KeyCode::Backspace => LogicalKey::Backspace,
        KeyCode::Enter => LogicalKey::Enter,
        KeyCode::Space => LogicalKey::Space,
        KeyCode::Tab => LogicalKey::Tab,
        KeyCode::Delete => LogicalKey::Delete,
        KeyCode::ArrowLeft => LogicalKey::ArrowLeft,
        KeyCode::ArrowRight => LogicalKey::ArrowRight,
        KeyCode::ArrowUp => LogicalKey::ArrowUp,
        KeyCode::ArrowDown => LogicalKey::ArrowDown,
        KeyCode::Home => LogicalKey::Home,
        KeyCode::End => LogicalKey::End,
        KeyCode::PageUp => LogicalKey::PageUp,
        KeyCode::PageDown => LogicalKey::PageDown,
        _ => match &key.text {
            Some(t) if !t.is_empty() => LogicalKey::Character(t.clone()),
            _ => LogicalKey::Other,
        },
    };
    let physical = match key.code {
        KeyCode::A => PhysicalKey::KeyA,
        KeyCode::B => PhysicalKey::KeyB,
        KeyCode::C => PhysicalKey::KeyC,
        KeyCode::S => PhysicalKey::KeyS,
        KeyCode::V => PhysicalKey::KeyV,
        KeyCode::X => PhysicalKey::KeyX,
        KeyCode::Y => PhysicalKey::KeyY,
        KeyCode::Z => PhysicalKey::KeyZ,
        _ => PhysicalKey::Other,
    };
    Some(ShellEvent::KeyPressed { logical, physical })
}

/// The `ShellEvent`s for one unhandled framework input; `held` is the modifier state
/// mlc last saw (a change goes first, as winit reports `ModifiersChanged` first).
pub fn shell_events(u: &UnhandledInput, held: NormalizedModifiers) -> Vec<ShellEvent> {
    let mut out = Vec::new();
    let mods = |m: &uzor::input::ModifierKeys, out: &mut Vec<ShellEvent>| {
        let n = mods_of(m);
        if !same(n, held) {
            out.push(ShellEvent::ModifiersChanged(n));
        }
    };
    match u {
        UnhandledInput::Pointer { pointer, .. } => match pointer {
            PointerInput::Moved { pos, mods: m } => {
                mods(m, &mut out);
                out.push(ShellEvent::CursorMoved { x: pos.x, y: pos.y });
            }
            PointerInput::Down { pos, button, mods: m } | PointerInput::Up { pos, button, mods: m } => {
                mods(m, &mut out);
                let pressed = matches!(pointer, PointerInput::Down { .. });
                let button = match button {
                    uzor::input::MouseButton::Left => MouseBtn::Left,
                    uzor::input::MouseButton::Right => MouseBtn::Right,
                    uzor::input::MouseButton::Middle => MouseBtn::Middle,
                };
                out.push(ShellEvent::MouseInput { button, pressed, x: pos.x, y: pos.y });
            }
            PointerInput::Left | PointerInput::Cancelled => out.push(ShellEvent::CursorLeft),
            _ => {}
        },
        UnhandledInput::Wheel { wheel, .. } => {
            mods(&wheel.mods, &mut out);
            let (dx, dy, continuous) = match wheel.delta {
                WheelDelta::Lines { x, y } => (x * UNITS_PER_NOTCH, y * UNITS_PER_NOTCH, false),
                WheelDelta::Pixels { x, y } => (pixels_to_notch_units(x), pixels_to_notch_units(y), true),
            };
            out.push(ShellEvent::Scroll { dx, dy, continuous, pinch: false });
        }
        UnhandledInput::Key { key, .. } => {
            mods(&key.mods, &mut out);
            out.extend(shell_event(key));
        }
        UnhandledInput::Paste { text, .. } => out.push(ShellEvent::ImeCommit(text.clone())),
    }
    out
}
