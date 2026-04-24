//! Encapsulation trait for trading panels.
//!
//! Each panel state struct (DomState, FootprintState, etc.) implements
//! `TradingPanel` to co-locate render and input handling with state.
//! Panels do NOT know their own ID — the docking system assigns IDs
//! from above.

use crate::render::RenderContext;

/// The encapsulation contract for a trading panel.
///
/// Each of the 11 panel state structs implements this trait in its own file,
/// migrated incrementally from the monolithic `panel_renderers_orderflow.rs`.
pub trait TradingPanel {
    /// Short stable identifier for this panel kind, e.g. `"dom"`, `"footprint"`.
    ///
    /// Used by the router to match widget ID prefixes and dispatch events.
    /// Must be unique across all 11 panel types.
    fn kind(&self) -> &'static str;

    /// Human-readable label for UI display, e.g. `"DOM"`, `"Footprint"`.
    fn label(&self) -> &'static str;

    /// Render the panel content into the given rect.
    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    );

    /// Handle a click on a widget belonging to this panel.
    ///
    /// `local_id` is the widget ID with the panel prefix already stripped
    /// (e.g. if full widget_id was `"dom:center"`, local_id is `"center"`).
    ///
    /// Returns `true` if the event was consumed.
    fn handle_click(
        &mut self,
        local_id: &str,
        x: f64,
        y: f64,
    ) -> bool;
}
