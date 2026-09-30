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
use mlc_core::drawing::primitives_v2::elliott::{WaveLink, WaveSlot};
use mlc_core::drawing::primitives_v2::{
    ControlPoint, ControlPointType, LineStyle, Primitive, PrimitiveData, SyncMode, TextAlign,
};
use mlc_core::drawing::PrimitiveSnapshot;
use mlc_core::ids::BubbleId;
use mlc_core::state::selected_config::SelectedPrimitiveConfig;
use mlc_core::{Bar, PriceScale, Viewport};

use crate::bubble::BubbleStore;

use super::manager::{DeletedPrimitives, DragType, DrawingManager, DrawingState, PrimitiveListItem};

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
    /// Ephemeral primitives painted over the real ones; never stored.
    overlay: &'a [Box<dyn Primitive>],
}

impl<'a> BubbleDrawingView<'a> {
    /// Paint `overlay` on top of this bubble's primitives. The slice is
    /// owned by the caller and never enters the drawing store.
    pub fn with_overlay(mut self, overlay: &'a [Box<dyn Primitive>]) -> Self {
        self.overlay = overlay;
        self
    }

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

    pub fn shift_constrain(&self) -> bool {
        self.mgr.shift_constrain(self.bid)
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
    fn shift_constrain(&self) -> bool {
        BubbleDrawingView::shift_constrain(self)
    }
    fn render_bid(&self) -> Option<BubbleId> {
        Some(self.bid)
    }
    fn overlay_primitives(&self) -> &[Box<dyn Primitive>] {
        self.overlay
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
            overlay: &[],
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

    pub fn get_snapshot(&self, bid: BubbleId, prim_id: u64) -> Option<PrimitiveSnapshot> {
        self.mgr.get_snapshot(&self.bubbles, bid, prim_id)
    }

    /// Every `(child_id, slot)` pair currently linked under `parent_id` —
    /// see `DrawingManager::wave_children_of`'s own doc.
    pub fn wave_children_of(&self, bid: BubbleId, parent_id: u64) -> Vec<(u64, WaveSlot)> {
        self.mgr.wave_children_of(&self.bubbles, bid, parent_id)
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

    pub fn drawing_points(&self, bid: BubbleId) -> Vec<(i64, f64)> {
        self.mgr.drawing_points(bid)
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

    /// See `DrawingManager::wave_child_id`.
    pub fn wave_child_id(&self, bid: BubbleId, parent_id: u64, slot: WaveSlot) -> Option<u64> {
        self.mgr.wave_child_id(&self.bubbles, bid, parent_id, slot)
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

    pub fn drawing_points(&self, bid: BubbleId) -> Vec<(i64, f64)> {
        self.mgr.drawing_points(bid)
    }

    pub fn set_shift_constrain(&mut self, bid: BubbleId, held: bool) {
        self.mgr.set_shift_constrain(bid, held);
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

    /// Every `(child_id, slot)` pair currently linked under `parent_id` —
    /// see `DrawingManager::wave_children_of`'s own doc. The caller asks
    /// this right after discovering a just-created primitive's own id
    /// (`last_index`), to learn whether higher-degree auto-adoption fired
    /// during its own construction and, if so, build `Command::
    /// PromoteWaveChain` instead of `Command::CreatePrimitive` for it.
    pub fn wave_children_of(&self, bid: BubbleId, parent_id: u64) -> Vec<(u64, WaveSlot)> {
        self.mgr.wave_children_of(&self.bubbles, bid, parent_id)
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
        self.mgr.end_drag(&mut self.bubbles, bid)
    }

    // ---- External insertion / undo-redo --------------------------------

    pub fn add_external(&mut self, bid: BubbleId, prim: Box<dyn Primitive>) -> u64 {
        self.mgr.add_external(&mut self.bubbles, bid, prim)
    }

    /// Create a new Elliott child primitive nested into `slot` of the
    /// parent primitive `parent_id`, built as `kind_id` (E2 two-step
    /// wave-slot/wave-kind picker). See `DrawingManager::create_wave_child`
    /// for the degree/anchor/type rules; `None` mirrors `elliott::count::
    /// child_seed`'s own `None` cases, plus a `kind_id` outside
    /// `elliott::count::child_kinds(parent, slot)`.
    pub fn create_wave_child(&mut self, bid: BubbleId, parent_id: u64, slot: WaveSlot, kind_id: &str) -> Option<u64> {
        self.mgr.create_wave_child(&mut self.bubbles, bid, parent_id, slot, kind_id)
    }

    /// Create a new Elliott successor primitive continuing `prev_id` at its
    /// own degree (E3 wave-succession picker). See
    /// `DrawingManager::create_wave_successor` for the degree/anchor/type
    /// rules; `None` mirrors `elliott::count::successor_seed`'s own `None`
    /// cases, plus a `type_id` outside `elliott::count::successor_kinds(prev)`.
    /// Arm the drawing tool for the next claim after `prev_id` in its own
    /// family's narrative — see `DrawingManager::arm_markup_successor` for
    /// why this family arms a tool where Elliott creates a primitive.
    /// `false` when the grammar does not allow `type_id` after `prev_id`.
    pub fn arm_markup_successor(&mut self, bid: BubbleId, prev_id: u64, type_id: &str) -> bool {
        self.mgr.arm_markup_successor(&self.bubbles, bid, prev_id, type_id)
    }

    pub fn create_wave_successor(&mut self, bid: BubbleId, prev_id: u64, type_id: &str) -> Option<u64> {
        self.mgr.create_wave_successor(&mut self.bubbles, bid, prev_id, type_id)
    }

    /// Create a new Elliott predecessor primitive that `next_id` continues
    /// FROM. See `DrawingManager::create_wave_predecessor` for the full
    /// contract; `None` mirrors `elliott::count::predecessor_seed`'s own
    /// `None` cases, plus a `type_id` outside `elliott::count::
    /// predecessor_kinds(next)`.
    pub fn create_wave_predecessor(&mut self, bid: BubbleId, next_id: u64, type_id: &str) -> Option<(u64, PrimitiveSnapshot)> {
        self.mgr.create_wave_predecessor(&mut self.bubbles, bid, next_id, type_id)
    }

    pub fn insert_at(&mut self, bid: BubbleId, index: usize, type_id: &str, json: &str) -> Option<u64> {
        self.mgr.insert_at(&mut self.bubbles, bid, index, type_id, json)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_pasted_at(
        &mut self,
        bid: BubbleId,
        index: usize,
        type_id: &str,
        json: &str,
        points: &[(i64, f64)],
        data: &PrimitiveData,
    ) -> Option<u64> {
        self.mgr.insert_pasted_at(&mut self.bubbles, bid, index, type_id, json, points, data)
    }

    /// See `DrawingManager::delete_with_snapshot`.
    pub fn delete_with_snapshot(&mut self, bid: BubbleId, prim_id: u64) -> Option<DeletedPrimitives> {
        self.mgr.delete_with_snapshot(&mut self.bubbles, bid, prim_id)
    }

    /// See `DrawingManager::set_wave_link`.
    pub fn set_wave_link(&mut self, bid: BubbleId, prim_id: u64, link: Option<WaveLink>) {
        self.mgr.set_wave_link(&mut self.bubbles, bid, prim_id, link)
    }

    /// See `DrawingManager::set_wave_follows`.
    pub fn set_wave_follows(&mut self, bid: BubbleId, prim_id: u64, follows: Option<u64>) {
        self.mgr.set_wave_follows(&mut self.bubbles, bid, prim_id, follows)
    }

    /// See `DrawingManager::promote_wave_chain`.
    pub fn promote_wave_chain(&mut self, bid: BubbleId, prim_id: u64) -> Option<(u64, Vec<(u64, WaveSlot)>)> {
        self.mgr.promote_wave_chain(&mut self.bubbles, bid, prim_id)
    }

    /// Convert `prim_id` to `kind_id` in place, mid-count. See
    /// `DrawingManager::convert_primitive_kind` for the full contract;
    /// `None` mirrors every reason it refuses (not a live Elliott
    /// primitive, `kind_id` outside `elliott::count::conversion_kinds`, or
    /// `kind_id` naming the kind `prim_id` already is).
    pub fn convert_primitive_kind(&mut self, bid: BubbleId, prim_id: u64, kind_id: &str) -> Option<(PrimitiveSnapshot, PrimitiveSnapshot, Vec<(u64, Option<WaveSlot>, Option<WaveSlot>)>)> {
        self.mgr.convert_primitive_kind(&mut self.bubbles, bid, prim_id, kind_id)
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

    /// See `DrawingManager::clear_dangling_wave_links` — cold-restore
    /// integrity sweep (2026-08-14 report), called once per restored
    /// bubble at the end of `apply_preset_cold`, before the matching
    /// `Self::refresh_wave_guards` call.
    pub fn clear_dangling_wave_links(&mut self, bid: BubbleId) -> u32 {
        self.mgr.clear_dangling_wave_links(&mut self.bubbles, bid)
    }

    /// See `DrawingManager::refresh_wave_guards`. Exposed on the facade so
    /// `mlc-app`'s cold-restore sweep (`hydration_impl.rs`) can fill the
    /// guard cache for every bubble `BubbleStore::restore_from_snapshots`
    /// just rebuilt — that path never goes through `DrawingManager`, so
    /// without an explicit call here the cache stays at its
    /// freshly-deserialized default (empty) until the next live edit
    /// happens to touch the bubble (2026-08-14 report).
    /// Draw every smart-money setup the engine assembles from this
    /// bubble's own bars; returns how many claims were created. See
    /// `DrawingManager::create_proposed_chains`.
    pub fn create_proposed_chains(&mut self, bid: BubbleId) -> usize {
        self.mgr.create_proposed_chains(&mut self.bubbles, bid)
    }

    /// Re-run every derived pass over every bubble — see
    /// `DrawingManager::refresh_all_derived` for why an engine-level
    /// setting needs a store-wide refresh where a gesture needs only its
    /// own bubble.
    pub fn refresh_all_derived(&mut self) {
        self.mgr.refresh_all_derived(&mut self.bubbles)
    }

    pub fn refresh_wave_guards(&mut self, bid: BubbleId) {
        self.mgr.refresh_wave_guards(&mut self.bubbles, bid)
    }

    /// See `DrawingManager::refresh_footprint_derived` — the footprint
    /// counterpart of [`Self::refresh_all_derived`]'s own bar-derived pass,
    /// scoped to one bubble. Ordinary production writes to
    /// `DataBubble::footprint_bars` already call this via
    /// `bar_derived_cache::refresh_footprint_derived_caches` at their own
    /// write point (every trade-fold install); this is for a caller that
    /// wrote `footprint_bars` directly (`BubblesMut::with_bubble_mut`) and
    /// needs every order-flow-family primitive's own cache to catch up
    /// without waiting for the next live trade-fold event.
    pub fn refresh_footprint_derived(&mut self, bid: BubbleId) {
        self.mgr.refresh_footprint_derived(&mut self.bubbles, bid)
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
