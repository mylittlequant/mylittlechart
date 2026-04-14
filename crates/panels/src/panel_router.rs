//! Routing helpers for dispatching render and input to encapsulated panels.
//!
//! These are thin utilities called by `chart-app`. During the incremental
//! migration, callers fall through to old code when panels return `None`.

use crate::panel_theme::PanelTheme;
use crate::panel_trait::TradingPanel;
use crate::render::RenderContext;

/// Try to render a panel via `TradingPanel::render`.
///
/// Returns `true` if the panel was rendered (migrated), `false` otherwise.
pub fn try_render(
    panel: &dyn TradingPanel,
    ctx: &mut dyn RenderContext,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    theme: &PanelTheme,
) -> bool {
    panel.render(ctx, x, y, w, h, theme);
    true
}

/// Try to route a click to a panel.
///
/// `widget_id` is the full widget ID string. If it starts with
/// `"{panel.kind()}:"`, the prefix is stripped and `handle_click` is called.
/// Returns `true` if the event was consumed.
pub fn try_route_click(
    panel: &mut dyn TradingPanel,
    widget_id: &str,
    x: f64,
    y: f64,
) -> bool {
    let prefix = panel.kind();
    if let Some(local_id) = widget_id.strip_prefix(prefix) {
        // Strip the separator colon after prefix
        let local_id = local_id.strip_prefix(':').unwrap_or(local_id);
        panel.handle_click(local_id, x, y)
    } else {
        false
    }
}
