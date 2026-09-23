//! `Bubbles` / `BubblesMut` — the only doctrinally-sanctioned access path to
//! `BubbleStore` from outside `mlc-backend`.
//!
//! Unlike Alert/Drawing, `BubbleStore` IS the manager (decision (β), no
//! separate `BubbleManager` struct — see
//! `docs/mlc/plans/step-3.2-bubble-manager-design.md` §1). The facade holds
//! exactly one guard.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::chart::types::price_scale::ScaleMode;
use mlc_core::ids::{BubbleId, ConnectionStatus, SyncFlags, SyncMemberId};
use mlc_core::indicator_types::IndicatorGroupConfig;
use mlc_core::state::{CommandHistory, Timeframe};

use super::data_bubble::DataBubble;
use super::store::{BubbleStore, InstrumentKey};

/// Bundled read view of one bubble — used by [`Bubbles::each_bubble`] so
/// callers that need more than just the id don't have to reach `&DataBubble`
/// by field name.
pub struct BubbleView<'a> {
    bubble: &'a DataBubble,
}

impl<'a> BubbleView<'a> {
    pub(crate) fn new(bubble: &'a DataBubble) -> Self {
        Self { bubble }
    }

    pub fn id(&self) -> BubbleId {
        self.bubble.id
    }
    /// Whether an active account-wide alert targets this bubble's
    /// instrument (`AlertManager::resync_has_alerts` keeps this mirror
    /// fresh) — the release guard `park_active_preset`/`apply_preset_cold`
    /// use to keep an alerted bubble resident across a preset switch.
    pub fn has_active_alerts(&self) -> bool {
        self.bubble.has_active_alerts
    }
    pub fn symbol(&self) -> &str {
        &self.bubble.address.symbol
    }
    pub fn exchange(&self) -> &str {
        &self.bubble.address.exchange
    }
    pub fn account_type(&self) -> &str {
        self.bubble.address.account_type.as_label()
    }
    pub fn timeframe(&self) -> &Timeframe {
        &self.bubble.timeframe
    }
    pub fn chart_type(&self) -> &'static str {
        self.bubble.chart_type
    }
    pub fn chart_type_param(&self) -> f64 {
        self.bubble.chart_type_param
    }
    pub fn chart_type_interval(&self) -> &'static str {
        self.bubble.chart_type_interval
    }
    pub fn bars_len(&self) -> usize {
        self.bubble.bars.len()
    }
    /// Total points across every kline raw layer on this bubble — memory
    /// diagnostics (wasm-leak differential, 2026-07-26).
    pub fn kline_layer_points(&self) -> usize {
        self.bubble.kline_layers.values().map(|l| l.len()).sum()
    }

    /// DOM-heatmap ring diagnostics: cells, distinct bar columns, grid
    /// geometry, and the span it covers. The overlay is silent both when no
    /// snapshot has landed and when the writer stalled — these tell them
    /// apart.
    pub fn dom_probe(&self) -> (usize, usize, i64, i64, i64, i64) {
        match self.bubble.dom_heatmap.as_ref() {
            Some(b) => (
                b.cells.len(),
                b.bucket_count(),
                b.bar_ms,
                b.row_ticks,
                b.oldest_bucket_ms(),
                b.newest_bucket_ms(),
            ),
            None => (0, 0, 0, 0, 0, 0),
        }
    }

    /// Total resting size in the live (forming) heat column, bids + asks.
    ///
    /// The one number that says WHOSE book is in the grid: folding four
    /// venues has to move it, and a source toggle that leaves it unchanged
    /// did nothing regardless of what the picture looks like.
    pub fn dom_live_sum(&self) -> f64 {
        let Some(b) = self.bubble.dom_heatmap.as_ref() else { return 0.0 };
        b.live.iter().map(|c| c.bid_depth + c.ask_depth).sum()
    }

    /// Realized-liquidation tape: `(prints, venues, total notional)`.
    pub fn liq_probe(&self) -> (usize, usize, f64) {
        match self.bubble.liq_tape.as_ref() {
            Some(t) => (t.prints.len(), t.venues.len(), t.total_usd()),
            None => (0, 0, 0.0),
        }
    }

    /// Presence mask over the last `n` bar columns, oldest character first:
    /// `1` = that bar has cells, `0` = the bar was never written.
    ///
    /// A cell count cannot show this — 30k cells spread over 201 of 427
    /// possible columns and a full 427 look identical in the total. The
    /// SHAPE of the holes is the diagnosis: evenly spaced means something
    /// periodic (a resubscribe, a resync), a ragged run means the writer was
    /// starved in bursts.
    pub fn dom_column_mask(&self, n: usize) -> String {
        let Some(b) = self.bubble.dom_heatmap.as_ref() else { return String::new() };
        if b.cells.is_empty() || b.bar_ms <= 0 {
            return String::new();
        }
        let newest = b.cells.back().map(|c| c.ts_ms).unwrap_or(0);
        let present: std::collections::HashSet<i64> = b.cells.iter().map(|c| c.ts_ms).collect();
        (0..n)
            .rev()
            .map(|i| {
                let ts = newest - (i as i64) * b.bar_ms;
                if present.contains(&ts) { '1' } else { '0' }
            })
            .collect()
    }

    /// Price span the heat ring actually covers, `(low, high)` — the answer
    /// to "is the layer missing rows, or are they there and invisible".
    pub fn dom_price_span(&self) -> (f64, f64) {
        let Some(b) = self.bubble.dom_heatmap.as_ref() else { return (0.0, 0.0) };
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for c in b.cells.iter().chain(b.live.iter()) {
            let p = c.price(b.tick_size, b.row_ticks);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        if lo > hi { (0.0, 0.0) } else { (lo, hi) }
    }
    pub fn bars(&self) -> &[mlc_core::Bar] {
        &self.bubble.bars
    }
    pub fn primitive_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.bubble.primitives.iter().map(|p| p.data().id)
    }
    pub fn max_primitive_id(&self) -> Option<u64> {
        self.primitive_ids().max()
    }
    pub fn sync_flags(&self) -> SyncFlags {
        self.bubble.sync_flags.clone()
    }
    /// `(indicator_config_symbol_or_bubble_symbol, primitive_symbol)` counts
    /// per symbol, for `TagView::per_symbol_counts` (Details-tab modal).
    pub fn indicator_configs(&self) -> &[IndicatorGroupConfig] {
        &self.bubble.indicator_configs
    }
    pub fn primitive_symbols(&self) -> impl Iterator<Item = &str> + '_ {
        self.bubble.primitives.iter().map(|p| p.data().symbol.as_str())
    }
    /// Persistence snapshot of this bubble, joined with its paired `Tag`
    /// (Step 6 doctrine cutover — color/auto_created/members/
    /// member_overrides live on `Tag`, not `DataBubble`).
    pub fn to_snapshot(
        &self,
        tag: &mlc_core::tag::Tag,
        synced_panels: &[SyncMemberId],
    ) -> mlc_core::preset::snapshots::DataBubbleSnapshot {
        super::snapshot::to_snapshot(self.bubble, tag, synced_panels)
    }
}

