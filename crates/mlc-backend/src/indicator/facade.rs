//! `Indicators` / `IndicatorsMut` — the only doctrinally-sanctioned access
//! path to `IndicatorManager` from outside `mlc-backend`.
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

use mlc_core::drawing::TimeframeVisibilityConfig;
use mlc_core::ids::BubbleId;
use mlc_core::indicator_source::{
    IndicatorInfo, IndicatorRenderInstance, IndicatorSettingsData, IndicatorSource, SignalShape,
};
use mlc_backend_indicator_engine::managers::indicator_manager::{IndicatorDefinition, IndicatorInstance};
use mlc_backend_indicator_engine::managers::stream_event::StreamEvent;
use mlc_backend_indicator_engine::{IndicatorParamValue, OutputConfig, RecalcMode};

use crate::bubble::BubbleStore;
use super::manager::IndicatorManager;

/// Full settable state of an [`IndicatorInstance`], used to copy one
/// instance's configuration onto another (sync-group clone, tag-join
/// clone). Deliberately excludes identity fields (`id`, `type_id`,
/// `name`, computed `values`, `signals`) — those are either fixed at
/// creation or runtime-derived.
#[derive(Debug, Clone, Default)]
pub struct IndicatorFullConfig {
    pub params: std::collections::HashMap<String, IndicatorParamValue>,
    pub outputs: std::collections::HashMap<String, OutputConfig>,
    pub visible: bool,
    pub locked: bool,
    pub pane: usize,
    pub order: i32,
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
    pub origin_id: Option<u64>,
}

impl From<&IndicatorInstance> for IndicatorFullConfig {
    fn from(src: &IndicatorInstance) -> Self {
        Self {
            params: src.params.clone(),
            outputs: src.outputs.clone(),
            visible: src.visible,
            locked: src.locked,
            pane: src.pane,
            order: src.order,
            timeframe_visibility: src.timeframe_visibility.clone(),
            origin_id: src.origin_id,
        }
    }
}

/// Snapshot of the fields callers most commonly read off an instance in one
/// shot (id-keyed CRUD summaries: agent API, daemon REST DTOs, sidebar tree
/// items). Replaces ad-hoc `.instance(id).map(|inst| (inst.a, inst.b, ...))`
/// call sites — see [`Indicators::summary`] / [`IndicatorsMut::summary`].
#[derive(Debug, Clone)]
pub struct IndicatorSummary {
    pub id: u64,
    pub type_id: String,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub pane: usize,
    pub bubble_id: Option<u64>,
}

impl From<&IndicatorInstance> for IndicatorSummary {
    fn from(src: &IndicatorInstance) -> Self {
        Self {
            id: src.id,
            type_id: src.type_id.clone(),
            name: src.name.clone(),
            visible: src.visible,
            locked: src.locked,
            pane: src.pane,
            bubble_id: src.bubble_id,
        }
    }
}

