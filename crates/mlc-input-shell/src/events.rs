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
    /// Pre-normalized: LineDelta already multiplied by 20.0 by the caller.
    Scroll { dx: f64, dy: f64 },
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
    KeyY,
    KeyZ,
    Other,
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