/// Read-only facade over `BubbleStore`.
pub struct Bubbles<'a> {
    store: RwLockReadGuard<'a, BubbleStore>,
}

impl<'a> Bubbles<'a> {
    pub(crate) fn new(store: RwLockReadGuard<'a, BubbleStore>) -> Self {
        Self { store }
    }

    // ---- Registry — lifecycle & lookup -----------------------------------

    pub fn exists(&self, bid: BubbleId) -> bool {
        self.store.exists(bid)
    }

    pub fn bubble_ids(&self) -> impl Iterator<Item = BubbleId> + '_ {
        self.store.bubble_ids()
    }

    pub fn each_bubble<F: FnMut(BubbleView<'_>)>(&self, f: F) {
        self.store.each_bubble(f)
    }

    pub fn bubble_for_instrument(&self, exchange: &str, account_type: &str, symbol: &str) -> Option<BubbleId> {
        self.store.bubble_for_instrument(exchange, account_type, symbol)
    }

    pub fn bubbles_for_instrument(&self, exchange: &str, account_type: &str, symbol: &str) -> Vec<BubbleId> {
        self.store.bubbles_for_instrument(exchange, account_type, symbol)
    }

    /// Resolve the bubble matching `address` exactly (exchange, symbol AND
    /// account_type) — the "does a subscription already exist for this
    /// instrument?" check `BackendCore::ensure_alert_bubble` uses.
    pub fn get_by_address(&self, address: &mlc_core::instrument::InstrumentAddress) -> Option<BubbleId> {
        self.store.get_by_address(address)
    }

    // ---- Reads ------------------------------------------------------------

    pub fn bars(&self, bid: BubbleId) -> &[mlc_core::Bar] {
        self.store.bars(bid)
    }

    pub fn bars_source(&self, bid: BubbleId) -> super::data_bubble::BarsSource {
        self.store.bars_source(bid)
    }

    /// Stale-array tripwire stamp (K1 lifecycle groundwork, chart-type-
    /// engine-consolidation B2). See
    /// [`mlc_core::chart_type::BarsGeneration`]'s doc comment.
    pub fn bars_generation(&self, bid: BubbleId) -> mlc_core::chart_type::BarsGeneration {
        self.store.bars_generation(bid)
    }

    /// History depth backing the bubble's currently-active chart_type.
    pub fn history_depth(&self, bid: BubbleId) -> super::data_bubble::HistoryDepth {
        self.store.history_depth(bid)
    }

    pub fn footprint_bars(&self, bid: BubbleId) -> Option<&[mlc_core::types::FootprintBar]> {
        self.store.footprint_bars(bid)
    }

    pub fn renko_bars(&self, bid: BubbleId) -> Option<&[mlc_core::types::RenkoBrick]> {
        self.store.renko_bars(bid)
    }

    pub fn three_line_break(&self, bid: BubbleId) -> Option<&[mlc_core::types::ThreeLineBreakLine]> {
        self.store.three_line_break(bid)
    }

    pub fn pnf_columns(&self, bid: BubbleId) -> Option<&[mlc_core::types::PnfColumn]> {
        self.store.pnf_columns(bid)
    }

    pub fn kagi_segments(&self, bid: BubbleId) -> Option<&[mlc_core::types::KagiSegment]> {
        self.store.kagi_segments(bid)
    }

    pub fn cvd_line(&self, bid: BubbleId) -> Option<&[mlc_core::types::CvdSample]> {
        self.store.cvd_line(bid)
    }

    pub fn tpo_sessions(&self, bid: BubbleId) -> Option<&[mlc_core::types::TpoSession]> {
        self.store.tpo_sessions(bid)
    }

    pub fn oi_delta(&self, bid: BubbleId) -> Option<&[mlc_core::types::OiDeltaBar]> {
        self.store.oi_delta(bid)
    }

    /// Per-venue open-interest history rows, `venue -> row count`.
    pub fn oi_hist_shape(&self, bid: BubbleId) -> Vec<(String, usize)> {
        self.store.oi_hist_shape(bid)
    }

    /// Projected liquidation levels cached on the bubble.
    pub fn liq_projection(
        &self,
        bid: BubbleId,
    ) -> Option<&[mlc_core::chart::liquidation_projection::ProjectedLevel]> {
        self.store.liq_projection(bid)
    }

    pub fn funding_rate(&self, bid: BubbleId) -> Option<&[mlc_core::types::FundingRateCell]> {
        self.store.funding_rate(bid)
    }

    pub fn dom_heatmap(&self, bid: BubbleId) -> Option<&mlc_core::chart::dom_heatmap::DomHeatmapBuffer> {
        self.store.dom_heatmap(bid)
    }

    pub fn dom_trade_dots(&self, bid: BubbleId) -> Option<&[mlc_core::chart::dom_heatmap::DomTradeDot]> {
        self.store.dom_trade_dots(bid)
    }

    pub fn liq_tape(&self, bid: BubbleId) -> Option<&mlc_core::chart::liquidations::LiquidationTape> {
        self.store.liq_tape(bid)
    }

    /// Raw kline layer for timeframe `tf` (unified-data-plane Stage Э1).
    pub fn kline_layer(&self, bid: BubbleId, tf: &str) -> Option<&super::raw_layer::RawLayer<mlc_core::Bar>> {
        self.store.kline_layer(bid, tf)
    }

    /// Full chronological snapshot of the raw kline layer for timeframe
    /// `tf`, oldest-first. Empty if missing/untouched.
    pub fn kline_layer_snapshot(&self, bid: BubbleId, tf: &str) -> Vec<mlc_core::Bar> {
        self.store.kline_layer_snapshot(bid, tf)
    }

    pub fn symbol(&self, bid: BubbleId) -> Option<&str> {
        self.store.symbol(bid)
    }

    pub fn exchange(&self, bid: BubbleId) -> Option<&str> {
        self.store.exchange(bid)
    }

    pub fn account_type(&self, bid: BubbleId) -> Option<&str> {
        self.store.account_type(bid)
    }

    /// Typed account type — canonical form, see
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1C.
    pub fn account_type_typed(&self, bid: BubbleId) -> Option<mlc_core::instrument::AccountType> {
        self.store.account_type_typed(bid)
    }

    pub fn timeframe(&self, bid: BubbleId) -> Option<&Timeframe> {
        self.store.timeframe(bid)
    }

    pub fn chart_type(&self, bid: BubbleId) -> Option<&'static str> {
        self.store.chart_type(bid)
    }

    pub fn chart_type_param(&self, bid: BubbleId) -> Option<f64> {
        self.store.chart_type_param(bid)
    }

    pub fn chart_type_param_mode(&self, bid: BubbleId) -> Option<mlc_core::chart_type::ParamMode> {
        self.store.chart_type_param_mode(bid)
    }

    pub fn chart_type_interval(&self, bid: BubbleId) -> Option<&'static str> {
        self.store.chart_type_interval(bid)
    }

    pub fn instrument(&self, bid: BubbleId) -> Option<InstrumentKey> {
        self.store.instrument(bid)
    }

    pub fn scale_mode(&self, bid: BubbleId) -> Option<ScaleMode> {
        self.store.scale_mode(bid)
    }

    pub fn sync_flags(&self, bid: BubbleId) -> Option<SyncFlags> {
        self.store.sync_flags(bid)
    }

    pub fn command_history(&self, bid: BubbleId) -> Option<&CommandHistory> {
        self.store.command_history(bid)
    }

    pub fn prev_close(&self, bid: BubbleId) -> Option<f64> {
        self.store.prev_close(bid)
    }

    pub fn connection_status(&self, bid: BubbleId) -> Option<ConnectionStatus> {
        self.store.connection_status(bid)
    }

    pub fn indicator_configs(&self, bid: BubbleId) -> &[IndicatorGroupConfig] {
        self.store.indicator_configs(bid)
    }

    pub fn pre_tag_indicator_ids(&self, bid: BubbleId) -> &[u64] {
        self.store.pre_tag_indicator_ids(bid)
    }

    // ---- Bundled render-state builder --------------------------------------

    /// Bundled render-state builder for one bubble — the only place a
    /// `&DataBubble` momentarily crosses into a backend-owned function
    /// (`build_chart_render_state`, `mlc-backend::bubble::render`) that
    /// legitimately needs the whole struct for the render pass. Never
    /// exposes `&DataBubble` itself to the caller.
    ///
    /// `window` is a narrow struct-of-refs (not `&ChartContainer` — that
    /// type lives in `mlc-app`, which `mlc-backend` cannot depend on).
    pub fn render_state_for<'w>(
        &'w self,
        bid: BubbleId,
        window: super::render::ChartWindowRenderInputs<'w>,
        chart_rect: mlc_core::chart::render_data::ChartRect,
        theme: &'w mlc_core::chart::render_data::ChartTheme,
        current_timeframe: Option<&'w str>,
        time_format_settings: Option<&'w mlc_core::TimeFormatSettings>,
        timeframe_interval_ms: Option<i64>,
        grid: &'w mlc_core::chart::types::GridOptions,
        legend: &'w mlc_core::chart::types::Legend,
    ) -> Option<mlc_core::chart::render_data::ChartRenderState<'w>> {
        let bubble = self.store.bubble(bid)?;
        Some(super::render::build_chart_render_state(
            window,
            bubble,
            chart_rect,
            theme,
            current_timeframe,
            time_format_settings,
            timeframe_interval_ms,
            grid,
            legend,
        ))
    }

    /// TRANSITIONAL — general mechanical-sweep escape hatch, read-side
    /// counterpart of [`BubblesMut::with_bubble_mut`]. Runs `f` with `&DataBubble`
    /// for the duration of the call only; the closure completes within one
    /// statement so the guard never escapes (mirrors `ChartApp::with_bubble`,
    /// which forwards here).
    pub fn with_bubble<R>(&self, bid: BubbleId, f: impl FnOnce(&DataBubble) -> R) -> Option<R> {
        self.store.bubble(bid).map(f)
    }

    /// Persistence snapshot of every bubble, joined with its paired `Tag`
    /// (preset save). `tags` is the sibling `Tags` facade — LOCK ORDER:
    /// caller acquires `bubble` first, then `tag` (this method takes both
    /// already-acquired facades, doesn't acquire locks itself).
    pub fn snapshot_all(
        &self,
        tags: &crate::tag::Tags<'_>,
        synced_panels: &[SyncMemberId],
    ) -> Vec<mlc_core::preset::snapshots::DataBubbleSnapshot> {
        let mut out = Vec::new();
        self.each_bubble(|bv| {
            if let Some(tag) = tags.get(bv.id()) {
                out.push(bv.to_snapshot(tag, synced_panels));
            }
        });
        out
    }
}

