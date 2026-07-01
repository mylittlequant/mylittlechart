//! `Indicators` / `IndicatorsMut` — the only doctrinally-sanctioned access
//! path to `IndicatorManager` from outside `mlc-backend-core`.
//!
//! Each facade struct holds both the `IndicatorManager` guard and the
//! `BubbleStore` guard for the duration of the call chain. App code never
//! sees `DataBubble` or the raw `IndicatorManager` — it calls
//! `core.indicators().X(...)` / `core.indicators_mut().X(...)`.
//!
//! `bubbles: &BubbleStore` is threaded purely for existence-validation
//! (`add`/`reassign_bubble` confirm the target bubble exists before recording
//! it on an instance) — instance data itself lives on `IndicatorManager`, not
//! on `DataBubble` (design: `docs/mlc/plans/step-3.3-indicator-watchlist-design.md`
//! §3.1).
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::ids::BubbleId;
use mlc_core::indicator_source::{
    IndicatorInfo, IndicatorRenderInstance, IndicatorSettingsData, IndicatorSource,
};
use mlc_indicators::managers::indicator_manager::{IndicatorDefinition, IndicatorInstance};
use mlc_indicators::{IndicatorManager, RecalcMode};

use crate::bubble::BubbleStore;

/// One bubble's indicator state, bundled for the render path — the
/// bubble-scoped `IndicatorSource` implementation chart rendering functions
/// consume. Mirrors Drawing's `BubbleDrawingView`
/// (`mlc_backend_core::drawing::facade`).
///
/// Constructed via [`Indicators::with_bubble`]. Replaces the deleted
/// `IndicatorManager::current_render_bubble_id` scoping cursor — the bubble
/// id is bound at construction time instead of stashed in a shared atomic
/// between a store call and a read call.
pub struct BubbleIndicatorView<'a> {
    mgr: &'a IndicatorManager,
    bid: BubbleId,
}

impl<'a> BubbleIndicatorView<'a> {
    pub(crate) fn new(mgr: &'a IndicatorManager, bid: BubbleId) -> Self {
        Self { mgr, bid }
    }
}

impl<'a> IndicatorSource for BubbleIndicatorView<'a> {
    fn get_instances_for_symbol(&self, _symbol: &str) -> Vec<IndicatorInfo> {
        self.mgr
            .get_instances_for_bubble(self.bid.0)
            .into_iter()
            .map(|inst| IndicatorInfo {
                id: inst.id,
                name: inst.name.clone(),
                pane_index: inst.pane,
                visible: inst.visible,
            })
            .collect()
    }

    fn calculate_pane_range(&self, instance_id: u64, visible_start: usize, visible_end: usize) -> Option<(f64, f64)> {
        self.mgr.calculate_pane_range(instance_id, visible_start, visible_end)
    }

    fn get_render_instances_for_symbol(&self, _symbol: &str) -> Vec<IndicatorRenderInstance> {
        self.mgr
            .get_instances_for_bubble(self.bid.0)
            .into_iter()
            .filter_map(|inst| self.mgr.build_render_instance(inst))
            .collect()
    }

    fn get_render_instance(&self, instance_id: u64) -> Option<IndicatorRenderInstance> {
        self.mgr.get_render_instance(instance_id)
    }

    fn get_settings_data(&self, instance_id: u64) -> Option<IndicatorSettingsData> {
        self.mgr.get_settings_data(instance_id)
    }
}

/// Read-only facade over `IndicatorManager`.
///
/// Unlike [`IndicatorsMut`], the read facade does not co-acquire
/// `BubbleStore` — every read-only method in the §2.1 method table operates
/// purely on manager state (no existence-validation needed for a read).
pub struct Indicators<'a> {
    mgr: RwLockReadGuard<'a, IndicatorManager>,
}

