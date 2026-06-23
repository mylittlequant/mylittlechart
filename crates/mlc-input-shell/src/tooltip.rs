//! Tooltip state management (inlined from uzor 1.0.43 — removed in 1.1.0).

use uzor::WidgetId;

/// Request to show a tooltip.
#[derive(Clone, Debug)]
pub struct TooltipRequest {
    /// Tooltip text content.
    pub text: String,
    /// Position to show tooltip (usually near cursor).
    pub position: (f64, f64),
}

/// Manages tooltip display state including fade-in opacity.
#[derive(Clone, Debug, Default)]
pub struct TooltipState {
    active: Option<TooltipRequest>,
    hovered_widget: Option<WidgetId>,
    hover_start: f64,
    show_delay_ms: f64,
    visible: bool,
    visible_start: f64,
    fade_in_duration_ms: f64,
    fade_opacity: f64,
}

impl TooltipState {
    /// Create with default 500ms delay and 150ms fade.
    pub fn new() -> Self {
        Self {
            active: None,
            hovered_widget: None,
            hover_start: 0.0,
            show_delay_ms: 500.0,
            visible: false,
            visible_start: 0.0,
            fade_in_duration_ms: 150.0,
            fade_opacity: 0.0,
        }
    }

    /// Create with a custom show delay.
    pub fn with_delay(delay_ms: f64) -> Self {
        Self {
            show_delay_ms: delay_ms,
            ..Self::new()
        }
    }

    /// Update state based on currently hovered widget. Call each frame.
    pub fn update(&mut self, hovered_widget: Option<WidgetId>, time: f64) {
        match (&self.hovered_widget, &hovered_widget) {
            (Some(old_id), Some(new_id)) if old_id != new_id => {
                self.hovered_widget = Some(new_id.clone());
                self.hover_start = time;
                self.visible = false;
                self.visible_start = 0.0;
                self.fade_opacity = 0.0;
                self.active = None;
            }
            (None, Some(new_id)) => {
                self.hovered_widget = Some(new_id.clone());
                self.hover_start = time;
                self.visible = false;
                self.visible_start = 0.0;
                self.fade_opacity = 0.0;
            }
            (Some(_), None) => {
                self.hovered_widget = None;
                self.visible = false;
                self.visible_start = 0.0;
                self.fade_opacity = 0.0;
                self.active = None;
            }
            _ => {}
        }

        if self.hovered_widget.is_some() && !self.visible && self.should_show(time) {
            self.visible = true;
            self.visible_start = time;
        }

        if self.visible {
            self.fade_opacity = fade_opacity(time - self.visible_start, self.fade_in_duration_ms);
        }
    }

    /// Register a tooltip request for a widget. Call each frame while hovered.
    pub fn request_tooltip(
        &mut self,
        widget_id: WidgetId,
        text: String,
        pos: (f64, f64),
        time: f64,
    ) {
        if let Some(ref hovered) = self.hovered_widget {
            if hovered == &widget_id {
                self.active = Some(TooltipRequest {
                    text,
                    position: pos,
                });
                if !self.visible && self.should_show(time) {
                    self.visible = true;
                    self.visible_start = time;
                }
                if self.visible {
                    self.fade_opacity =
                        fade_opacity(time - self.visible_start, self.fade_in_duration_ms);
                }
            }
        }
    }

    /// Whether enough time has elapsed to show the tooltip.
    pub fn should_show(&self, time: f64) -> bool {
        self.hovered_widget.is_some() && (time - self.hover_start) >= self.show_delay_ms
    }

    /// Get the active tooltip request if visible.
    pub fn get_active(&self) -> Option<&TooltipRequest> {
        if self.visible { self.active.as_ref() } else { None }
    }

    /// Clear and hide the tooltip.
    pub fn clear(&mut self) {
        self.visible = false;
        self.visible_start = 0.0;
        self.fade_opacity = 0.0;
        self.active = None;
        self.hovered_widget = None;
    }

    /// Whether the tooltip is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// The widget currently being hovered.
    pub fn hovered_widget(&self) -> Option<&WidgetId> {
        self.hovered_widget.as_ref()
    }

    /// Current fade opacity in [0.0, 1.0].
    pub fn get_opacity(&self) -> f64 {
        if self.visible { self.fade_opacity } else { 0.0 }
    }
}

fn fade_opacity(elapsed_ms: f64, fade_duration_ms: f64) -> f64 {
    if fade_duration_ms <= 0.0 || elapsed_ms >= fade_duration_ms {
        1.0
    } else {
        elapsed_ms / fade_duration_ms
    }
}

/// Calculate tooltip position near cursor, avoiding screen edges.
pub fn calculate_tooltip_position(
    cursor: (f64, f64),
    tooltip_size: (f64, f64),
    screen_size: (f64, f64),
    offset: (f64, f64),
) -> (f64, f64) {
    let mut x = cursor.0 + offset.0;
    let mut y = cursor.1 + offset.1;

    if x + tooltip_size.0 > screen_size.0 {
        x = cursor.0 - tooltip_size.0 - offset.0;
        if x < 0.0 {
            x = screen_size.0 - tooltip_size.0;
        }
    }
    if x < 0.0 {
        x = 0.0;
    }

    if y + tooltip_size.1 > screen_size.1 {
        y = cursor.1 - tooltip_size.1 - offset.1;
        if y < 0.0 {
            y = screen_size.1 - tooltip_size.1;
        }
    }
    if y < 0.0 {
        y = 0.0;
    }

    (x, y)
}