/// Mutable facade over `BubbleStore`.
pub struct BubblesMut<'a> {
    store: RwLockWriteGuard<'a, BubbleStore>,
}

impl<'a> BubblesMut<'a> {
    pub(crate) fn new(store: RwLockWriteGuard<'a, BubbleStore>) -> Self {
        Self { store }
    }

    // ---- Read passthrough (mirrors `Bubbles`, for write-holders that also
    // need to read without re-acquiring) -------------------------------------

    pub fn bars(&self, bid: BubbleId) -> &[mlc_core::Bar] {
        self.store.bars(bid)
    }

    pub fn bars_source(&self, bid: BubbleId) -> super::data_bubble::BarsSource {
        self.store.bars_source(bid)
    }

    /// History depth backing the bubble's currently-active chart_type.
    pub fn history_depth(&self, bid: BubbleId) -> super::data_bubble::HistoryDepth {
        self.store.history_depth(bid)
    }

    pub fn symbol(&self, bid: BubbleId) -> Option<&str> {
        self.store.symbol(bid)
    }

    pub fn timeframe(&self, bid: BubbleId) -> Option<&Timeframe> {
        self.store.timeframe(bid)
    }

    pub fn scale_mode(&self, bid: BubbleId) -> Option<ScaleMode> {
        self.store.scale_mode(bid)
    }

