//! `Drawings` / `DrawingsMut` — the only doctrinally-sanctioned access path
//! to `DrawingManager` from outside `mlc-backend`.
//!
//! Each facade struct holds both the `DrawingManager` guard and the
//! `BubbleStore` guard for the duration of the call chain. App code never
//! sees `DataBubble` or the raw `DrawingManager` — it calls
//! `core.drawings().X(...)` / `core.drawings_mut().X(...)`.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::drawing::primitives_v2::config::{
    FibLevelConfig, PropertyKind, PropertyValue, TemplateStyle,
    TimeframeVisibilityConfig,
};
use mlc_core::drawing::primitives_v2::{
    ControlPoint, ControlPointType, LineStyle, Primitive, PrimitiveData, SyncMode, TextAlign,
};
use mlc_core::drawing::PrimitiveSnapshot;
use mlc_core::ids::BubbleId;
use mlc_core::state::selected_config::SelectedPrimitiveConfig;
use mlc_core::{Bar, PriceScale, Viewport};

use crate::bubble::BubbleStore;

use super::manager::{DragType, DrawingManager, DrawingState, PrimitiveListItem};

/// Result of [`DrawingsMut::swap_symbol`]. `old_primitives` is the snapshot
/// taken from the bubble's primitives *before* the clear — the caller
/// stashes it into the window's per-symbol drawing cache if needed.
/// `restored` is the count of primitives rebuilt from `restore_snaps`, if
/// any were supplied.
pub struct SwapSymbolResult {
    pub old_primitives: Vec<PrimitiveSnapshot>,
    pub restored: usize,
}

/// One bubble's drawing state, bundled for the render path (N7). Combines
/// the editing-session read surface with a direct slice of the bubble's
/// primitives — the bubble-scoped equivalent of what `Option<&DrawingManager>`
/// used to be under the per-panel model.
pub struct BubbleDrawingView<'a> {
    mgr: &'a DrawingManager,
    bubbles: &'a BubbleStore,
    bid: BubbleId,
    primitives: &'a [Box<dyn Primitive>],
}

impl<'a> BubbleDrawingView<'a> {
    pub fn count(&self) -> usize {
        self.primitives.len()
    }

    pub fn primitives(&self) -> &'a [Box<dyn Primitive>] {
        self.primitives
    }

    pub fn current_tool(&self) -> Option<&'a str> {
        self.mgr.current_tool(self.bid)
    }

    pub fn tool_variant(&self) -> Option<&'a str> {
        self.mgr.tool_variant(self.bid)
    }

    pub fn is_locked(&self) -> bool {
        self.mgr.is_locked(self.bid)
    }

    pub fn is_visible(&self) -> bool {
        self.mgr.is_visible(self.bid)
    }

    pub fn is_drawing(&self) -> bool {
        self.mgr.is_drawing(self.bid)
    }

    pub fn is_drawing_freehand(&self) -> bool {
        self.mgr.is_drawing_freehand(self.bid)
    }

    pub fn drawing_state(&self) -> DrawingState {
        self.mgr.drawing_state(self.bid)
    }

    pub fn current_pane(&self) -> Option<u64> {
        self.mgr.current_pane(self.bid)
    }

    pub fn drawing_points(&self) -> Vec<(i64, f64)> {
        self.mgr.drawing_points(self.bid)
    }

    pub fn selected(&self) -> Option<u64> {
        self.mgr.selected(self.bid)
    }

    pub fn dragging(&self) -> Option<u64> {
        self.mgr.dragging(self.bid)
    }

    pub fn create_preview(&self, cursor_ts_ms: i64, cursor_price: f64) -> Option<Box<dyn Primitive>> {
        self.mgr.create_preview(self.bubbles, self.bid, cursor_ts_ms, cursor_price)
    }

    pub fn primitives_sorted_by_z_order(&self) -> Vec<usize> {
        self.mgr.primitives_sorted_by_z_order(self.bubbles, self.bid)
    }

    pub fn drawing_tool_id(&self) -> Option<String> {
        self.mgr.drawing_tool_id(self.bid)
    }

    pub fn effective_color(&self) -> String {
        self.mgr.effective_color(self.bubbles, self.bid)
    }
}

