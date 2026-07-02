//! `DrawingSession` — thin trait abstracting over the per-bubble drawing
//! manager for hit-test / freehand interaction helpers.
//!
//! `DrawingManager` physically lives in `mlc-backend` (Step 3.1 — the
//! type dictionary doesn't hold managers). Interaction helpers that need
//! drawing-session behavior (`mlc-app::panel_grid::ChartPanelGrid`,
//! `mlc-core::state::toolbar_state`) live at different layers and neither
//! can depend on `mlc-backend` directly (would invert the dep graph) or on
//! each other in the wrong direction. This trait is the narrow interface
//! both need; `mlc-app` implements it as a thin per-bubble wrapper around
//! `mlc_backend::drawing::DrawingsMut` (see
//! `mlc-app/src/input/drawing_session.rs`).
//!
//! All methods operate on "the active bubble" implicitly — the implementor
//! closes over `bid` — so the trait surface mirrors what a bubble-scoped
//! `BubbleDrawingSession` view would expose, minus the bubble parameter.

pub trait DrawingSession {
    fn is_freehand_tool(&self) -> bool;
    fn is_drawing(&self) -> bool;
    fn current_pane(&self) -> Option<u64>;
    fn set_current_pane(&mut self, pane_id: Option<u64>);
    fn start_freehand(&mut self, ts_ms: i64, price: f64) -> bool;
    fn add_freehand_point(&mut self, ts_ms: i64, price: f64, bar_interval_ms: i64) -> bool;
    fn complete_freehand(&mut self) -> bool;
    fn selected(&self) -> Option<u64>;
    fn last_index(&self) -> Option<u64>;
    fn get_type_id(&self, prim_id: u64) -> Option<String>;
    fn get_points(&self, prim_id: u64) -> Option<Vec<(i64, f64)>>;
    fn get_data(&self, prim_id: u64) -> Option<crate::drawing::PrimitiveData>;
    fn pane_id_of(&self, prim_id: u64) -> Option<u64>;
    fn toggle_lock(&mut self);
    fn toggle_visible(&mut self);
    fn is_visible(&self) -> bool;
    fn set_tool(&mut self, tool_id: Option<&str>);
    fn delete_selected(&mut self) -> bool;
    fn delete_all(&mut self);

    #[allow(clippy::too_many_arguments)]
    fn hit_test(
        &self,
        x: f64,
        y: f64,
        bars: &[crate::Bar],
        viewport: &crate::Viewport,
        price_scale: &crate::PriceScale,
    ) -> Option<u64>;

    #[allow(clippy::too_many_arguments)]
    fn hit_test_in_pane(
        &self,
        x: f64,
        y: f64,
        pane_id: u64,
        bars: &[crate::Bar],
        viewport: &crate::Viewport,
        price_scale: &crate::PriceScale,
    ) -> Option<u64>;

    fn hit_test_control_point(
        &self,
        x: f64,
        y: f64,
        bars: &[crate::Bar],
        viewport: &crate::Viewport,
        price_scale: &crate::PriceScale,
    ) -> Option<crate::drawing::ControlPointType>;

    #[allow(clippy::too_many_arguments)]
    fn hit_test_control_point_in_pane(
        &self,
        x: f64,
        y: f64,
        pane_id: u64,
        bars: &[crate::Bar],
        viewport: &crate::Viewport,
        price_scale: &crate::PriceScale,
    ) -> Option<crate::drawing::ControlPointType>;
}
