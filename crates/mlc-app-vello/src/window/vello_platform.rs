//! VelloPlatform — winit + arboard implementation of mlc_input_shell::Platform.

use std::sync::Arc;
use winit::window::{Window, CursorIcon};
use mlc_core::CursorStyle;
use mlc_input_shell::platform::Platform;
use mlc_input_shell::events::ResizeDir;

fn cursor_style_to_winit(style: CursorStyle) -> CursorIcon {
    match style {
        CursorStyle::Default    => CursorIcon::Default,
        CursorStyle::Pointer    => CursorIcon::Pointer,
        CursorStyle::Grab       => CursorIcon::Grab,
        CursorStyle::Grabbing   => CursorIcon::Grabbing,
        CursorStyle::Move       => CursorIcon::Move,
        CursorStyle::NsResize   => CursorIcon::NsResize,
        CursorStyle::EwResize   => CursorIcon::EwResize,
        CursorStyle::NeswResize => CursorIcon::NeswResize,
        CursorStyle::NwseResize => CursorIcon::NwseResize,
        CursorStyle::Crosshair  => CursorIcon::Crosshair,
        CursorStyle::NotAllowed => CursorIcon::NotAllowed,
        CursorStyle::None       => CursorIcon::Default,
    }
}

fn resize_dir_to_winit(dir: ResizeDir) -> winit::window::ResizeDirection {
    match dir {
        ResizeDir::North     => winit::window::ResizeDirection::North,
        ResizeDir::South     => winit::window::ResizeDirection::South,
        ResizeDir::East      => winit::window::ResizeDirection::East,
        ResizeDir::West      => winit::window::ResizeDirection::West,
        ResizeDir::NorthEast => winit::window::ResizeDirection::NorthEast,
        ResizeDir::NorthWest => winit::window::ResizeDirection::NorthWest,
        ResizeDir::SouthEast => winit::window::ResizeDirection::SouthEast,
        ResizeDir::SouthWest => winit::window::ResizeDirection::SouthWest,
    }
}

/// Winit + arboard platform implementation.
pub struct VelloPlatform<'a> {
    pub window: &'a Arc<Window>,
}

impl Platform for VelloPlatform<'_> {
    fn set_cursor(&self, style: CursorStyle) {
        self.window.set_cursor(cursor_style_to_winit(style));
    }

    fn set_cursor_visible(&self, visible: bool) {
        self.window.set_cursor_visible(visible);
    }

    fn drag_window(&self) {
        let _ = self.window.drag_window();
    }

    fn drag_resize_window(&self, dir: ResizeDir) {
        let _ = self.window.drag_resize_window(resize_dir_to_winit(dir));
    }

    fn set_minimized(&self, v: bool) {
        self.window.set_minimized(v);
    }

    fn set_maximized(&self, v: bool) {
        self.window.set_maximized(v);
    }

    fn is_maximized(&self) -> bool {
        self.window.is_maximized()
    }

    fn window_size(&self) -> (u32, u32) {
        let s = self.window.inner_size();
        (s.width, s.height)
    }

    fn clipboard_write(&self, text: &str) {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(text);
        }
    }

    fn clipboard_read(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

