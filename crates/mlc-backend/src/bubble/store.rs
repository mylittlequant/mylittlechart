use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use mlc_core::chart::types::price_scale::ScaleMode;
use mlc_core::ids::{BubbleId, ConnectionStatus, SyncFlags};
use mlc_core::indicator_types::IndicatorGroupConfig;
use mlc_core::instrument::InstrumentAddress;
use mlc_core::state::{CommandHistory, Timeframe};

use super::data_bubble::DataBubble;

// =============================================================================
// BubbleStore
// =============================================================================

/// Manages data bubbles that bind multiple chart windows and panels.
///
/// Thin `HashMap<BubbleId, DataBubble>` + bid-keyed registry only (Step 6,
/// `docs/mlc/plans/doctrine-cutover-2026-06-27.md`). Member↔bubble routing
/// (`connect`/`disconnect`/`chart_member_ids`/`peers`/synced-panel set/
/// active-chart tracking/color routing) lives on
/// `mlc_backend::tag::TagManager` — every member→bubble routing question
/// goes through `core.tags()` now.
pub struct BubbleStore {
    bubbles: std::collections::HashMap<BubbleId, DataBubble>,
    next_indicator_config_id: u64,
}


impl BubbleStore {
    /// Create an empty `BubbleStore`.
    pub fn new() -> Self {
        Self {
            bubbles: std::collections::HashMap::new(),
            next_indicator_config_id: 1,
        }
    }

    // =========================================================================
    // Bubble lifecycle
    // =========================================================================

    /// Create a new data bubble with the given symbol and timeframe.
    ///
    /// Returns the id of the newly created bubble. Color / auto_created /
    /// members live on the paired `Tag` — caller pairs one via
    /// `core.tags_mut().insert(Tag::new(bid, color))` immediately after
    /// (Step 6 doctrine cutover).
    pub fn create_bubble(
        &mut self,
        symbol: String,
        timeframe: Timeframe,
    ) -> BubbleId {
        let id = BubbleId::generate();
        let bubble = DataBubble {
            id,
            primitives: Vec::new(),
            indicator_configs: Vec::new(),
            address: InstrumentAddress::new(String::new(), symbol, mlc_core::instrument::AccountType::default()),
            timeframe,
            chart_type: "candles",
            chart_type_param: 0.0,
            chart_type_interval: "1m",
            sync_flags: SyncFlags::default(),
            scale_mode: ScaleMode::Auto,
            command_history: CommandHistory::new(250),
            pre_tag_indicator_ids: Vec::new(),
            last_used_style: Arc::new(RwLock::new(HashMap::new())),
            prev_close_price: None,
            connection_status: ConnectionStatus::default(),
            // === Per-Kind storage (Phase X0) ===
            bars: Vec::new(),
            bars_source: crate::bubble::data_bubble::BarsSource::default(),
            footprint_bars: None,
            renko_bars: None,
            three_line_break: None,
            pnf_columns: None,
            kagi_segments: None,
            cvd_line: None,
            tpo_sessions: None,
            oi_delta: None,
            funding_rate: None,
            dom_heatmap: None,
            dom_trade_dots: None,
            liq_heatmap: None,
            liq_dots: None,
            alerts: Vec::new(),
            alert_next_id: 1,
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            scroll_prefetch_done: false,
            derived_seed_in_flight: None,
        };
        self.bubbles.insert(id, bubble);
        id
    }

    /// Create an invisible default bubble (no color tag in UI). Caller pairs
    /// a `Tag::new_auto(bid, color)` immediately after — see
    /// [`Self::create_bubble`].
    pub fn create_bubble_auto(
        &mut self,
        symbol: String,
        timeframe: Timeframe,
    ) -> BubbleId {
        let id = BubbleId::generate();
        let bubble = DataBubble {
            id,
            primitives: Vec::new(),
            indicator_configs: Vec::new(),
            address: InstrumentAddress::new(String::new(), symbol, mlc_core::instrument::AccountType::default()),
            timeframe,
            chart_type: "candles",
            chart_type_param: 0.0,
            chart_type_interval: "1m",
            sync_flags: SyncFlags::default(),
            scale_mode: ScaleMode::Auto,
            command_history: CommandHistory::new(250),
            pre_tag_indicator_ids: Vec::new(),
            last_used_style: Arc::new(RwLock::new(HashMap::new())),
            prev_close_price: None,
            connection_status: ConnectionStatus::default(),
            // === Per-Kind storage (Phase X0) ===
            bars: Vec::new(),
            bars_source: crate::bubble::data_bubble::BarsSource::default(),
            footprint_bars: None,
            renko_bars: None,
            three_line_break: None,
            pnf_columns: None,
            kagi_segments: None,
            cvd_line: None,
            tpo_sessions: None,
            oi_delta: None,
            funding_rate: None,
            dom_heatmap: None,
            dom_trade_dots: None,
            liq_heatmap: None,
            liq_dots: None,
            alerts: Vec::new(),
            alert_next_id: 1,
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            scroll_prefetch_done: false,
            derived_seed_in_flight: None,
        };
        self.bubbles.insert(id, bubble);
        id
    }

