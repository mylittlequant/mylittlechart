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
    pub fn bars_len(&self) -> usize {
        self.bubble.bars.len()
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
    /// per symbol, for `GroupView::per_symbol_counts` (Details-tab modal).
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

    // ---- Reads ------------------------------------------------------------

    pub fn bars(&self, bid: BubbleId) -> &[mlc_core::Bar] {
        self.store.bars(bid)
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

    pub fn funding_rate(&self, bid: BubbleId) -> Option<&[mlc_core::types::FundingRateCell]> {
        self.store.funding_rate(bid)
    }

    pub fn dom_heatmap(&self, bid: BubbleId) -> Option<&mlc_core::chart::dom_heatmap::DomHeatmapBuffer> {
        self.store.dom_heatmap(bid)
    }

    pub fn dom_trade_dots(&self, bid: BubbleId) -> Option<&[mlc_core::chart::dom_heatmap::DomTradeDot]> {
        self.store.dom_trade_dots(bid)
    }

    pub fn liq_heatmap(&self, bid: BubbleId) -> Option<&mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer> {
        self.store.liq_heatmap(bid)
    }

    pub fn liq_dots(&self, bid: BubbleId) -> Option<&[mlc_core::chart::liquidation_heatmap::LiquidationDot]> {
        self.store.liq_dots(bid)
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

    pub fn timeframe(&self, bid: BubbleId) -> Option<&Timeframe> {
        self.store.timeframe(bid)
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
    pub fn render_state_for<'w>(
        &'w self,
        bid: BubbleId,
        window: &'w mlc_core::state::chart_container::ChartContainer,
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

    pub fn symbol(&self, bid: BubbleId) -> Option<&str> {
        self.store.symbol(bid)
    }

    pub fn scale_mode(&self, bid: BubbleId) -> Option<ScaleMode> {
        self.store.scale_mode(bid)
    }

    pub fn bubbles_for_instrument(&self, exchange: &str, account_type: &str, symbol: &str) -> Vec<BubbleId> {
        self.store.bubbles_for_instrument(exchange, account_type, symbol)
    }

    // ---- Registry — lifecycle ----------------------------------------------

    pub fn create_group(&mut self, symbol: String, timeframe: Timeframe) -> BubbleId {
        self.store.create_group(symbol, timeframe)
    }

    pub fn create_group_auto(&mut self, symbol: String, timeframe: Timeframe) -> BubbleId {
        self.store.create_group_auto(symbol, timeframe)
    }

    pub fn remove_group(&mut self, bid: BubbleId) {
        self.store.remove_group(bid)
    }

    // ---- Writes -------------------------------------------------------------

    pub fn append_bar(&mut self, bid: BubbleId, bar: mlc_core::Bar) {
        self.store.append_bar(bid, bar)
    }

    pub fn replace_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        self.store.replace_bars(bid, bars)
    }

    pub fn merge_scroll_bars(&mut self, bid: BubbleId, bars: &[mlc_core::Bar]) -> Vec<mlc_core::Bar> {
        self.store.merge_scroll_bars(bid, bars)
    }

    pub fn set_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        self.store.set_bars(bid, bars)
    }

    pub fn set_timeframe(&mut self, bid: BubbleId, tf: Timeframe) -> bool {
        self.store.set_timeframe(bid, tf)
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

    /// Replace the entire store with `new`, returning the previous one.
    ///
    /// Used exclusively by preset tab park/unpark (`mlc-app/src/persistence/
    /// profile.rs`), which swaps the whole live-state bundle for a tab in
    /// one shot.
    pub fn swap(&mut self, new: BubbleStore) -> BubbleStore {
        std::mem::replace(&mut self.store, new)
    }
}
