//! Platform abstraction trait and dispatch output.

use mlc_core::CursorStyle;
use crate::events::ResizeDir;

/// Platform-neutral window services.
///
/// Implemented by `VelloPlatform` (winit) and `WebPlatform` (canvas/web-sys).
/// No-op impls are allowed on web for window management methods.
pub trait Platform {
    // ── Cursor ──────────────────────────────────────────────────────────────
    fn set_cursor(&self, style: CursorStyle);
    fn set_cursor_visible(&self, visible: bool);

    // ── Window management (no-op allowed on web) ─────────────────────────
    fn drag_window(&self);
    fn drag_resize_window(&self, dir: ResizeDir);
    fn set_minimized(&self, v: bool);
    fn set_maximized(&self, v: bool);
    fn is_maximized(&self) -> bool;
    /// Window logical size for chrome hit-test.
    fn window_size(&self) -> (u32, u32);

    // ── Clipboard (best-effort; may be no-op on wasm sync path) ─────────
    fn clipboard_write(&self, text: &str);
    fn clipboard_read(&self) -> Option<String>;

    // ── Redraw request ───────────────────────────────────────────────────
    fn request_redraw(&self);
}

/// Flags set by `process_event`, consumed by the shell's `about_to_wait`.
///
/// These survive only one event-loop iteration and must not live in `InputState`.
#[derive(Default)]
pub struct DispatchOut {
    /// Chrome X or OS-level close — shut down entire app.
    pub close_requested: bool,
    /// "Close Window" context-menu item — close this window only.
    pub close_window_requested: bool,
    /// The chrome's update control was pressed while an installed update was
    /// waiting. The shell saves and restarts into the new binary; nothing
    /// restarts without this, by design — an update announces itself and waits
    /// (owner, 2026-09-04).
    pub restart_requested: bool,
    /// "Delete Window" context-menu item — delete preset + close window.
    pub delete_window_requested: bool,
    /// New-window button clicked — spawn a sibling window.
    pub spawn_new_window: bool,
}