    pub fn bubbles_for_instrument(&self, exchange: &str, account_type: &str, symbol: &str) -> Vec<BubbleId> {
        self.store.bubbles_for_instrument(exchange, account_type, symbol)
    }

    // ---- Registry — lifecycle ----------------------------------------------

    pub fn create_bubble(&mut self, symbol: String, timeframe: Timeframe) -> BubbleId {
        self.store.create_bubble(symbol, timeframe)
    }

    pub fn create_bubble_auto(&mut self, symbol: String, timeframe: Timeframe) -> BubbleId {
        self.store.create_bubble_auto(symbol, timeframe)
    }

    pub fn remove_bubble(&mut self, bid: BubbleId) {
        self.store.remove_bubble(bid)
    }

    // ---- Writes -------------------------------------------------------------

    pub fn append_bar(&mut self, bid: BubbleId, bar: mlc_core::Bar) {
        self.store.append_bar(bid, bar)
    }

    pub fn replace_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        self.store.replace_bars(bid, bars)
    }

    pub fn set_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        self.store.set_bars(bid, bars)
    }

    /// Live-tick write into a bubble's raw kline layer for timeframe `tf`
    /// (unified-data-plane Stage Э1).
    pub fn kline_layer_push_live(&mut self, bid: BubbleId, tf: &str, bar: mlc_core::Bar) {
        self.store.kline_layer_push_live(bid, tf, bar)
    }

    /// Backfill-batch write into a bubble's raw kline layer for timeframe
    /// `tf` (unified-data-plane Stage Э1).
    pub fn kline_layer_prepend_batch(&mut self, bid: BubbleId, tf: &str, bars: Vec<mlc_core::Bar>) {
        self.store.kline_layer_prepend_batch(bid, tf, bars)
    }

    /// Phase-A / re-subscribe write into a bubble's raw kline layer for
    /// timeframe `tf` — see [`BubbleStore::kline_layer_seed_block`].
    pub fn kline_layer_seed_block(&mut self, bid: BubbleId, tf: &str, block: Vec<mlc_core::Bar>) {
        self.store.kline_layer_seed_block(bid, tf, block)
    }

    /// Full chronological snapshot of the raw kline layer for timeframe
    /// `tf`, oldest-first. Empty if missing/untouched.
    pub fn kline_layer_snapshot(&self, bid: BubbleId, tf: &str) -> Vec<mlc_core::Bar> {
        self.store.kline_layer_snapshot(bid, tf)
    }

    /// Configure the raw kline-layer ring capacity for a bubble known by id
    /// directly (unified-data-plane Stage Э1). Trade-layer capacity is no
    /// longer configured per-bubble — see
    /// [`crate::trade_store::TradeStore::set_default_capacity`]
    /// (`BackendCore::set_layer_capacities` calls both).
    pub fn set_layer_capacities(&mut self, bid: BubbleId, kline_capacity: usize) {
        self.with_bubble_mut(bid, |b| b.set_layer_capacities(kline_capacity));
    }

    pub fn set_timeframe(&mut self, bid: BubbleId, tf: Timeframe) -> bool {
        self.store.set_timeframe(bid, tf)
    }

    pub fn set_chart_type(&mut self, bid: BubbleId, ct: &'static str) -> bool {
        self.store.set_chart_type(bid, ct)
    }

    pub fn set_chart_type_param(&mut self, bid: BubbleId, param: f64) -> bool {
        self.store.set_chart_type_param(bid, param)
    }

    pub fn set_chart_type_param_mode(&mut self, bid: BubbleId, mode: mlc_core::chart_type::ParamMode) -> bool {
        self.store.set_chart_type_param_mode(bid, mode)
    }

    pub fn set_chart_type_interval(&mut self, bid: BubbleId, iv: &'static str) -> bool {
        self.store.set_chart_type_interval(bid, iv)
    }

    /// Record a seed/deepen depth report for `chart_type` on `bid`. Ignored
    /// (both by the store lookup and by `DataBubble::set_history_depth`'s
    /// own guard) when `bid` is unknown or `chart_type` doesn't match the
    /// bubble's currently-active chart_type.
    pub fn set_history_depth(&mut self, bid: BubbleId, chart_type: &str, depth: super::data_bubble::HistoryDepth) -> bool {
        self.store.set_history_depth(bid, chart_type, depth)
    }

    pub fn set_instrument(&mut self, bid: BubbleId, symbol: String, exchange: String, account_type: String) -> bool {
        self.store.set_instrument(bid, symbol, exchange, account_type)
    }

    pub fn set_bubble_instrument(&mut self, bid: BubbleId, exchange: String, account_type: String) -> bool {
        self.store.set_bubble_instrument(bid, exchange, account_type)
    }

    pub fn set_bid_instrument_full(&mut self, bid: BubbleId, symbol: String, exchange: String, account_type: String, timeframe: Timeframe) -> bool {
        self.store.set_bid_instrument_full(bid, symbol, exchange, account_type, timeframe)
    }

    pub fn set_exchange_for_bid(&mut self, bid: BubbleId, exchange: String) -> bool {
        self.store.set_exchange_for_bid(bid, exchange)
    }

    /// Drain kline-forwarder keys orphaned by an instrument-identity change
    /// this tick — see [`super::store::PendingKlineTeardown`]'s doc comment.
    /// Called once per frame from `BackendCore::tick_writers`.
    pub fn take_pending_kline_teardowns(&mut self) -> Vec<super::store::PendingKlineTeardown> {
        self.store.take_pending_kline_teardowns()
    }

    pub fn set_scale_mode(&mut self, bid: BubbleId, mode: ScaleMode) {
        self.store.set_scale_mode(bid, mode)
    }

    pub fn set_prev_close(&mut self, bid: BubbleId, price: Option<f64>) {
        self.store.set_prev_close(bid, price)
    }

    pub fn set_connection_status(&mut self, bid: BubbleId, status: ConnectionStatus) {
        self.store.set_connection_status(bid, status)
    }

    pub fn set_pre_tag_indicator_ids(&mut self, bid: BubbleId, ids: Vec<u64>) {
        self.store.set_pre_tag_indicator_ids(bid, ids)
    }

    // ---- Bulk / restore -----------------------------------------------------

    pub fn clear(&mut self) {
        self.store.clear()
    }

    pub fn restore_from_snapshots(
        &mut self,
        snaps: &[mlc_core::preset::snapshots::DataBubbleSnapshot],
    ) -> Vec<BubbleId> {
        self.store.restore_from_snapshots(snaps)
    }

    // ---- Transitional escape hatch (mirrors Drawing's `with_bubble_mut`) --

    /// TRANSITIONAL — general mechanical-sweep escape hatch for call sites
    /// with unusual multi-field / multi-statement bodies not yet worth a
    /// dedicated intent method. Also used by `tick_writers`-adjacent code
    /// that legitimately needs `&mut DataBubble` for a per-Kind-series write
    /// not yet covered by a named intent (Step 3.2 §6.5 — intra-crate use is
    /// exempt from the facade-hiding rationale; this fn is `pub` because a
    /// few outer-crate call sites need the exact same closure shape for
    /// stashed-primitives + multi-field restore bodies during preset load).
    pub fn with_bubble_mut<R>(&mut self, bid: BubbleId, f: impl FnOnce(&mut DataBubble) -> R) -> Option<R> {
        self.store.bubble_mut(bid).map(f)
    }

    /// Mark every bubble refold-owed (`refold_dirty = true`) — the
    /// broadcast-`Lagged` recovery path in `BackendCore::tick_writers`:
    /// dropped updates may include `LayerBackfilled` pages, so the next
    /// `stage_boundary` sweep must resynthesize every fold from the raw
    /// layers rather than trust the last reseed.
    pub fn mark_all_refold_dirty(&mut self) {
        let ids: Vec<BubbleId> = self.store.bubble_ids().collect();
        for bid in ids {
            if let Some(b) = self.store.bubble_mut(bid) {
                b.refold_dirty = true;
            }
        }
    }

    /// Remove and return only the bubbles named in `ids` as a fresh, small
    /// `BubbleStore`. Used exclusively by preset tab park (shared-BackendCore
    /// arc §5.3) — replaces whole-store `swap()`, which under a shared core
    /// would blank every other window's bubbles too.
    pub fn extract_bubbles(&mut self, ids: &[BubbleId]) -> BubbleStore {
        self.store.extract_bubbles(ids)
    }

    /// Re-insert every bubble from `other` into the store. Used exclusively
    /// by preset tab unpark.
    pub fn merge_bubbles(&mut self, other: BubbleStore) {
        self.store.merge_bubbles(other)
    }
}