/// One bubble's indicator state, bundled for the render path — the
/// bubble-scoped `IndicatorSource` implementation chart rendering functions
/// consume. Mirrors Drawing's `BubbleDrawingView`
/// (`mlc_backend::drawing::facade`).
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

    /// Whether an instance with this id exists.
    pub fn exists(&self, id: u64) -> bool {
        self.mgr.get_instance(id).is_some()
    }

    /// CRUD-summary snapshot (id, type_id, name, visible, locked, pane,
    /// bubble_id) — the fields agent-API/daemon-REST/sidebar-tree call
    /// sites need together.
    pub fn summary(&self, id: u64) -> Option<IndicatorSummary> {
        self.mgr.get_instance(id).map(IndicatorSummary::from)
    }

    /// Full owned clone of the instance. For sync-group / tag-join clone
    /// flows that build an [`IndicatorFullConfig`] from the source instance
    /// and need `id` + `type_id` alongside the config — anything narrower
    /// would just re-fetch the same instance twice.
    pub fn full_clone(&self, id: u64) -> Option<IndicatorInstance> {
        self.mgr.get_instance(id).cloned()
    }

    /// Type ID (references `IndicatorDefinition`).
    /// Data streams the indicator type DECLARED it consumes, from its mli
    /// catalog signature (`input_stream` + `aux_streams`). A bubble uses
    /// this to decide which live feeds it must keep alive for the
    /// indicators bound to it — the same way it already keeps klines and
    /// trades alive for its chart.
    pub fn definition_streams(&self, type_id: &str) -> Vec<mlc_backend_indicator_engine::StreamKind> {
        self.definition(type_id).map(|d| d.streams.clone()).unwrap_or_default()
    }

    pub fn type_id_of(&self, id: u64) -> Option<String> {
        self.mgr.get_instance(id).map(|inst| inst.type_id.clone())
    }

    /// Display name.
    pub fn name_of(&self, id: u64) -> Option<String> {
        self.mgr.get_instance(id).map(|inst| inst.name.clone())
    }

    /// Pane index (0 = main chart, 1+ = separate panes).
    pub fn pane_of(&self, id: u64) -> Option<usize> {
        self.mgr.get_instance(id).map(|inst| inst.pane)
    }

    /// Whether the instance's bubble id matches a live bubble — the raw
    /// `bubble_id` field, not existence-checked against `BubbleStore`.
    pub fn bubble_id_of(&self, id: u64) -> Option<Option<u64>> {
        self.mgr.get_instance(id).map(|inst| inst.bubble_id)
    }

    /// Whether signal generation is enabled for this instance.
    pub fn signals_enabled_of(&self, id: u64) -> Option<bool> {
        self.mgr.get_instance(id).map(|inst| inst.signals_enabled)
    }

    /// Snapshot of the signal-marker display config.
    pub fn signal_display_of(&self, id: u64) -> Option<mlc_core::indicator_source::SignalDisplayConfig> {
        self.mgr.get_instance(id).map(|inst| inst.signal_display.clone())
    }

    /// Snapshot of the timeframe-visibility config (`None` = not set —
    /// distinct from "instance not found", hence the outer `Option`).
    pub fn timeframe_visibility_of(&self, id: u64) -> Option<Option<TimeframeVisibilityConfig>> {
        self.mgr.get_instance(id).map(|inst| inst.timeframe_visibility.clone())
    }

    /// Single calculation parameter's current value.
    pub fn param_value(&self, id: u64, name: &str) -> Option<IndicatorParamValue> {
        self.mgr.get_instance(id).and_then(|inst| inst.params.get(name).cloned())
    }

    /// Snapshot of every calculation parameter.
    pub fn params_snapshot(&self, id: u64) -> Option<std::collections::HashMap<String, IndicatorParamValue>> {
        self.mgr.get_instance(id).map(|inst| inst.params.clone())
    }

    /// Color override for one output.
    pub fn output_color(&self, id: u64, output_name: &str) -> Option<String> {
        self.mgr.get_instance(id).and_then(|inst| inst.outputs.get(output_name)).and_then(|o| o.color.clone())
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

    /// Whether an instance with this id exists.
    pub fn exists(&self, id: u64) -> bool {
        self.mgr.get_instance(id).is_some()
    }

    /// CRUD-summary snapshot (id, type_id, name, visible, locked, pane,
    /// bubble_id).
    pub fn summary(&self, id: u64) -> Option<IndicatorSummary> {
        self.mgr.get_instance(id).map(IndicatorSummary::from)
    }

    /// Full owned clone of the instance.
    pub fn full_clone(&self, id: u64) -> Option<IndicatorInstance> {
        self.mgr.get_instance(id).cloned()
    }

    /// Replace an instance wholesale from its serialized form (undo/redo of a
    /// parameter edit). The caller recalculates.
    pub fn replace_from_json(&mut self, id: u64, json: &str) -> bool {
        self.mgr.replace_from_json(id, json)
    }

    /// Type ID (references `IndicatorDefinition`).
    /// Data streams the indicator type DECLARED it consumes, from its mli
    /// catalog signature (`input_stream` + `aux_streams`). A bubble uses
    /// this to decide which live feeds it must keep alive for the
    /// indicators bound to it — the same way it already keeps klines and
    /// trades alive for its chart.
    pub fn definition_streams(&self, type_id: &str) -> Vec<mlc_backend_indicator_engine::StreamKind> {
        self.definition(type_id).map(|d| d.streams.clone()).unwrap_or_default()
    }

    pub fn type_id_of(&self, id: u64) -> Option<String> {
        self.mgr.get_instance(id).map(|inst| inst.type_id.clone())
    }

    /// Display name.
    pub fn name_of(&self, id: u64) -> Option<String> {
        self.mgr.get_instance(id).map(|inst| inst.name.clone())
    }

    /// Pane index (0 = main chart, 1+ = separate panes).
    pub fn pane_of(&self, id: u64) -> Option<usize> {
        self.mgr.get_instance(id).map(|inst| inst.pane)
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

    /// Restore an instance's full persisted state from a preset/hydration
    /// [`IndicatorSnapshot`](mlc_core::preset::snapshots::IndicatorSnapshot).
    /// Call after [`Self::restore_with_id`] has created the instance at
    /// `snap.id`. Output style entries are unconditionally overwritten
    /// (the persisted snapshot is the full truth, unlike template merge).
    /// Returns `false` if the instance does not exist.
    pub fn restore_from_snapshot(&mut self, snap: &mlc_core::preset::snapshots::IndicatorSnapshot) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(snap.id) else { return false };
        inst.name = snap.name.clone();
        inst.pane = snap.pane;
        inst.order = snap.order;
        inst.visible = snap.visible;
        inst.locked = snap.locked;
        inst.origin_id = snap.origin_id;
        inst.signals_enabled = snap.signals_enabled;
        inst.timeframe_visibility = snap.timeframe_visibility.clone();
        for (k, v) in &snap.params {
            if let Ok(iv) = serde_json::from_value(v.clone()) {
                inst.params.insert(k.clone(), iv);
            }
        }
        for (key, out_snap) in &snap.outputs {
            let entry = inst.outputs.entry(key.clone()).or_default();
            entry.color = out_snap.color.clone();
            entry.line_width = out_snap.line_width;
            if let Some(vis) = out_snap.visible {
                entry.visible = vis;
            }
        }
        // Restore cached computed values so indicator lines are visible
        // immediately on tab switch, before the next recalc pass.
        if !snap.values.is_empty() {
            inst.values = std::sync::Arc::new(snap.values.clone());
        }
        true
    }

    /// Publish a host-computed series onto an instance's output.
    ///
    /// The host-rendered entries carry `machine_id: None`, so the compute
    /// pass produces nothing for them. Their data arrives on the bubble
    /// (open interest, funding) and is resampled onto the bar grid by the
    /// caller; writing it here lets them render through the ORDINARY output
    /// path — a line, like any other sub-pane indicator — instead of a
    /// bespoke painter.
    pub fn publish_values(&mut self, id: u64, output: &str, values: Vec<f64>) -> bool {
        match self.mgr.get_instance_mut(id) {
            Some(inst) => {
                inst.set_values(output, values);
                true
            }
            None => false,
        }
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
    ///
    /// "Every" is literal: the bubble owns its instances, so when the bubble
    /// goes away they go with it. This used to delegate to
    /// `purge_synced_instances_for_bubble`, which filters `origin_id.is_some()`
    /// and therefore reaped only sync-group CLONES — deleting a bubble left
    /// every original instance in the manager, bound to a dead bubble id, with
    /// no render site able to reach it and no way to delete it.
    pub fn purge_for_bubble(&mut self, bid: BubbleId) -> usize {
        let before = self.mgr.instances_count();
        self.mgr.purge_all_instances_for_bubble(bid.0);
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

    /// Calculate values for a single instance. Builds the real per-bar
    /// (buy_volume, sell_volume) delta series (if the instance's bubble has
    /// footprint or CVD-line data) so delta-aware variants (CVD,
    /// VolumeDelta) get real data instead of freezing.
    pub fn calculate(&mut self, id: u64, bars: &[mlc_core::Bar]) {
        let bid = self.mgr.get_instance(id).and_then(|inst| inst.bubble_id).map(BubbleId);
        let deltas = bid.and_then(|b| self.build_deltas_for_bubble(b, bars));
        self.mgr.calculate_with_deltas(id, bars, deltas.as_deref());
    }

    /// Recalculate every instance bound to `bid`. Collapses the legacy
    /// `calculate_for_bubble`/`calculate_all_for_bubble`/`_window` alias
    /// family — this always goes through the incremental-aware path
    /// (`calculate_for_bubble`).
    ///
    /// Builds the real per-bar (buy_volume, sell_volume) delta series from
    /// the bubble's footprint bars (preferred, real server-side per-level
    /// aggressor volume) or standalone CVD-line samples (fallback), and
    /// threads it into the compute path for delta-aware indicator variants.
    pub fn recalc_bubble(&mut self, bid: BubbleId, bars: &[mlc_core::Bar]) {
        let deltas = self.build_deltas_for_bubble(bid, bars);
        self.mgr.calculate_for_bubble(bid.0, bars, deltas.as_deref());
    }

    /// Feed one stream event (tick, order book, order book delta, funding
    /// rate, open interest, liquidation) to every stream-consuming
    /// indicator on `bid`. Called once per event (`Tick`/`Funding`/
    /// `OpenInterest`) or once per frame per changed snapshot
    /// (`OrderBook`/`Liquidation`) — see the arc D plan's per-stream
    /// hook-point table (`docs/mlc/plans/
    /// indicator-streams-arc-d-2026-09-25.md` §3.3).
    pub fn feed_stream(&mut self, bid: BubbleId, ev: &StreamEvent) {
        self.mgr.feed_stream_for_bubble(bid.0, ev);
    }

    /// Build the real per-bar (buy_volume, sell_volume) delta series for
    /// `bid`, aligned by index to `bars`. Returns `None` when neither
    /// footprint bars nor a CVD line are available for this bubble (the
    /// overwhelming majority of bubbles — no allocation in that case).
    ///
    /// Priority:
    /// 1. `footprint_bars` (real, per-price-level aggressor volume already
    ///    cached server-side) — `FootprintBar.open_time` is milliseconds,
    ///    `Bar.timestamp` is seconds; bars with no matching footprint bar
    ///    get `(0.0, 0.0)`.
    /// 2. `cvd_line` (real running cumulative delta, independent of
    ///    footprint mode) — per-bar delta = `cvd_at_or_before(bar_end) -
    ///    cvd_at_or_before(prev_bar_end)`, encoded as signed
    ///    `(delta.max(0.0), (-delta).max(0.0))` so `buy - sell == delta`.
    fn build_deltas_for_bubble(&self, bid: BubbleId, bars: &[mlc_core::Bar]) -> Option<Vec<(f64, f64)>> {
        if bars.is_empty() {
            return None;
        }

        if let Some(fp_bars) = self.bubbles.footprint_bars(bid) {
            if !fp_bars.is_empty() {
                let mut by_ts_sec: std::collections::HashMap<i64, (f64, f64)> =
                    std::collections::HashMap::with_capacity(fp_bars.len());
                for fb in fp_bars {
                    let (buy, sell) = fb.levels.iter()
                        .fold((0.0_f64, 0.0_f64), |(b, s), l| (b + l.buy_vol, s + l.sell_vol));
                    by_ts_sec.insert(fb.open_time / 1000, (buy, sell));
                }
                return Some(
                    bars.iter()
                        .map(|b| by_ts_sec.get(&b.timestamp).copied().unwrap_or((0.0, 0.0)))
                        .collect(),
                );
            }
        }

        if let Some(cvd) = self.bubbles.cvd_line(bid) {
            if !cvd.is_empty() {
                // Two-pointer sweep: samples are append-ordered (ascending
                // ts_ms) and the per-bar bounds are monotonically increasing,
                // so one forward cursor covers all bars in O(bars + samples).
                // A per-bar rev-scan is O(bars × samples) — with a deep
                // sample line and 2000 bars that's real jank on every recalc.
                let mut cursor = 0usize;
                let mut last_val = 0.0_f64;
                let mut advance_to = |bound_ms: i64| -> f64 {
                    while cursor < cvd.len() && cvd[cursor].ts_ms <= bound_ms {
                        last_val = cvd[cursor].value;
                        cursor += 1;
                    }
                    last_val
                };

                let mut out = Vec::with_capacity(bars.len());
                let mut prev_cvd = advance_to(bars[0].timestamp * 1000 - 1);
                for i in 0..bars.len() {
                    // bar_end = next bar's start (exclusive), or i64::MAX
                    // for the last bar (= "as of the latest known sample").
                    let bar_end_ms = bars.get(i + 1).map(|b| b.timestamp * 1000).unwrap_or(i64::MAX);
                    let cur_cvd = advance_to(bar_end_ms);
                    let delta = cur_cvd - prev_cvd;
                    out.push((delta.max(0.0), (-delta).max(0.0)));
                    prev_cvd = cur_cvd;
                }
                return Some(out);
            }
        }

        None
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

    /// Remove and return only the indicator instances bound to `bubble_ids`
    /// as a fresh, small `IndicatorManager`. Used exclusively by preset tab
    /// park (shared-BackendCore arc §5.3) — replaces whole-manager `swap()`,
    /// which under a shared core would rip out every other window's
    /// indicators too.
    pub fn extract_for_bubbles(&mut self, bubble_ids: &[BubbleId]) -> IndicatorManager {
        let ids: Vec<u64> = bubble_ids.iter().map(|b| b.0).collect();
        self.mgr.extract_for_bubbles(&ids)
    }

    /// Re-insert every instance and cache entry from `other`. Used
    /// exclusively by preset tab unpark.
    pub fn merge_for_bubbles(&mut self, other: IndicatorManager) {
        self.mgr.merge_for_bubbles(other)
    }

    // =========================================================================
    // Intent API — replaces the raw `instance_mut()` escape (Step 3.3 tail).
    // Every method below finds the instance internally and mutates one named
    // aspect of it; app code never holds a `&mut IndicatorInstance`.
    // =========================================================================

    /// Set the instance-level display name.
    pub fn set_name(&mut self, id: u64, name: impl Into<String>) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.name = name.into();
        true
    }

    /// Set a single calculation parameter.
    pub fn set_param(&mut self, id: u64, name: &str, value: IndicatorParamValue) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.set_param(name, value);
        true
    }

    /// Set instance-level visibility (whether the indicator is shown at all).
    pub fn set_visible(&mut self, id: u64, visible: bool) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.visible = visible;
        true
    }

    /// Set the locked flag.
    pub fn set_locked(&mut self, id: u64, locked: bool) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.locked = locked;
        true
    }

    /// Set the color override for one output.
    pub fn set_output_color(&mut self, id: u64, output_name: &str, color: impl Into<String>) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        let Some(cfg) = inst.outputs.get_mut(output_name) else { return false };
        cfg.color = Some(color.into());
        true
    }

    /// Set the same color override across every output (auto-color for a new
    /// overlay duplicate).
    pub fn set_all_output_colors(&mut self, id: u64, color: impl Into<String>) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        let color = color.into();
        for cfg in inst.outputs.values_mut() {
            cfg.color = Some(color.clone());
        }
        true
    }

    /// Clear every calculation parameter and output style override, reverting
    /// the instance to its type's defaults.
    pub fn reset_to_defaults(&mut self, id: u64) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.params.clear();
        inst.outputs.clear();
        true
    }

    /// Enable/disable signal generation for this instance.
    pub fn set_signals_enabled(&mut self, id: u64, enabled: bool) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.signals_enabled = enabled;
        true
    }

    /// Set the signal marker shape.
    pub fn set_signal_shape(&mut self, id: u64, shape: SignalShape) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.signal_display.shape = shape;
        true
    }

    /// Set the bullish signal marker color.
    pub fn set_signal_bullish_color(&mut self, id: u64, color: impl Into<String>) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.signal_display.bullish_color = color.into();
        true
    }

    /// Set the bearish signal marker color.
    pub fn set_signal_bearish_color(&mut self, id: u64, color: impl Into<String>) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.signal_display.bearish_color = color.into();
        true
    }

    /// Set the signal marker size in pixels.
    pub fn set_signal_size(&mut self, id: u64, size: f64) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.signal_display.size = size;
        true
    }

    /// Set the signal marker offset from bar high/low in pixels.
    pub fn set_signal_offset(&mut self, id: u64, offset: f64) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.signal_display.offset = offset;
        true
    }

    /// Apply a closure to the instance's timeframe-visibility config,
    /// materialising `TimeframeVisibilityConfig::all()` first if unset.
    /// Covers every slider/toggle mutation on the range-per-timeframe UI
    /// (dual-slider drag, min/max text commit, per-timeframe toggle button).
    pub fn update_timeframe_visibility(
        &mut self,
        id: u64,
        f: impl FnOnce(&mut TimeframeVisibilityConfig),
    ) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        let mut cfg = inst.timeframe_visibility.clone().unwrap_or_else(TimeframeVisibilityConfig::all);
        f(&mut cfg);
        inst.timeframe_visibility = Some(cfg);
        true
    }

    /// Clear the sync-group origin link (this instance is no longer
    /// considered a clone of another).
    pub fn clear_origin(&mut self, id: u64) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.origin_id = None;
        true
    }

    /// Apply a full settable-state patch (params, outputs, visibility,
    /// lock, pane, order, timeframe-visibility, origin link) — the
    /// sync-group / tag-join clone intent (copy one instance's full
    /// configuration onto a freshly-created instance).
    pub fn apply_full_config(&mut self, id: u64, config: IndicatorFullConfig) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.params = config.params;
        inst.outputs = config.outputs;
        inst.visible = config.visible;
        inst.locked = config.locked;
        inst.pane = config.pane;
        inst.order = config.order;
        inst.timeframe_visibility = config.timeframe_visibility;
        inst.origin_id = config.origin_id;
        true
    }

    /// Apply a saved indicator-set template entry (visibility + params +
    /// per-output style overrides) onto an existing instance. Output style
    /// entries are merged into whatever the instance already has, not
    /// replaced wholesale (matches the pre-cutover `deploy_indicator_set`
    /// merge behaviour): each `OutputStyleConfig` field only overwrites the
    /// instance's `OutputConfig` field when it is `Some`.
    pub fn apply_template(
        &mut self,
        id: u64,
        visible: bool,
        params: impl IntoIterator<Item = (String, IndicatorParamValue)>,
        outputs: impl IntoIterator<Item = (String, mlc_core::templates::OutputStyleConfig)>,
    ) -> bool {
        let Some(inst) = self.mgr.get_instance_mut(id) else { return false };
        inst.visible = visible;
        for (k, v) in params {
            inst.params.insert(k, v);
        }
        for (key, out_style) in outputs {
            let entry = inst.outputs.entry(key).or_default();
            if let Some(color) = out_style.color {
                entry.color = Some(color);
            }
            if let Some(lw) = out_style.line_width {
                entry.line_width = Some(lw);
            }
            if let Some(vis) = out_style.visible {
                entry.visible = vis;
            }
        }
        true
    }
}
