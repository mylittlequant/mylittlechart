//! Platform-neutral window event types.

/// Platform-neutral window events (port of winit::WindowEvent subset).
pub enum ShellEvent {
    CloseRequested,
    /// Focus gained.
    Focused,
    /// (0,0) = minimized.
    Resized { w: u32, h: u32 },
    Moved,
    CursorMoved { x: f64, y: f64 },
    CursorLeft,
    /// `x`,`y` come from the CALLER who tracks last_mouse_pos — NOT from winit event
    /// (winit MouseInput carries no position). Both shells must fill these.
    MouseInput { button: MouseBtn, pressed: bool, x: f64, y: f64 },
    /// Pre-normalized to this crate's own "~[`UNITS_PER_NOTCH`] units per
    /// physical wheel notch" contract by the caller: native's `LineDelta *
    /// UNITS_PER_NOTCH`, and both native `PixelDelta` (trackpad) and web
    /// `PlatformEvent::Scroll` (CSS pixels, see `uzor::input::core::
    /// event_processor::PlatformEvent::Scroll`'s own doc) go through
    /// [`pixels_to_notch_units`] first — so a physical notch reads the same
    /// magnitude regardless of platform or input device.
    ///
    /// `continuous` is `true` for pixel-mode input (a trackpad's two-finger
    /// scroll and pinch, a pixel-mode wheel) and `false` for line-mode wheel
    /// notches; `pinch` marks a trackpad pinch (a browser reports it as a
    /// wheel event with `ctrlKey` set).
    Scroll { dx: f64, dy: f64, continuous: bool, pinch: bool },
    ModifiersChanged(NormalizedModifiers),
    KeyPressed { logical: LogicalKey, physical: PhysicalKey },
    ImeCommit(String),
}

/// Platform-neutral modifier state.
#[derive(Clone, Copy, Default)]
pub struct NormalizedModifiers {
    pub ctrl:  bool,
    pub shift: bool,
    pub alt:   bool,
}

/// Platform-neutral mouse button.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MouseBtn {
    Left,
    Right,
    Middle,
    Other,
}

/// Platform-neutral logical key.
#[derive(Clone, PartialEq, Eq)]
pub enum LogicalKey {
    Escape,
    Backspace,
    Enter,
    Space,
    Tab,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    /// Matches winit Key::Character — one allocation per keystroke.
    Character(String),
    Other,
}

/// Platform-neutral physical key (position-based, layout-independent).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PhysicalKey {
    KeyA,
    KeyB,
    KeyC,
    KeyS,
    KeyV,
    KeyX,
    KeyY,
    KeyZ,
    Other,
}

/// Chromium/Windows' default one-wheel-notch magnitude, in raw pixels —
/// `WheelEvent.deltaY`'s pixel-mode value on that platform combo, and the
/// same order of magnitude winit's raw `PixelDelta` reports for an ordinary
/// mouse wheel there. Both shells' raw-pixel wheel paths (web's normalized
/// `PlatformEvent::Scroll`, native's `PixelDelta` trackpad branch) convert
/// through this into [`UNITS_PER_NOTCH`] via [`pixels_to_notch_units`], so
/// a physical notch produces the same `ShellEvent::Scroll` magnitude
/// wherever it came from.
pub const PIXELS_PER_NOTCH: f64 = 100.0;

/// This crate's own per-notch contract for `ShellEvent::Scroll` (see its
/// doc comment above).
pub const UNITS_PER_NOTCH: f64 = 20.0;

/// Convert a raw pixel-space wheel/trackpad delta into this crate's
/// "~[`UNITS_PER_NOTCH`] units per notch" `ShellEvent::Scroll` contract.
pub fn pixels_to_notch_units(pixels: f64) -> f64 {
    pixels * (UNITS_PER_NOTCH / PIXELS_PER_NOTCH)
}

/// Window resize direction (mirrors winit::window::ResizeDirection).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResizeDir {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}