impl<'a> mlc_core::drawing::DrawingRenderView for BubbleDrawingView<'a> {
    fn primitives(&self) -> &[Box<dyn Primitive>] {
        self.primitives
    }
    fn primitives_sorted_by_z_order(&self) -> Vec<usize> {
        BubbleDrawingView::primitives_sorted_by_z_order(self)
    }
    fn selected(&self) -> Option<u64> {
        BubbleDrawingView::selected(self)
    }
    fn is_visible(&self) -> bool {
        BubbleDrawingView::is_visible(self)
    }
    fn is_drawing(&self) -> bool {
        BubbleDrawingView::is_drawing(self)
    }
    fn is_drawing_freehand(&self) -> bool {
        BubbleDrawingView::is_drawing_freehand(self)
    }
    fn current_pane(&self) -> Option<u64> {
        BubbleDrawingView::current_pane(self)
    }
    fn drawing_points(&self) -> Vec<(i64, f64)> {
        BubbleDrawingView::drawing_points(self)
    }
    fn drawing_tool_id(&self) -> Option<String> {
        BubbleDrawingView::drawing_tool_id(self)
    }
    fn effective_color(&self) -> String {
        BubbleDrawingView::effective_color(self)
    }
    fn create_preview(&self, cursor_ts_ms: i64, cursor_price: f64) -> Option<Box<dyn Primitive>> {
        BubbleDrawingView::create_preview(self, cursor_ts_ms, cursor_price)
    }
}

/// Read-only facade over `DrawingManager` + `BubbleStore`.
pub struct Drawings<'a> {
    mgr: RwLockReadGuard<'a, DrawingManager>,
    bubbles: RwLockReadGuard<'a, BubbleStore>,
}