    /// Remove a data bubble. Caller is responsible for unmapping its members
    /// via `core.tags_mut().remove(id)` (member routing lives on
    /// `TagManager`, not here).
    pub fn remove_bubble(&mut self, id: BubbleId) {
        self.bubbles.remove(&id);
    }

    /// Get an immutable reference to a bubble by id.
    ///
    /// `pub(crate)` — external callers (outside `mlc-backend`) go
    /// through the `Bubbles`/`BubblesMut` facade instead (Step 3.2,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A). Kept `pub` at
    /// the crate level so `tick_writers`, `Alerts`/`AlertsMut`, and
    /// `Drawings`/`DrawingsMut` can still reach it directly.
    pub(crate) fn bubble(&self, id: BubbleId) -> Option<&DataBubble> {
        self.bubbles.get(&id)
    }

    /// Get a mutable reference to a bubble by id. `pub(crate)` — see [`Self::bubble`].
    pub(crate) fn bubble_mut(&mut self, id: BubbleId) -> Option<&mut DataBubble> {
        self.bubbles.get_mut(&id)
    }

    /// Returns `true` if a bubble with this id exists.
    pub fn exists(&self, id: BubbleId) -> bool {
        self.bubbles.contains_key(&id)
    }

    /// Iterate over every bubble's id.
    pub fn bubble_ids(&self) -> impl Iterator<Item = BubbleId> + '_ {
        self.bubbles.keys().copied()
    }

    /// Call `f` once per bubble with a bundled read view. Replaces the raw
    /// `bubbles()` iterator for external callers that need more than just
    /// the id (Step 3.2 §2.1 #6).
    pub fn each_bubble<F: FnMut(super::facade::BubbleView<'_>)>(&self, mut f: F) {
        for bubble in self.bubbles.values() {
            f(super::facade::BubbleView::new(bubble));
        }
    }

    /// Iterate over all data bubbles. `pub(crate)` — see [`Self::bubble`].
    pub(crate) fn bubbles(&self) -> impl Iterator<Item = &DataBubble> {
        self.bubbles.values()
    }

    /// Iterate over all data bubbles mutably. `pub(crate)` — see [`Self::bubble`].
    pub(crate) fn bubbles_mut(&mut self) -> impl Iterator<Item = &mut DataBubble> {
        self.bubbles.values_mut()
    }

    // =========================================================================
    // Instrument read helpers — convenience wrappers that replace read_* bridge fns
    // =========================================================================

    /// Resolve the bubble that owns the given instrument coordinates.
    ///
    /// Answers "which bubble holds the stream for `(exchange, account_type,
    /// symbol)`?". Used by the live-data bridge to route per-Kind series
    /// writes into bubble storage (Phase X0).
    ///
    /// Returns the first matching bubble. Instrument coordinates are intended
    /// to be unique per bubble.
    pub fn bubble_for_instrument(
        &self,
        exchange: &str,
        account_type: &str,
        symbol: &str,
    ) -> Option<BubbleId> {
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        self.bubbles.iter().find_map(|(id, b)| {
            (b.address.exchange == exchange && b.address.account_type == at && b.address.symbol == symbol)
                .then_some(*id)
        })
    }

    /// All bubbles whose `(exchange, account_type, symbol)` matches.
    ///
    /// Required for live-data writers when multiple charts each hold their
    /// own auto-bubble pointing at the same instrument — writing only to
    /// the singular result of `bubble_for_instrument` would starve every
    /// chart except the first one (see `mlc-app-test-harness` unit tier).
    pub fn bubbles_for_instrument(
        &self,
        exchange: &str,
        account_type: &str,
        symbol: &str,
    ) -> Vec<BubbleId> {
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        self.bubbles.iter()
            .filter_map(|(id, b)| {
                (b.address.exchange == exchange && b.address.account_type == at && b.address.symbol == symbol)
                    .then_some(*id)
            })
            .collect()
    }

    // =========================================================================
    // Symbol / Timeframe / Instrument
    // =========================================================================

    /// Update the timeframe for a bubble known by id directly. Caller
    /// resolves `chart_id → bid` via `core.tags().tag_for_chart(cid)` first
    /// (member routing lives on `TagManager`, not here).
    pub fn set_timeframe(&mut self, bid: BubbleId, tf: Timeframe) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.timeframe = tf;
        true
    }

    /// Update the chart type for a bubble known by id directly. Caller
    /// resolves `chart_id → bid` via `core.tags().tag_for_chart(cid)` first.
    pub fn set_chart_type(&mut self, bid: BubbleId, ct: &'static str) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.chart_type = ct;
        true
    }

    /// Update the chart-type numeric parameter (range_bar / volume_bar size)
    /// for a bubble known by id directly.
    pub fn set_chart_type_param(&mut self, bid: BubbleId, param: f64) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.chart_type_param = param;
        true
    }

    /// Update the chart-type kline interval (footprint) for a bubble known
    /// by id directly.
    pub fn set_chart_type_interval(&mut self, bid: BubbleId, iv: &'static str) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.chart_type_interval = iv;
        true
    }

    /// Update symbol, exchange, and account_type for a bubble known by id
    /// directly. Caller resolves `member → bid` via `core.tags()` first.
    pub fn set_instrument(
        &mut self,
        bid: BubbleId,
        symbol: String,
        exchange: String,
        account_type: String,
    ) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.address.symbol = symbol;
        bubble.address.exchange = exchange;
        bubble.address.account_type = account_type.parse().unwrap_or_default();
        true
    }

    /// Update exchange and account_type for a bubble known by id directly
    /// (no chart member required). Used by bare/headless bubble creation —
    /// e.g. the daemon's `POST /v1/bubbles` route, which creates a bubble
    /// with no chart attached yet.
    pub fn set_bubble_instrument(&mut self, bid: BubbleId, exchange: String, account_type: String) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.address.exchange = exchange;
        bubble.address.account_type = account_type.parse().unwrap_or_default();
        true
    }

    /// Update symbol/exchange/account_type/timeframe for a bubble known by id
    /// directly. Covers the "assign full instrument to a freshly-created
    /// bubble" shape used by chart-seed / new-window flows.
    pub fn set_bid_instrument_full(
        &mut self,
        bid: BubbleId,
        symbol: String,
        exchange: String,
        account_type: String,
        timeframe: Timeframe,
    ) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.address.symbol = symbol;
        bubble.address.exchange = exchange;
        bubble.address.account_type = account_type.parse().unwrap_or_default();
        bubble.timeframe = timeframe;
        true
    }

    /// Update just the exchange field for a bubble known by id directly.
    pub fn set_exchange_for_bid(&mut self, bid: BubbleId, exchange: String) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.address.exchange = exchange;
        true
    }

    // =========================================================================
    // Preset restore helpers
    // =========================================================================

    /// Remove all data bubbles. Used during preset restore to start from a
    /// clean slate before re-inserting bubbles reconstructed from snapshots.
    /// Caller pairs this with `core.tags_mut().clear()` (member routing +
    /// synced-panel set live on `TagManager`, not here).
    pub fn clear(&mut self) {
        self.bubbles.clear();
        self.next_indicator_config_id = 1;
    }

    // =========================================================================
    // Scoped park/unpark — bubble-id-subset extraction (shared-BackendCore arc,
    // §5.3). Replaces whole-store `swap()` for preset park/unpark: under a
    // single process-wide `BackendCore`, wholesale-swapping the store would
    // blank every OTHER window's bubbles too. Extraction removes only the
    // named ids into a small, fresh `BubbleStore`; merge re-inserts them.
    // =========================================================================

    /// Remove and return only the bubbles named in `ids` as a fresh, small
    /// `BubbleStore`. Ids not present in `self` are silently skipped.
    ///
    /// Used exclusively by preset tab park (`mlc-app/src/persistence/
    /// profile.rs::park_active_preset`).
    pub fn extract_bubbles(&mut self, ids: &[BubbleId]) -> BubbleStore {
        let mut out = BubbleStore::new();
        for &id in ids {
            if let Some(bubble) = self.bubbles.remove(&id) {
                out.bubbles.insert(id, bubble);
            }
        }
        out
    }

    /// Re-insert every bubble from `other` into `self`. `other` is consumed
    /// (its own `next_indicator_config_id` counter is discarded — that field
    /// is whole-store scoped and not currently used per-bubble; see
    /// `mlc-backend/src/bubble/store.rs` field doc).
    ///
    /// Used exclusively by preset tab unpark (`mlc-app/src/persistence/
    /// profile.rs::unpark_preset`).
    pub fn merge_bubbles(&mut self, other: BubbleStore) {
        for (id, bubble) in other.bubbles {
            self.bubbles.insert(id, bubble);
        }
    }

    /// Insert a fully-constructed [`DataBubble`] using its own id.
    ///
    /// No new id is generated. If a bubble with the same id already exists it
    /// is silently replaced.
    ///
    /// Private — the only caller is [`Self::restore_from_snapshots`]. External
    /// callers use that instead (Step 3.2, doctrine-cutover-2026-06-27.md §1A).
    fn insert_bubble_raw(&mut self, bubble: DataBubble) {
        let bubble_id = bubble.id;
        self.bubbles.insert(bubble_id, bubble);
    }

    /// Rebuild the entire bubble registry from persisted snapshots.
    ///
    /// For each snapshot: constructs a [`DataBubble`] via
    /// [`DataBubble::from_snapshot`], backfills primitive `symbol`/`exchange`/
    /// `account_type` from the bubble's own instrument fields (for primitives
    /// saved before those fields existed), and inserts the bubble.
    ///
    /// Pure registry data — the paired `Tag`s (color, auto_created, members,
    /// member_overrides, synced_panel_ids) are reconstructed separately from
    /// the same snapshot list by
    /// `crate::tag::TagManager::restore_from_snapshots` (Step 6 doctrine
    /// cutover). Does NOT call [`Self::clear`] — callers that want a full
    /// wipe-and-rebuild call `clear()` first.
    ///
    /// Returns the ids of every bubble restored, in snapshot order.
    pub fn restore_from_snapshots(
        &mut self,
        snaps: &[mlc_core::preset::snapshots::DataBubbleSnapshot],
    ) -> Vec<BubbleId> {
        let mut restored_ids = Vec::with_capacity(snaps.len());
        for snap in snaps {
            let mut bubble = DataBubble::from_snapshot(snap);
            if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                for ps in &snap.primitives {
                    if let Some(mut prim) = reg.from_json(&ps.type_id, &ps.json) {
                        // Backfill symbol/exchange/account_type for primitives
                        // saved before these fields existed (serde(default)
                        // gives empty string). Use the bubble's shared
                        // instrument as source of truth.
                        if prim.data().symbol.is_empty() && !snap.symbol.is_empty() {
                            prim.data_mut().symbol = snap.symbol.clone();
                        }
                        if prim.data().exchange.is_empty() && !snap.exchange.is_empty() {
                            prim.data_mut().exchange = snap.exchange.clone();
                        }
                        if prim.data().account_type.is_empty() && !snap.account_type.is_empty() {
                            prim.data_mut().account_type = snap.account_type.clone();
                        }
                        bubble.primitives.push(prim);
                    }
                }
            }
            let bid = bubble.id;
            self.insert_bubble_raw(bubble);
            restored_ids.push(bid);
        }
        restored_ids
    }

    // =========================================================================
    // Bid-keyed read intents (Step 3.2 §2.2 — the `Bubbles`/`BubblesMut`
    // facade forwards to these).
    // =========================================================================

    /// OHLC bars for a bubble. Empty slice if the bubble doesn't exist.
    /// Current `bars` ownership marker (see [`super::data_bubble::BarsSource`]).
    /// Unknown bubble reads as the default `Kline` — the safe side for the
    /// kline-family writers that consult this before writing.
    pub fn bars_source(&self, bid: BubbleId) -> super::data_bubble::BarsSource {
        self.bubbles.get(&bid).map(|b| b.bars_source).unwrap_or_default()
    }

    pub fn bars(&self, bid: BubbleId) -> &[mlc_core::Bar] {
        self.bubbles.get(&bid).map(|b| b.bars.as_slice()).unwrap_or(&[])
    }

    /// Footprint cluster bars, if any have been written.
    pub fn footprint_bars(&self, bid: BubbleId) -> Option<&[mlc_core::types::FootprintBar]> {
        self.bubbles.get(&bid)?.footprint_bars.as_deref()
    }

    /// Renko bricks, if any have been written.
    pub fn renko_bars(&self, bid: BubbleId) -> Option<&[mlc_core::types::RenkoBrick]> {
        self.bubbles.get(&bid)?.renko_bars.as_deref()
    }

    /// Three Line Break lines, if any have been written.
    pub fn three_line_break(&self, bid: BubbleId) -> Option<&[mlc_core::types::ThreeLineBreakLine]> {
        self.bubbles.get(&bid)?.three_line_break.as_deref()
    }

    /// Point-and-Figure columns, if any have been written.
    pub fn pnf_columns(&self, bid: BubbleId) -> Option<&[mlc_core::types::PnfColumn]> {
        self.bubbles.get(&bid)?.pnf_columns.as_deref()
    }

    /// Kagi segments, if any have been written.
    pub fn kagi_segments(&self, bid: BubbleId) -> Option<&[mlc_core::types::KagiSegment]> {
        self.bubbles.get(&bid)?.kagi_segments.as_deref()
    }

    /// Cumulative Volume Delta samples, if any have been written.
    pub fn cvd_line(&self, bid: BubbleId) -> Option<&[mlc_core::types::CvdSample]> {
        self.bubbles.get(&bid)?.cvd_line.as_deref()
    }

    /// TPO Market Profile session snapshots, if any have been written.
    pub fn tpo_sessions(&self, bid: BubbleId) -> Option<&[mlc_core::types::TpoSession]> {
        self.bubbles.get(&bid)?.tpo_sessions.as_deref()
    }

    /// Open Interest Δ bars, if any have been written.
    pub fn oi_delta(&self, bid: BubbleId) -> Option<&[mlc_core::types::OiDeltaBar]> {
        self.bubbles.get(&bid)?.oi_delta.as_deref()
    }

    /// Funding Rate cells, if any have been written.
    pub fn funding_rate(&self, bid: BubbleId) -> Option<&[mlc_core::types::FundingRateCell]> {
        self.bubbles.get(&bid)?.funding_rate.as_deref()
    }

    /// DOM heatmap ring buffer, if any has been written.
    pub fn dom_heatmap(&self, bid: BubbleId) -> Option<&mlc_core::chart::dom_heatmap::DomHeatmapBuffer> {
        self.bubbles.get(&bid)?.dom_heatmap.as_ref()
    }

    /// DOM trade-circle ring overlay, if any has been written.
    pub fn dom_trade_dots(&self, bid: BubbleId) -> Option<&[mlc_core::chart::dom_heatmap::DomTradeDot]> {
        self.bubbles.get(&bid)?.dom_trade_dots.as_deref()
    }

    /// Liquidation heatmap ring buffer, if any has been written.
    pub fn liq_heatmap(&self, bid: BubbleId) -> Option<&mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer> {
        self.bubbles.get(&bid)?.liq_heatmap.as_ref()
    }

    /// Liquidation dot overlay, if any has been written.
    pub fn liq_dots(&self, bid: BubbleId) -> Option<&[mlc_core::chart::liquidation_heatmap::LiquidationDot]> {
        self.bubbles.get(&bid)?.liq_dots.as_deref()
    }

    /// Symbol of a bubble known by id directly. `None` if missing or empty.
    pub fn symbol(&self, bid: BubbleId) -> Option<&str> {
        self.bubbles.get(&bid).map(|b| b.address.symbol.as_str()).filter(|s| !s.is_empty())
    }

    /// Exchange of a bubble known by id directly. `None` if missing or empty.
    pub fn exchange(&self, bid: BubbleId) -> Option<&str> {
        self.bubbles.get(&bid).map(|b| b.address.exchange.as_str()).filter(|s| !s.is_empty())
    }

    /// Account type of a bubble known by id directly. `None` if missing or empty.
    pub fn account_type(&self, bid: BubbleId) -> Option<&str> {
        self.bubbles.get(&bid).map(|b| b.address.account_type.as_label()).filter(|s| !s.is_empty())
    }

    /// Typed account type of a bubble known by id directly. `None` only if
    /// the bubble is missing — unlike the label form, a typed `AccountType`
    /// has no "empty" state to filter.
    pub fn account_type_typed(&self, bid: BubbleId) -> Option<mlc_core::instrument::AccountType> {
        self.bubbles.get(&bid).map(|b| b.address.account_type)
    }

    /// Timeframe of a bubble known by id directly.
    pub fn timeframe(&self, bid: BubbleId) -> Option<&Timeframe> {
        self.bubbles.get(&bid).map(|b| &b.timeframe)
    }

    /// Chart type of a bubble known by id directly.
    pub fn chart_type(&self, bid: BubbleId) -> Option<&'static str> {
        self.bubbles.get(&bid).map(|b| b.chart_type)
    }

    /// Chart-type numeric parameter of a bubble known by id directly.
    pub fn chart_type_param(&self, bid: BubbleId) -> Option<f64> {
        self.bubbles.get(&bid).map(|b| b.chart_type_param)
    }

    /// Chart-type kline interval of a bubble known by id directly.
    pub fn chart_type_interval(&self, bid: BubbleId) -> Option<&'static str> {
        self.bubbles.get(&bid).map(|b| b.chart_type_interval)
    }

    /// Bundled instrument-address + timeframe read — the shape most DTO
    /// builders (e.g. the daemon's bubble snapshot route) actually want.
    pub fn instrument(&self, bid: BubbleId) -> Option<InstrumentKey> {
        let b = self.bubbles.get(&bid)?;
        Some(InstrumentKey {
            address: b.address.clone(),
            timeframe: b.timeframe.clone(),
        })
    }

    /// Price-scale corner mode of a bubble known by id directly.
    pub fn scale_mode(&self, bid: BubbleId) -> Option<ScaleMode> {
        self.bubbles.get(&bid).map(|b| b.scale_mode)
    }

    /// Sync flags of a bubble known by id directly.
    pub fn sync_flags(&self, bid: BubbleId) -> Option<SyncFlags> {
        self.bubbles.get(&bid).map(|b| b.sync_flags.clone())
    }

    /// Shared undo/redo history of a bubble known by id directly.
    pub fn command_history(&self, bid: BubbleId) -> Option<&CommandHistory> {
        self.bubbles.get(&bid).map(|b| &b.command_history)
    }

    /// Previous session close price of a bubble known by id directly.
    pub fn prev_close(&self, bid: BubbleId) -> Option<f64> {
        self.bubbles.get(&bid).and_then(|b| b.prev_close_price)
    }

    /// Live/Delayed/Disconnected status of a bubble known by id directly.
    pub fn connection_status(&self, bid: BubbleId) -> Option<ConnectionStatus> {
        self.bubbles.get(&bid).map(|b| b.connection_status)
    }

    /// Indicator configs of a bubble known by id directly. Empty slice if missing.
    pub fn indicator_configs(&self, bid: BubbleId) -> &[IndicatorGroupConfig] {
        self.bubbles.get(&bid).map(|b| b.indicator_configs.as_slice()).unwrap_or(&[])
    }

    /// Indicator instance ids hidden pre-tag (desync/rejoin bookkeeping).
    /// Empty slice if missing.
    pub fn pre_tag_indicator_ids(&self, bid: BubbleId) -> &[u64] {
        self.bubbles.get(&bid).map(|b| b.pre_tag_indicator_ids.as_slice()).unwrap_or(&[])
    }

    // =========================================================================
    // Bid-keyed write intents (Step 3.2 §2.4)
    // =========================================================================

    /// Append one bar to a bubble's OHLC series.
    pub fn append_bar(&mut self, bid: BubbleId, bar: mlc_core::Bar) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.append_bar(bar);
        }
    }

    /// Replace a bubble's entire OHLC series.
    pub fn replace_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.replace_bars(bars);
        }
    }

    /// Merge a page of scroll-loaded bars into the existing series by
    /// timestamp (last-write-wins on collision — the live ring's version of
    /// any overlapping bar takes priority), sorted oldest→newest. Returns the
    /// merged series so the caller can build the matching `BubbleEvent`.
    pub fn merge_scroll_bars(&mut self, bid: BubbleId, bars: &[mlc_core::Bar]) -> Vec<mlc_core::Bar> {
        let Some(b) = self.bubbles.get_mut(&bid) else { return Vec::new() };
        let mut by_ts: std::collections::BTreeMap<i64, mlc_core::Bar> = std::collections::BTreeMap::new();
        for bar in bars { by_ts.insert(bar.timestamp, *bar); }
        for bar in &b.bars { by_ts.insert(bar.timestamp, *bar); }
        let merged: Vec<mlc_core::Bar> = by_ts.into_values().collect();
        b.replace_bars(merged.clone());
        merged
    }

    /// Replace a bubble's entire OHLC series (restore/hydration path — same
    /// underlying write as `replace_bars`, distinct name for call-site intent
    /// clarity between "live tick" and "restore").
    pub fn set_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.replace_bars(bars);
        }
    }

    /// Set the price-scale corner mode of a bubble known by id directly.
    pub fn set_scale_mode(&mut self, bid: BubbleId, mode: ScaleMode) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.scale_mode = mode;
        }
    }

    /// Set the previous session close price of a bubble known by id directly.
    pub fn set_prev_close(&mut self, bid: BubbleId, price: Option<f64>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.prev_close_price = price;
        }
    }

    /// Set the Live/Delayed/Disconnected status of a bubble known by id directly.
    pub fn set_connection_status(&mut self, bid: BubbleId, status: ConnectionStatus) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.connection_status = status;
        }
    }

    /// Set the pre-tag indicator ids of a bubble known by id directly
    /// (desync-join bookkeeping write).
    pub fn set_pre_tag_indicator_ids(&mut self, bid: BubbleId, ids: Vec<u64>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.pre_tag_indicator_ids = ids;
        }
    }
}

