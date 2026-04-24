//! Drawing state queries and coordinate conversion helpers.

use crate::ChartApp;
use zengeld_chart::ExtendedFrameLayout;

impl ChartApp {
    /// Returns `true` when the drawing manager is mid-drawing (first point placed,
    /// waiting for the user to place the next point).
    ///
    /// Used by the winit runner to call `SetCapture` so that `CursorMoved` events
    /// continue arriving even when the cursor leaves the window boundary.
    pub fn is_drawing(&self) -> bool {
        self.panel_app
            .panel_grid
            .active_window()
            .map(|w| w.drawing_manager.is_drawing())
            .unwrap_or(false)
    }

    /// Returns true when a click-based (non-freehand) drawing tool is selected.
    /// Used by the runner to always route mouse-release to `on_click` instead of
    /// `on_drag_end`, so accidental micro-drags don't swallow the click.
    pub fn has_click_drawing_tool(&self) -> bool {
        self.panel_app
            .panel_grid
            .active_window()
            .map(|w| {
                w.drawing_manager.current_tool().is_some()
                    && !w.drawing_manager.is_freehand_tool()
            })
            .unwrap_or(false)
    }

    /// Convert raw screen coordinates to data coordinates (bar, price).
    ///
    /// When `pane_id` is `Some(instance_id)` the conversion uses the sub-pane's
    /// content rect (for local_y) and its price range.  When `pane_id` is `None`
    /// the main-chart coordinate system is used.
    ///
    /// The bar index is always derived from the main chart's viewport X-axis since
    /// all panes share the same time axis.
    pub(crate) fn screen_to_data_coords(
        &self,
        screen_x: f64,
        screen_y: f64,
        pane_id: Option<u64>,
        extended: &ExtendedFrameLayout,
        chart_x: f64,
        chart_y: f64,
        chart_h: f64,
    ) -> (f64, f64) {
        // X-axis is shared across all panes — always convert using main chart origin.
        let local_x = screen_x - chart_x;

        let Some(window) = self.panel_app.panel_grid.active_window() else {
            return (screen_x, screen_y);
        };
        // Snap to bar center (matching crosshair coordinate system).
        let bar = if let Some(idx) = window.viewport.x_to_bar(local_x) {
            idx as f64
        } else {
            window.viewport.x_to_bar_f64(local_x)
        };

        let price = if let Some(instance_id) = pane_id {
            // Sub-pane: find the pane layout rect and price range.
            if let Some(pane_layout) = extended.sub_panes.iter()
                .find(|p| p.instance_id == instance_id)
            {
                let content = pane_layout.content;
                let local_y = screen_y - content.y;
                let (p_min, p_max) = window.sub_panes.iter()
                    .find(|sp| sp.instance_id == instance_id)
                    .map(|sp| (sp.price_min, sp.price_max))
                    .unwrap_or((0.0, 100.0));
                let pane_h = content.height;
                if pane_h > 0.0 {
                    p_max - (local_y / pane_h) * (p_max - p_min)
                } else {
                    p_min
                }
            } else {
                // Fallback: use main chart coordinate system.
                let local_y = screen_y - chart_y;
                window.price_scale.y_to_price(local_y, chart_h)
            }
        } else {
            // Main chart.
            let local_y = screen_y - chart_y;
            window.price_scale.y_to_price(local_y, chart_h)
        };

        (bar, price)
    }
}
