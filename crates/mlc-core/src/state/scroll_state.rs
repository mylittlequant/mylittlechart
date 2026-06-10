//! Scroll state for scrollable containers (pure data, no rendering dependencies)

/// Scroll state for a scrollable container
///
/// Include this in your modal/widget state to enable scrolling.
#[derive(Clone, Debug, Default)]
pub struct ScrollState {
    /// Current scroll offset (pixels from top)
    pub offset: f64,
    /// Is scrollbar handle being dragged?
    pub is_dragging: bool,
    /// Y position where drag started
    pub drag_start_y: Option<f64>,
    /// Scroll offset when drag started
    pub drag_start_offset: Option<f64>,
}

impl ScrollState {
    /// Create new scroll state
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset scroll state (e.g., when content changes)
    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.is_dragging = false;
        self.drag_start_y = None;
        self.drag_start_offset = None;
    }

    /// Start scrollbar drag
    pub fn start_drag(&mut self, y: f64) {
        self.is_dragging = true;
        self.drag_start_y = Some(y);
        self.drag_start_offset = Some(self.offset);
    }

    /// End scrollbar drag
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_start_y = None;
        self.drag_start_offset = None;
    }

    /// Handle mouse wheel scroll
    ///
    /// Returns true if scroll was handled (content overflows)
    pub fn handle_wheel(&mut self, delta_y: f64, content_height: f64, viewport_height: f64) -> bool {
        if content_height <= viewport_height {
            return false;
        }
        let max_scroll = (content_height - viewport_height).max(0.0);
        let scroll_step = 10.0; // pixels per scroll tick (delta_y is pre-multiplied ~20x by platform)
        self.offset = (self.offset + delta_y * scroll_step).clamp(0.0, max_scroll);
        true
    }

    /// Handle scrollbar drag motion
    ///
    /// Call this in on_mouse_move when is_dragging is true
    pub fn handle_drag(&mut self, y: f64, track_height: f64, content_height: f64, viewport_height: f64) {
        if !self.is_dragging {
            return;
        }

        let Some(start_y) = self.drag_start_y else { return };
        let Some(start_offset) = self.drag_start_offset else { return };

        let max_scroll = (content_height - viewport_height).max(0.0);
        if max_scroll <= 0.0 {
            return;
        }

        let handle_height = (viewport_height / content_height * track_height).max(20.0);
        let scroll_range = track_height - handle_height;
        if scroll_range <= 0.0 {
            return;
        }

        let dy = y - start_y;
        let scroll_delta = dy / scroll_range * max_scroll;
        self.offset = (start_offset + scroll_delta).clamp(0.0, max_scroll);
    }

    /// Scroll to position
    pub fn scroll_to(&mut self, offset: f64, content_height: f64, viewport_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        self.offset = offset.clamp(0.0, max_scroll);
    }

    /// Ensure item is visible (scroll if needed)
    pub fn ensure_visible(&mut self, item_y: f64, item_height: f64, viewport_height: f64, content_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        // If item top is above viewport
        if item_y < self.offset {
            self.offset = item_y.max(0.0);
        }
        // If item bottom is below viewport
        else if item_y + item_height > self.offset + viewport_height {
            self.offset = (item_y + item_height - viewport_height).clamp(0.0, max_scroll);
        }
    }

    /// Handle click on scrollbar track (jump to position)
    pub fn handle_track_click(&mut self, click_y: f64, track_y: f64, track_height: f64, content_height: f64, viewport_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        if max_scroll <= 0.0 {
            return;
        }
        let relative_y = (click_y - track_y) / track_height;
        self.offset = (relative_y * max_scroll).clamp(0.0, max_scroll);
    }

    /// Clamp offset to valid range
    pub fn clamp(&mut self, content_height: f64, viewport_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        self.offset = self.offset.clamp(0.0, max_scroll);
    }
}