/// Bundled `{ address, timeframe }` read — see [`BubbleStore::instrument`].
#[derive(Debug, Clone)]
pub struct InstrumentKey {
    pub address: InstrumentAddress,
    pub timeframe: Timeframe,
}

impl Default for BubbleStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod park_unpark_tests {
    //! Round-trip coverage for `extract_bubbles`/`merge_bubbles` — the
    //! shared-BackendCore-arc scoped-park replacement for whole-store
    //! `swap()` (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §5.3).

    use super::*;

    #[test]
    fn extract_then_merge_round_trips_to_identical_store() {
        let mut store = BubbleStore::new();
        let a = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        let b = store.create_bubble("ETHUSDT".to_string(), Timeframe::h1());
        let c = store.create_bubble("SOLUSDT".to_string(), Timeframe::h1());
        store.append_bar(a, mlc_core::Bar {
            timestamp: 1, open: 1.0, high: 1.0, low: 1.0, close: 1.0, volume: 1.0,
        });

        // Extract only `a` and `b` — `c` must stay behind untouched.
        let extracted = store.extract_bubbles(&[a, b]);
        assert!(!store.exists(a));
        assert!(!store.exists(b));
        assert!(store.exists(c), "non-extracted bubble must remain in the source store");
        assert!(extracted.exists(a));
        assert!(extracted.exists(b));
        assert!(!extracted.exists(c));
        assert_eq!(extracted.bars(a).len(), 1, "bubble data must travel with extraction");

        // Merge back — store must be identical to pre-extraction in content.
        store.merge_bubbles(extracted);
        assert!(store.exists(a));
        assert!(store.exists(b));
        assert!(store.exists(c));
        assert_eq!(store.bars(a).len(), 1);
    }

    #[test]
    fn extract_skips_ids_not_present() {
        let mut store = BubbleStore::new();
        let a = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        let missing = BubbleId(999_999);

        let extracted = store.extract_bubbles(&[a, missing]);
        assert!(extracted.exists(a));
        assert!(!extracted.exists(missing));
        assert_eq!(extracted.bubble_ids().count(), 1);
    }

    #[test]
    fn extract_other_windows_bubbles_are_untouched() {
        // Simulates the §5.1 hazard directly: window A's bubbles must never
        // be visible in — or removed from — an extraction scoped to window
        // B's bubble ids.
        let mut store = BubbleStore::new();
        let window_a_bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        let window_b_bid = store.create_bubble("ETHUSDT".to_string(), Timeframe::h1());

        let parked_b = store.extract_bubbles(&[window_b_bid]);

        assert!(store.exists(window_a_bid), "window A's bubble must survive parking window B's preset");
        assert!(!parked_b.exists(window_a_bid), "extraction must not leak window A's bubble into window B's payload");
    }
}
