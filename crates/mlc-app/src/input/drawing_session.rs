//! `DrawingSessionView` — implements `mlc_core::state::DrawingSession` for
//! a `(bid, &mut DrawingsMut)` pair.
//!
//! `mlc_core::state::ChartPanelGrid::{handle_right_click, handle_drag_start,
//! update_crosshair, extend_freehand, complete_freehand}` and
//! `mlc_core::state::toolbar_state::ToolbarState::{
//! handle_toolbar_click_with_chart, handle_dropdown_select_with_chart}` are
//! generic over `impl DrawingSession` because `mlc-core` cannot depend on
//! `mlc-backend-core` (the reverse dependency already exists). This is the
//! `mlc-app`-side implementor those callers pass in — a thin adapter over
//! the bid-scoped facade, TRANSITIONAL until Step 4/5 moves these callers
//! out of `mlc-core::state` entirely (see
//! `docs/mlc/plans/step-3.1-drawing-manager-design.md` §2 "6 N3 external-fn
//! callers").

use mlc_backend_core::drawing::DrawingsMut;
use mlc_core::drawing::primitives_v2::ControlPointType;
use mlc_core::drawing::PrimitiveData;
use mlc_core::ids::BubbleId;
use mlc_core::state::DrawingSession;
use mlc_core::{Bar, PriceScale, Viewport};

pub struct DrawingSessionView<'a, 'b> {
    pub facade: &'a mut DrawingsMut<'b>,
    pub bid: BubbleId,
}

impl<'a, 'b> DrawingSessionView<'a, 'b> {
    pub fn new(facade: &'a mut DrawingsMut<'b>, bid: BubbleId) -> Self {
        Self { facade, bid }
    }
}

impl<'a, 'b> DrawingSession for DrawingSessionView<'a, 'b> {
    fn is_freehand_tool(&self) -> bool {
        self.facade.is_freehand_tool(self.bid)
    }

    fn is_drawing(&self) -> bool {
        self.facade.is_drawing(self.bid)
    }

    fn current_pane(&self) -> Option<u64> {
        self.facade.current_pane(self.bid)
    }

    fn set_current_pane(&mut self, pane_id: Option<u64>) {
        self.facade.set_current_pane(self.bid, pane_id);
    }

    fn start_freehand(&mut self, ts_ms: i64, price: f64) -> bool {
        self.facade.start_freehand(self.bid, ts_ms, price)
    }

    fn add_freehand_point(&mut self, ts_ms: i64, price: f64, bar_interval_ms: i64) -> bool {
        self.facade.add_freehand_point(self.bid, ts_ms, price, bar_interval_ms)
    }

    fn complete_freehand(&mut self) -> bool {
        self.facade.complete_freehand(self.bid)
    }

    fn selected(&self) -> Option<u64> {
        self.facade.selected(self.bid)
    }

    fn last_index(&self) -> Option<u64> {
        self.facade.last_index(self.bid)
    }

    fn get_type_id(&self, prim_id: u64) -> Option<String> {
        self.facade.get_data(self.bid, prim_id).map(|d| d.type_id)
    }

    fn get_points(&self, prim_id: u64) -> Option<Vec<(i64, f64)>> {
        self.facade.primitives(self.bid).iter()
            .find(|p| p.data().id == prim_id)
            .map(|p| p.points())
    }

    fn get_data(&self, prim_id: u64) -> Option<PrimitiveData> {
        self.facade.get_data(self.bid, prim_id)
    }

    fn pane_id_of(&self, prim_id: u64) -> Option<u64> {
        self.facade.pane_id_of(self.bid, prim_id)
    }

    fn toggle_lock(&mut self) {
        self.facade.toggle_lock(self.bid);
    }

    fn toggle_visible(&mut self) {
        self.facade.toggle_visible(self.bid);
    }

    fn is_visible(&self) -> bool {
        // No direct read on DrawingsMut for the bubble-wide visibility flag;
        // conservatively report visible (matches the pre-reset default when
        // no explicit lock/visibility state has been set).
        true
    }

    fn set_tool(&mut self, tool_id: Option<&str>) {
        self.facade.set_tool(self.bid, tool_id);
    }

    fn delete_selected(&mut self) -> bool {
        self.facade.delete_selected(self.bid)
    }

    fn delete_all(&mut self) {
        self.facade.delete_all(self.bid);
    }

    fn hit_test(&self, x: f64, y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<u64> {
        self.facade.hit_test(self.bid, x, y, bars, viewport, price_scale)
    }

    fn hit_test_in_pane(&self, x: f64, y: f64, pane_id: u64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<u64> {
        self.facade.hit_test_in_pane(self.bid, x, y, pane_id, bars, viewport, price_scale)
    }

    fn hit_test_control_point(&self, x: f64, y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<ControlPointType> {
        self.facade.hit_test_control_point(self.bid, x, y, bars, viewport, price_scale)
    }

    fn hit_test_control_point_in_pane(&self, x: f64, y: f64, pane_id: u64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<ControlPointType> {
        self.facade.hit_test_control_point_in_pane(self.bid, x, y, pane_id, bars, viewport, price_scale)
    }
}
