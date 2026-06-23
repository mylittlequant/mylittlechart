//! InputState and DirtyFlags — per-window portable input pipeline state.

use crate::events::NormalizedModifiers;

/// Portable per-window input state (no winit types).
pub struct InputState {
    pub last_mouse_pos:          (f64, f64),
    pub mouse_pressed:           bool,
    pub drag_start_pos:          Option<(f64, f64)>,
    pub last_drag_pos:           Option<(f64, f64)>,
    pub last_click:              Option<(web_time::Instant, f64, f64)>,
    pub modifiers:               NormalizedModifiers,
    /// Portable flag; `#[cfg(target_os = "windows")]` polling consumer stays in vello shell.
    pub drawing_capture:         bool,
    pub last_sidebar_hover_row:  Option<usize>,
    pub was_minimized:           bool,
    /// Gates chrome hit filtering in skeleton mode.
    pub skeleton:                bool,
    /// Reference instant for chrome tooltip timing (ms elapsed).
    pub chrome_tooltip_start:    web_time::Instant,
    /// Chrome height in logical pixels. Default: `CHROME_HEIGHT` (32.0).
    /// Web shell sets this to 0.0 (no chrome).
    pub chrome_height:           f64,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            last_mouse_pos:         (0.0, 0.0),
            mouse_pressed:          false,
            drag_start_pos:         None,
            last_drag_pos:          None,
            last_click:             None,
            modifiers:              NormalizedModifiers::default(),
            drawing_capture:        false,
            last_sidebar_hover_row: None,
            was_minimized:          false,
            skeleton:               false,
            chrome_tooltip_start:   web_time::Instant::now(),
            chrome_height:          crate::chrome::CHROME_HEIGHT,
        }
    }
}

/// Per-window dirty flags that gate scene rebuilds.
pub struct DirtyFlags {
    pub chart_dirty:         bool,
    pub toolbar_dirty:       bool,
    pub sidebar_dirty_scene: bool,
    pub overlay_dirty:       bool,
}

impl Default for DirtyFlags {
    fn default() -> Self {
        Self {
            chart_dirty:         true,
            toolbar_dirty:       true,
            sidebar_dirty_scene: true,
            overlay_dirty:       false,
        }
    }
}

impl DirtyFlags {
    pub fn set_all(&mut self) {
        self.chart_dirty         = true;
        self.toolbar_dirty       = true;
        self.sidebar_dirty_scene = true;
        self.overlay_dirty       = true;
    }

    pub fn clear_all(&mut self) {
        self.chart_dirty         = false;
        self.toolbar_dirty       = false;
        self.sidebar_dirty_scene = false;
        self.overlay_dirty       = false;
    }
}