impl<'a> Drawings<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, DrawingManager>, bubbles: RwLockReadGuard<'a, BubbleStore>) -> Self {
        Self { mgr, bubbles }
    }

    /// Bundled read view of one bubble's drawing state — render-path (N7)
    /// entry point. Reuse one `Drawings` guard across an N-window render
    /// loop and call `with_bubble(bid)` per window.
    pub fn with_bubble(&self, bid: BubbleId) -> BubbleDrawingView<'_> {
        BubbleDrawingView {
            mgr: &self.mgr,
            bubbles: &self.bubbles,
            bid,
            primitives: self.mgr.primitives(&self.bubbles, bid),
        }
    }

    pub fn primitives(&self, bid: BubbleId) -> &[Box<dyn Primitive>] {
        self.mgr.primitives(&self.bubbles, bid)
    }

    pub fn count(&self, bid: BubbleId) -> usize {
        self.mgr.count(&self.bubbles, bid)
    }

    pub fn get_data(&self, bid: BubbleId, prim_id: u64) -> Option<PrimitiveData> {
        self.mgr.get_data(&self.bubbles, bid, prim_id)
    }

    pub fn pane_id_of(&self, bid: BubbleId, prim_id: u64) -> Option<u64> {
        self.mgr.pane_id_of(&self.bubbles, bid, prim_id)
    }

    pub fn get_points(&self, bid: BubbleId, prim_id: u64) -> Option<Vec<(i64, f64)>> {
        self.mgr.get_points(&self.bubbles, bid, prim_id)
    }

    pub fn get_type_id(&self, bid: BubbleId, prim_id: u64) -> Option<String> {
        self.mgr.get_type_id(&self.bubbles, bid, prim_id)
    }

    pub fn get_primitive_json(&self, bid: BubbleId, prim_id: u64) -> Option<String> {
        self.mgr.get_primitive_json(&self.bubbles, bid, prim_id)
    }

    pub fn is_drawing(&self, bid: BubbleId) -> bool {
        self.mgr.is_drawing(bid)
    }

    pub fn is_drawing_freehand(&self, bid: BubbleId) -> bool {
        self.mgr.is_drawing_freehand(bid)
    }

    pub fn is_freehand_tool(&self, bid: BubbleId) -> bool {
        self.mgr.is_freehand_tool(bid)
    }

    pub fn current_tool(&self, bid: BubbleId) -> Option<&str> {
        self.mgr.current_tool(bid)
    }

    pub fn tool_variant(&self, bid: BubbleId) -> Option<&str> {
        self.mgr.tool_variant(bid)
    }

    pub fn drawing_tool_id(&self, bid: BubbleId) -> Option<String> {
        self.mgr.drawing_tool_id(bid)
    }

    pub fn current_pane(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.current_pane(bid)
    }

    pub fn last_index(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.last_index(&self.bubbles, bid)
    }

    pub fn is_dragging(&self, bid: BubbleId) -> bool {
        self.mgr.is_dragging(bid)
    }

    pub fn dragging(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.dragging(bid)
    }

    pub fn drag_type(&self, bid: BubbleId) -> DragType {
        self.mgr.drag_type(bid)
    }

    pub fn selected(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.selected(bid)
    }

    pub fn selected_primitive(&self, bid: BubbleId) -> Option<&Box<dyn Primitive>> {
        self.mgr.selected_primitive(&self.bubbles, bid)
    }

    pub fn selected_config(&self, bid: BubbleId) -> Option<SelectedPrimitiveConfig> {
        self.mgr.selected_config(&self.bubbles, bid)
    }

    pub fn selected_control_points(&self, bid: BubbleId, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Vec<ControlPoint> {
        self.mgr.selected_control_points(&self.bubbles, bid, bars, viewport, price_scale)
    }

    pub fn primitive_list(&self, bid: BubbleId) -> Vec<PrimitiveListItem> {
        self.mgr.primitive_list(&self.bubbles, bid)
    }

    pub fn is_locked(&self, bid: BubbleId) -> bool {
        self.mgr.is_locked(bid)
    }

    pub fn is_visible(&self, bid: BubbleId) -> bool {
        self.mgr.is_visible(bid)
    }

    pub fn default_color(&self, bid: BubbleId) -> String {
        self.mgr.default_color(bid)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hit_test(&self, bid: BubbleId, x: f64, y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<u64> {
        self.mgr.hit_test(&self.bubbles, bid, x, y, bars, viewport, price_scale)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hit_test_in_pane(&self, bid: BubbleId, x: f64, y: f64, pane_id: u64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<u64> {
        self.mgr.hit_test_in_pane(&self.bubbles, bid, x, y, pane_id, bars, viewport, price_scale)
    }

    pub fn hit_test_control_point(&self, bid: BubbleId, x: f64, y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<ControlPointType> {
        self.mgr.hit_test_control_point(&self.bubbles, bid, x, y, bars, viewport, price_scale)
    }

    pub fn create_preview(&self, bid: BubbleId, cursor_ts_ms: i64, cursor_price: f64) -> Option<Box<dyn Primitive>> {
        self.mgr.create_preview(&self.bubbles, bid, cursor_ts_ms, cursor_price)
    }

    pub fn last_style_for(&self, bid: BubbleId, tool_id: &str) -> Option<TemplateStyle> {
        self.mgr.last_style_for(&self.bubbles, bid, tool_id)
    }
}

/// Mutable facade over `DrawingManager` + `BubbleStore`.
pub struct DrawingsMut<'a> {
    mgr: RwLockWriteGuard<'a, DrawingManager>,
    bubbles: RwLockWriteGuard<'a, BubbleStore>,
}

impl<'a> DrawingsMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, DrawingManager>, bubbles: RwLockWriteGuard<'a, BubbleStore>) -> Self {
        Self { mgr, bubbles }
    }

    // ---- Read passthrough (mirrors `Drawings`, for write-holders that
    // also need to read without re-acquiring) -------------------------------

    pub fn primitives(&self, bid: BubbleId) -> &[Box<dyn Primitive>] {
        self.mgr.primitives(&self.bubbles, bid)
    }

    pub fn count(&self, bid: BubbleId) -> usize {
        self.mgr.count(&self.bubbles, bid)
    }

    pub fn get_data(&self, bid: BubbleId, prim_id: u64) -> Option<PrimitiveData> {
        self.mgr.get_data(&self.bubbles, bid, prim_id)
    }

    pub fn pane_id_of(&self, bid: BubbleId, prim_id: u64) -> Option<u64> {
        self.mgr.pane_id_of(&self.bubbles, bid, prim_id)
    }

    pub fn is_drawing(&self, bid: BubbleId) -> bool {
        self.mgr.is_drawing(bid)
    }

    pub fn is_freehand_tool(&self, bid: BubbleId) -> bool {
        self.mgr.is_freehand_tool(bid)
    }

    pub fn current_tool(&self, bid: BubbleId) -> Option<&str> {
        self.mgr.current_tool(bid)
    }

    pub fn current_pane(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.current_pane(bid)
    }

    pub fn selected(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.selected(bid)
    }

    pub fn selected_primitive(&self, bid: BubbleId) -> Option<&Box<dyn Primitive>> {
        self.mgr.selected_primitive(&self.bubbles, bid)
    }

    pub fn dragging(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.dragging(bid)
    }

    pub fn is_dragging(&self, bid: BubbleId) -> bool {
        self.mgr.is_dragging(bid)
    }

    pub fn last_index(&self, bid: BubbleId) -> Option<u64> {
        self.mgr.last_index(&self.bubbles, bid)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hit_test(&self, bid: BubbleId, x: f64, y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<u64> {
        self.mgr.hit_test(&self.bubbles, bid, x, y, bars, viewport, price_scale)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hit_test_in_pane(&self, bid: BubbleId, x: f64, y: f64, pane_id: u64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<u64> {
        self.mgr.hit_test_in_pane(&self.bubbles, bid, x, y, pane_id, bars, viewport, price_scale)
    }

    pub fn hit_test_control_point(&self, bid: BubbleId, x: f64, y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<ControlPointType> {
        self.mgr.hit_test_control_point(&self.bubbles, bid, x, y, bars, viewport, price_scale)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn hit_test_control_point_in_pane(&self, bid: BubbleId, x: f64, y: f64, pane_id: u64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> Option<ControlPointType> {
        self.mgr.hit_test_control_point_in_pane(&self.bubbles, bid, x, y, pane_id, bars, viewport, price_scale)
    }

    pub fn create_preview(&self, bid: BubbleId, cursor_ts_ms: i64, cursor_price: f64) -> Option<Box<dyn Primitive>> {
        self.mgr.create_preview(&self.bubbles, bid, cursor_ts_ms, cursor_price)
    }

    // ---- Tool / pane / session -------------------------------------------

    pub fn set_tool(&mut self, bid: BubbleId, tool_id: Option<&str>) {
        self.mgr.set_tool(bid, tool_id)
    }

    pub fn set_current_pane(&mut self, bid: BubbleId, pane_id: Option<u64>) {
        self.mgr.set_current_pane(bid, pane_id)
    }

    pub fn cancel_drawing(&mut self, bid: BubbleId) {
        self.mgr.cancel_drawing(bid)
    }

    pub fn set_default_color(&mut self, bid: BubbleId, color: &str) {
        self.mgr.set_default_color(bid, color)
    }

    pub fn set_locked(&mut self, bid: BubbleId, locked: bool) {
        self.mgr.set_locked(bid, locked)
    }

    pub fn set_visible(&mut self, bid: BubbleId, visible: bool) {
        self.mgr.set_visible(bid, visible)
    }

    pub fn toggle_lock(&mut self, bid: BubbleId) {
        self.mgr.toggle_lock(bid)
    }

    pub fn toggle_visible(&mut self, bid: BubbleId) {
        self.mgr.toggle_visible(bid)
    }

    // ---- Selection ----------------------------------------------------

    pub fn select(&mut self, bid: BubbleId, prim_id: u64) {
        self.mgr.select(&self.bubbles, bid, prim_id)
    }

    pub fn deselect(&mut self, bid: BubbleId) {
        self.mgr.deselect(bid)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn try_select_at(&mut self, bid: BubbleId, x: f64, y: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) -> bool {
        self.mgr.try_select_at(&self.bubbles, bid, x, y, bars, viewport, price_scale)
    }

    // ---- Create / click -------------------------------------------------

    pub fn on_canvas_click(&mut self, bid: BubbleId, ts_ms: i64, price: f64) -> bool {
        self.mgr.on_canvas_click(&mut self.bubbles, bid, ts_ms, price)
    }

    pub fn finish_multipoint(&mut self, bid: BubbleId) -> bool {
        self.mgr.finish_multipoint(&mut self.bubbles, bid)
    }

    pub fn start_freehand(&mut self, bid: BubbleId, ts_ms: i64, price: f64) -> bool {
        self.mgr.start_freehand(bid, ts_ms, price)
    }

    pub fn add_freehand_point(&mut self, bid: BubbleId, ts_ms: i64, price: f64, bar_interval_ms: i64) -> bool {
        self.mgr.add_freehand_point(bid, ts_ms, price, bar_interval_ms)
    }

    pub fn complete_freehand(&mut self, bid: BubbleId) -> bool {
        self.mgr.complete_freehand(&mut self.bubbles, bid)
    }

    // ---- Mutation by prim_id -------------------------------------------

    pub fn translate(&mut self, bid: BubbleId, prim_id: u64, dt_ms: i64, dprice: f64) {
        self.mgr.translate(&mut self.bubbles, bid, prim_id, dt_ms, dprice)
    }

    pub fn apply_property(&mut self, bid: BubbleId, prim_id: u64, kind: PropertyKind, prop_id: &str, value: &PropertyValue) -> bool {
        self.mgr.apply_property(&mut self.bubbles, bid, prim_id, kind, prop_id, value)
    }

    pub fn set_points(&mut self, bid: BubbleId, prim_id: u64, points: &[(i64, f64)]) {
        self.mgr.set_points(&mut self.bubbles, bid, prim_id, points)
    }

    pub fn set_data(&mut self, bid: BubbleId, prim_id: u64, data: &PrimitiveData) {
        self.mgr.set_data(&mut self.bubbles, bid, prim_id, data)
    }

    pub fn set_level_configs(&mut self, bid: BubbleId, prim_id: u64, configs: Vec<FibLevelConfig>) {
        self.mgr.set_level_configs(&mut self.bubbles, bid, prim_id, configs)
    }

    pub fn set_visibility(&mut self, bid: BubbleId, prim_id: u64, visible: bool) {
        self.mgr.set_visibility(&mut self.bubbles, bid, prim_id, visible)
    }

    pub fn toggle_visibility(&mut self, bid: BubbleId, prim_id: u64) {
        self.mgr.toggle_visibility(&mut self.bubbles, bid, prim_id)
    }

    pub fn set_lock(&mut self, bid: BubbleId, prim_id: u64, locked: bool) {
        self.mgr.set_lock(&mut self.bubbles, bid, prim_id, locked)
    }

    pub fn toggle_lock_primitive(&mut self, bid: BubbleId, prim_id: u64) {
        self.mgr.toggle_lock_primitive(&mut self.bubbles, bid, prim_id)
    }

    pub fn set_color(&mut self, bid: BubbleId, prim_id: u64, color: &str) {
        self.mgr.set_color(&mut self.bubbles, bid, prim_id, color)
    }

    pub fn set_width(&mut self, bid: BubbleId, prim_id: u64, width: f64) {
        self.mgr.set_width(&mut self.bubbles, bid, prim_id, width)
    }

    pub fn set_style(&mut self, bid: BubbleId, prim_id: u64, style: LineStyle) {
        self.mgr.set_style(&mut self.bubbles, bid, prim_id, style)
    }

    pub fn set_sync_mode(&mut self, bid: BubbleId, prim_id: u64, mode: SyncMode) {
        self.mgr.set_sync_mode(&mut self.bubbles, bid, prim_id, mode)
    }

    pub fn set_text_content(&mut self, bid: BubbleId, prim_id: u64, content: &str) {
        self.mgr.set_text_content(&mut self.bubbles, bid, prim_id, content)
    }

    pub fn set_text_font_size(&mut self, bid: BubbleId, prim_id: u64, font_size: f64) {
        self.mgr.set_text_font_size(&mut self.bubbles, bid, prim_id, font_size)
    }

    pub fn set_text_bold(&mut self, bid: BubbleId, prim_id: u64, bold: bool) {
        self.mgr.set_text_bold(&mut self.bubbles, bid, prim_id, bold)
    }

    pub fn set_text_italic(&mut self, bid: BubbleId, prim_id: u64, italic: bool) {
        self.mgr.set_text_italic(&mut self.bubbles, bid, prim_id, italic)
    }

    pub fn set_text_color(&mut self, bid: BubbleId, prim_id: u64, color: &str) {
        self.mgr.set_text_color(&mut self.bubbles, bid, prim_id, color)
    }

    pub fn set_text_v_align(&mut self, bid: BubbleId, prim_id: u64, align: TextAlign) {
        self.mgr.set_text_v_align(&mut self.bubbles, bid, prim_id, align)
    }

    pub fn set_text_h_align(&mut self, bid: BubbleId, prim_id: u64, align: TextAlign) {
        self.mgr.set_text_h_align(&mut self.bubbles, bid, prim_id, align)
    }

    pub fn apply_color_by_field(&mut self, bid: BubbleId, prim_id: u64, field: &str, color: &str) {
        self.mgr.apply_color_by_field(&mut self.bubbles, bid, prim_id, field, color)
    }

    pub fn set_timeframe_visibility(&mut self, bid: BubbleId, prim_id: u64, config: TimeframeVisibilityConfig) {
        self.mgr.set_timeframe_visibility(&mut self.bubbles, bid, prim_id, config)
    }

    pub fn set_show_on_all_timeframes(&mut self, bid: BubbleId, prim_id: u64) {
        self.mgr.set_show_on_all_timeframes(&mut self.bubbles, bid, prim_id)
    }

    pub fn apply_primitive_template(&mut self, bid: BubbleId, prim_id: u64, template: &mlc_core::templates::PrimitiveTemplate) {
        self.mgr.apply_primitive_template(&mut self.bubbles, bid, prim_id, template)
    }

    // ---- Last-used style --------------------------------------------------

    pub fn save_last_style_from_data(&mut self, bid: BubbleId, data: &PrimitiveData) {
        self.mgr.save_last_style_from_data(&self.bubbles, bid, data)
    }

    pub fn save_last_style_for_primitive(&mut self, bid: BubbleId, prim_id: u64) {
        self.mgr.save_last_style_for_primitive(&self.bubbles, bid, prim_id)
    }

    pub fn load_last_styles(&mut self, bid: BubbleId, stored: &std::collections::HashMap<String, serde_json::Value>) {
        self.mgr.load_last_styles(&self.bubbles, bid, stored)
    }

    // ---- Delete -------------------------------------------------------

    pub fn delete(&mut self, bid: BubbleId, prim_id: u64) -> bool {
        self.mgr.delete(&mut self.bubbles, bid, prim_id)
    }

    pub fn delete_selected(&mut self, bid: BubbleId) -> bool {
        self.mgr.delete_selected(&mut self.bubbles, bid)
    }

    pub fn delete_all(&mut self, bid: BubbleId) {
        self.mgr.delete_all(&mut self.bubbles, bid)
    }

    pub fn clone_primitive(&mut self, bid: BubbleId, prim_id: u64) -> Option<u64> {
        self.mgr.clone_primitive(&mut self.bubbles, bid, prim_id)
    }

    pub fn clone_selected(&mut self, bid: BubbleId) -> Option<u64> {
        self.mgr.clone_selected(&mut self.bubbles, bid)
    }

    // ---- Z-order --------------------------------------------------------

    pub fn bring_to_front(&mut self, bid: BubbleId, prim_id: u64) {
        self.mgr.bring_to_front(&mut self.bubbles, bid, prim_id)
    }

    pub fn send_to_back(&mut self, bid: BubbleId, prim_id: u64) {
        self.mgr.send_to_back(&mut self.bubbles, bid, prim_id)
    }

    // ---- Drag and drop ----------------------------------------------------

    pub fn start_drag(&mut self, bid: BubbleId, prim_id: u64, ts_ms: i64, price: f64) -> bool {
        self.mgr.start_drag(&self.bubbles, bid, prim_id, ts_ms, price)
    }

    pub fn start_control_point_drag(&mut self, bid: BubbleId, prim_id: u64, control_point: ControlPointType, ts_ms: i64, price: f64) -> bool {
        self.mgr.start_control_point_drag(&self.bubbles, bid, prim_id, control_point, ts_ms, price)
    }

    pub fn update_drag(&mut self, bid: BubbleId, current_ts_ms: i64, current_price: f64) {
        self.mgr.update_drag(&mut self.bubbles, bid, current_ts_ms, current_price)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_drag_screen(&mut self, bid: BubbleId, screen_x: f64, screen_y: f64, current_ts_ms: i64, current_price: f64, bars: &[Bar], viewport: &Viewport, price_scale: &PriceScale) {
        self.mgr.update_drag_screen(&mut self.bubbles, bid, screen_x, screen_y, current_ts_ms, current_price, bars, viewport, price_scale)
    }

    pub fn end_drag(&mut self, bid: BubbleId) {
        self.mgr.end_drag(bid)
    }

    // ---- External insertion / undo-redo --------------------------------

    pub fn add_external(&mut self, bid: BubbleId, prim: Box<dyn Primitive>) -> u64 {
        self.mgr.add_external(&mut self.bubbles, bid, prim)
    }

    pub fn insert_at(&mut self, bid: BubbleId, index: usize, type_id: &str, points: &[(i64, f64)], data: &PrimitiveData) -> Option<u64> {
        self.mgr.insert_at(&mut self.bubbles, bid, index, type_id, points, data)
    }

    pub fn replace_primitive_from_json(&mut self, bid: BubbleId, prim_id: u64, type_id: &str, json: &str) -> bool {
        self.mgr.replace_primitive_from_json(&mut self.bubbles, bid, prim_id, type_id, json)
    }

    // ---- Symbol swap (Audit-A N6) --------------------------------------

    pub fn swap_symbol(&mut self, bid: BubbleId, address: &mlc_core::instrument::InstrumentAddress, restore_snaps: Option<&[PrimitiveSnapshot]>) -> SwapSymbolResult {
        self.mgr.swap_symbol(&mut self.bubbles, bid, address, restore_snaps)
    }

    // ---- Cross-bubble adoption (desync) --------------------------------

    pub fn adopt(&mut self, bid: BubbleId, prims: Vec<Box<dyn Primitive>>) {
        self.mgr.adopt(&mut self.bubbles, bid, prims)
    }

    // ---- Legacy timestamp migration -------------------------------------

    pub fn migrate_legacy_timestamps(&mut self, bid: BubbleId) {
        self.mgr.migrate_legacy_timestamps(&mut self.bubbles, bid)
    }

    // ---- Bulk cross-bubble maintenance -----------------------------------

    /// Walk every bubble's primitives, deduplicate primitive IDs globally
    /// (replaces the `hydration_impl.rs` `bubbles_mut()` iterator use).
    /// Returns the count of renumbered primitives.
    pub fn dedup_primitive_ids(&mut self) -> u32 {
        self.mgr.dedup_primitive_ids(&mut self.bubbles)
    }

    // ---- Test-harness lifecycle -----------------------------------------

    pub fn drop_bubble(&mut self, bid: BubbleId) {
        self.mgr.drop_bubble(bid)
    }

    /// Remove and return only the editing sessions for `bubble_ids` as a
    /// fresh, small `DrawingManager`. Used exclusively by preset tab park
    /// (shared-BackendCore arc §5.3) — replaces whole-manager `swap()`,
    /// which under a shared core would drop every other window's drawing
    /// sessions too.
    pub fn extract_for_bubbles(&mut self, bubble_ids: &[BubbleId]) -> DrawingManager {
        self.mgr.extract_for_bubbles(bubble_ids)
    }

    /// Re-insert every editing session from `other`. Used exclusively by
    /// preset tab unpark.
    pub fn merge_for_bubbles(&mut self, other: DrawingManager) {
        self.mgr.merge_for_bubbles(other)
    }
}