impl<'a> Indicators<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, IndicatorManager>) -> Self {
        Self { mgr }
    }

    /// Bind this facade to one bubble for the `IndicatorSource` render-path
    /// contract. Replaces the deleted `current_render_bubble_id` cursor.
    pub fn with_bubble(&self, bid: BubbleId) -> BubbleIndicatorView<'_> {
        BubbleIndicatorView::new(&self.mgr, bid)
    }

    /// All available indicator definitions (the 480+ catalog).
    pub fn definitions(&self) -> Vec<&IndicatorDefinition> {
        self.mgr.get_definitions()
    }

    /// Definition by type ID.
    pub fn definition(&self, type_id: &str) -> Option<&IndicatorDefinition> {
        self.mgr.get_definition(type_id)
    }

    /// Instance by ID.
    pub fn instance(&self, id: u64) -> Option<&IndicatorInstance> {
        self.mgr.get_instance(id)
    }

    /// Full render data for a single instance by ID. Id-keyed — no bubble
    /// scoping needed (e.g. alert-bell rendering, which already knows the
    /// specific `indicator_id` an alert is bound to).
    pub fn render_instance(&self, id: u64) -> Option<IndicatorRenderInstance> {
        self.mgr.get_render_instance(id)
    }

    /// All instances bound to a bubble.
    pub fn instances_for_bubble(&self, bid: BubbleId) -> Vec<&IndicatorInstance> {
        self.mgr.get_instances_for_bubble(bid.0)
    }

    /// All instances, across every bubble.
    pub fn all_instances(&self) -> Vec<&IndicatorInstance> {
        self.mgr.get_all_instances()
    }

    /// Total instance count across every bubble.
    pub fn count(&self) -> usize {
        self.mgr.instances_count()
    }

    /// `(last_incremental_count, last_full_count)` from the most recent
    /// recalc pass — diagnostics only.
    pub fn recalc_stats(&self) -> (u32, u32) {
        (self.mgr.last_incremental_count, self.mgr.last_full_count)
    }

    /// Current recalculation mode.
    pub fn recalc_mode(&self) -> RecalcMode {
        self.mgr.recalc_mode
    }

    /// Hit test for overlay indicators (pane == 0) bound to `bid`.
    pub fn hit_test_overlay(
        &self,
        bid: BubbleId,
        screen_x: f64,
        screen_y: f64,
        viewport: &mlc_core::Viewport,
        price_scale: &mlc_core::PriceScale,
        chart_height: f64,
        hit_distance: f64,
    ) -> Option<u64> {
        self.mgr.hit_test_overlay(screen_x, screen_y, bid.0, viewport, price_scale, chart_height, hit_distance)
    }

    /// Hit test for a sub-pane indicator instance.
    pub fn hit_test_sub_pane(
        &self,
        instance_id: u64,
        screen_x: f64,
        screen_y: f64,
        viewport: &mlc_core::Viewport,
        price_min: f64,
        price_max: f64,
        pane_height: f64,
        hit_distance: f64,
    ) -> bool {
        self.mgr.hit_test_sub_pane(instance_id, screen_x, screen_y, viewport, price_min, price_max, pane_height, hit_distance)
    }

    /// Display name for an instance (e.g. "RSI (14)").
    pub fn display_name(&self, id: u64) -> String {
        match self.mgr.get_instance(id) {
            Some(inst) => self.mgr.get_instance_display_name(inst),
            None => String::new(),
        }
    }

    /// Calculate the Y-axis range for a sub-pane instance in the visible bar range.
    pub fn calculate_pane_range(&self, id: u64, visible_start: usize, visible_end: usize) -> Option<(f64, f64)> {
        self.mgr.calculate_pane_range(id, visible_start, visible_end)
    }

    /// Pick a distinct palette color for a new overlay indicator instance.
    pub fn pick_overlay_color(&self, type_id: &str, bid: BubbleId) -> Option<String> {
        self.mgr.pick_overlay_color(type_id, bid.0)
    }
}

/// Mutable facade over `IndicatorManager` + `BubbleStore`.
pub struct IndicatorsMut<'a> {
    mgr: RwLockWriteGuard<'a, IndicatorManager>,
    bubbles: RwLockWriteGuard<'a, BubbleStore>,
}

impl<'a> IndicatorsMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, IndicatorManager>, bubbles: RwLockWriteGuard<'a, BubbleStore>) -> Self {
        Self { mgr, bubbles }
    }

    /// Bind this facade to one bubble for the `IndicatorSource` render-path
    /// contract.
    pub fn with_bubble(&self, bid: BubbleId) -> BubbleIndicatorView<'_> {
        BubbleIndicatorView::new(&self.mgr, bid)
    }

    /// All available indicator definitions (the 480+ catalog).
    pub fn definitions(&self) -> Vec<&IndicatorDefinition> {
        self.mgr.get_definitions()
    }

    /// Definition by type ID.
    pub fn definition(&self, type_id: &str) -> Option<&IndicatorDefinition> {
        self.mgr.get_definition(type_id)
    }

    /// Instance by ID.
    pub fn instance(&self, id: u64) -> Option<&IndicatorInstance> {
        self.mgr.get_instance(id)
    }

    /// Mutable instance by ID.
    pub fn instance_mut(&mut self, id: u64) -> Option<&mut IndicatorInstance> {
        self.mgr.get_instance_mut(id)
    }

    /// All instances bound to a bubble.
    pub fn instances_for_bubble(&self, bid: BubbleId) -> Vec<&IndicatorInstance> {
        self.mgr.get_instances_for_bubble(bid.0)
    }

    /// All instances, across every bubble.
    pub fn all_instances(&self) -> Vec<&IndicatorInstance> {
        self.mgr.get_all_instances()
    }

    /// Total instance count across every bubble.
    pub fn count(&self) -> usize {
        self.mgr.instances_count()
    }

    /// `(last_incremental_count, last_full_count)` from the most recent
    /// recalc pass — diagnostics only.
    pub fn recalc_stats(&self) -> (u32, u32) {
        (self.mgr.last_incremental_count, self.mgr.last_full_count)
    }

    /// Current recalculation mode.
    pub fn recalc_mode(&self) -> RecalcMode {
        self.mgr.recalc_mode
    }

    /// Set the recalculation mode.
    pub fn set_recalc_mode(&mut self, mode: RecalcMode) {
        self.mgr.recalc_mode = mode;
    }

    /// Clear all pending dirty flags without triggering recalculation.
    pub fn clear_pending(&mut self) {
        self.mgr.clear_pending();
    }

    /// Create a new indicator instance of `type_id` and (optionally) assign
    /// it to `bid` in one call. Validates `bid` actually exists before
    /// recording it on the new instance — guards against a stale id
    /// silently binding an instance to a deleted bubble. Returns `None` if
    /// the type is unknown or `bid` does not resolve to a live bubble.
    pub fn add(&mut self, bid: Option<BubbleId>, type_id: &str) -> Option<u64> {
        if let Some(b) = bid {
            if !self.bubbles.exists(b) {
                return None;
            }
        }
        let id = self.mgr.create_instance(type_id)?;
        if let Some(b) = bid {
            self.mgr.assign_bubble(id, Some(b.0));
        }
        Some(id)
    }

    /// Restore an instance at a specific id (undo/redo replay, preset/hydration
    /// restore) and assign it to `bid`. Returns `false` if the id already
    /// exists, the type is unknown, or `bid` does not resolve to a live
    /// bubble.
    pub fn restore_with_id(&mut self, id: u64, bid: Option<BubbleId>, type_id: &str) -> bool {
        if let Some(b) = bid {
            if !self.bubbles.exists(b) {
                return false;
            }
        }
        if !self.mgr.create_instance_with_id(id, type_id) {
            return false;
        }
        if let Some(b) = bid {
            self.mgr.assign_bubble(id, Some(b.0));
        }
        true
    }

    /// Remove an instance by id.
    pub fn remove(&mut self, id: u64) -> Option<IndicatorInstance> {
        self.mgr.remove_instance(id)
    }

    /// Reassign an existing instance to a different bubble (or unassign with
    /// `None`). Validates `bid` actually exists before recording it.
    pub fn reassign_bubble(&mut self, id: u64, bid: Option<BubbleId>) {
        if let Some(b) = bid {
            if !self.bubbles.exists(b) {
                return;
            }
        }
        self.mgr.assign_bubble(id, bid.map(|b| b.0));
    }

    /// Toggle visibility of an instance. Returns the new visibility state, or
    /// `false` if the instance was not found.
    pub fn toggle_visibility(&mut self, id: u64) -> bool {
        self.mgr.toggle_visibility(id).unwrap_or(false)
    }

    /// Remove every indicator instance bound to `bid`. Returns the number
    /// removed. Replaces `purge_synced_instances_for_window`.
    pub fn purge_for_bubble(&mut self, bid: BubbleId) -> usize {
        let before = self.mgr.instances_count();
        self.mgr.purge_synced_instances_for_bubble(bid.0);
        before - self.mgr.instances_count()
    }

    /// Swap the display order of two sub-pane indicator instances within a bubble.
    pub fn swap_pane_order(&mut self, bid: BubbleId, id_a: u64, id_b: u64) -> bool {
        self.mgr.swap_pane_order(bid.0, id_a, id_b)
    }

    /// Pick a distinct palette color for a new overlay indicator instance.
    pub fn pick_overlay_color(&self, type_id: &str, bid: BubbleId) -> Option<String> {
        self.mgr.pick_overlay_color(type_id, bid.0)
    }

    /// Calculate values for a single instance.
    pub fn calculate(&mut self, id: u64, bars: &[mlc_core::Bar]) {
        self.mgr.calculate(id, bars);
    }

    /// Recalculate every instance bound to `bid`. Collapses the legacy
    /// `calculate_for_bubble`/`calculate_all_for_bubble`/`_window` alias
    /// family — this always goes through the incremental-aware path
    /// (`calculate_for_bubble`).
    pub fn recalc_bubble(&mut self, bid: BubbleId, bars: &[mlc_core::Bar]) {
        self.mgr.calculate_for_bubble(bid.0, bars);
    }

    /// Calculate the Y-axis range for a sub-pane instance in the visible bar range.
    pub fn calculate_pane_range(&self, id: u64, visible_start: usize, visible_end: usize) -> Option<(f64, f64)> {
        self.mgr.calculate_pane_range(id, visible_start, visible_end)
    }

    /// Mark a symbol as needing indicator recalculation.
    pub fn mark_dirty(&mut self, symbol: &str) {
        self.mgr.mark_dirty(symbol);
    }

    /// Mark a symbol as having formed a new bar (also marks it dirty).
    pub fn mark_new_bar(&mut self, symbol: &str) {
        self.mgr.mark_new_bar(symbol);
    }

    /// Drain pending recalculation requests, returning the symbols to recalculate.
    pub fn drain_pending_recalc(&mut self) -> Vec<String> {
        self.mgr.drain_pending_recalc()
    }

    /// Remove all indicator instances (preset restore).
    pub fn clear_all(&mut self) {
        self.mgr.clear_all();
    }
}
