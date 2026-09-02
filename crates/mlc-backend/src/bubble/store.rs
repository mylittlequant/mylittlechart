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
    /// Kline-forwarder keys orphaned by an instrument-identity reassignment
    /// this tick, queued for `BackendCore::tick_writers` to retire — see
    /// [`PendingKlineTeardown`]'s doc comment.
    pending_kline_teardowns: Vec<PendingKlineTeardown>,
}

/// Snapshot of a kline-forwarder key that just lost its only reason to
/// exist: a bubble that was displaying `(exchange, account_type, symbol,
/// timeframe)` was reassigned to a different instrument.
///
/// Captured by every `identity_changed` branch in the `set_*instrument*`
/// family (alongside `reset_raw_layers()`) and drained by
/// `BackendCore::tick_writers`, which owns the `DataBridge` handle needed to
/// actually abort the forwarder — `BubbleStore` itself has no bridge access
/// (doctrine: `bridge: Arc<DataBridge>` lives on `BackendCore` only).
///
/// Fixes a measured leak (2026-08-30): no `set_*instrument*` call site ever
/// tore down the OLD instrument's live kline forwarder — only the bespoke
/// TF-switch/chart-type-switch handlers in `mlc-app` called
/// `abort_kline_bars` explicitly, and a plain symbol switch (watchlist row
/// click) matched none of them. Measured via Gemini's `/v2/candles` REST
/// poll: 0 requests before ever visiting Gemini, 8 while on it, 23 (still
/// climbing) after switching away to BTCUSDT/BINANCE — every venue visited
/// once left its poller running forever, compounding across every switch.
#[derive(Debug, Clone)]
pub struct PendingKlineTeardown {
    pub exchange: String,
    pub account_type: mlc_core::instrument::AccountType,
    pub symbol: String,
    pub timeframe: String,
}

/// Lifecycle tripwire (2026-07-17): every genuine instrument-identity
/// change (the ONLY trigger of `reset_raw_layers`) is logged with the
/// full old→new key and the setter that drove it. Identity changes are
/// rare, user-driven events; an unexpected line here is the first stop
/// when a chart loses its series (series-leak / display-wipe forensics).
fn log_identity_reset(
    bubble: &DataBubble,
    bid: BubbleId,
    new_exchange: &str,
    new_at: mlc_core::instrument::AccountType,
    new_symbol: &str,
    via: &str,
) {
    mlc_backend_data_bridge::dlog!(
        "[instrument-reset] via={} bid={:?} old={}:{}:{} new={}:{}:{} ({} display bars dropped)",
        via, bid,
        bubble.address.exchange, bubble.address.account_type.as_label(), bubble.address.symbol,
        new_exchange, new_at.as_label(), new_symbol,
        bubble.bars.len(),
    );
}


impl BubbleStore {
    /// Create an empty `BubbleStore`.
    pub fn new() -> Self {
        Self {
            bubbles: std::collections::HashMap::new(),
            next_indicator_config_id: 1,
            pending_kline_teardowns: Vec::new(),
        }
    }

    /// Drain every kline-forwarder key orphaned by an instrument-identity
    /// change since the last drain. See [`PendingKlineTeardown`]'s doc
    /// comment — called once per frame from `BackendCore::tick_writers`.
    pub fn take_pending_kline_teardowns(&mut self) -> Vec<PendingKlineTeardown> {
        std::mem::take(&mut self.pending_kline_teardowns)
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
            chart_type_param_mode: mlc_core::chart_type::ParamMode::Fixed,
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
            bars_generation: mlc_core::chart_type::BarsGeneration::NONE,
            footprint_bars: None,
            renko_bars: None,
            three_line_break: None,
            pnf_columns: None,
            kagi_segments: None,
            cvd_line: None,
            tpo_sessions: None,
            oi_delta: None,
            oi_hist: std::collections::BTreeMap::new(),
            liq_projection: None,
            liq_projection_key: (0, 0, 0),
            funding_rate: None,
            dom_heatmap: None,
            dom_trade_dots: None,
            liq_tape: None,
            alerts: Vec::new(),
            alert_next_id: 1,
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            scroll_prefetch_done: false,
            history_depth: crate::bubble::data_bubble::HistoryDepth::default(),
            kline_layers: HashMap::new(),
            kline_layer_capacity: crate::bubble::data_bubble::DEFAULT_KLINE_LAYER_CAPACITY,
            active_fold: None,
            active_trade_fold: None,
            active_cvd_fold: None,
            active_trade_built_kline: None,
            last_refold_at: None,
            refold_dirty: false,
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
            chart_type_param_mode: mlc_core::chart_type::ParamMode::Fixed,
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
            bars_generation: mlc_core::chart_type::BarsGeneration::NONE,
            footprint_bars: None,
            renko_bars: None,
            three_line_break: None,
            pnf_columns: None,
            kagi_segments: None,
            cvd_line: None,
            tpo_sessions: None,
            oi_delta: None,
            oi_hist: std::collections::BTreeMap::new(),
            liq_projection: None,
            liq_projection_key: (0, 0, 0),
            funding_rate: None,
            dom_heatmap: None,
            dom_trade_dots: None,
            liq_tape: None,
            alerts: Vec::new(),
            alert_next_id: 1,
            scroll_fetch_in_flight: false,
            scroll_fetch_started: None,
            scroll_prefetch_done: false,
            history_depth: crate::bubble::data_bubble::HistoryDepth::default(),
            kline_layers: HashMap::new(),
            kline_layer_capacity: crate::bubble::data_bubble::DEFAULT_KLINE_LAYER_CAPACITY,
            active_fold: None,
            active_trade_fold: None,
            active_cvd_fold: None,
            active_trade_built_kline: None,
            last_refold_at: None,
            refold_dirty: false,
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
    ///
    /// Resets `history_depth` to `Unknown` — it reports the depth for
    /// whichever chart_type is CURRENTLY active on the bubble, so a switch
    /// invalidates whatever the previous type's seed reported (see
    /// `HistoryDepth`'s doc comment).
    ///
    /// Also resets `chart_type_param` back to its `0.0` pre-first-pick
    /// sentinel when `ct` actually differs from the bubble's current
    /// `chart_type` (param-spec ladders are per-family — e.g.
    /// `tick_imbalance_bar`'s `min_ticks=50` leftover has no business
    /// surviving a switch to `tick_bar`'s `count` axis). Re-selecting the
    /// SAME type (no-op switch, or a render-only style change already routed
    /// elsewhere) leaves the user's dialed-in param untouched. Without this,
    /// `mlc-app`'s `ChangeChartType` handler read the stale cross-family
    /// value straight into `build_subscribe_params` — the toolbar label
    /// (which independently falls back to the NEW type's own first registry
    /// preset for the sentinel) and the actual subscribed value would
    /// diverge (bug found 2026-07-10, tick_bar showed "1T" but subscribed at
    /// a leftover/hardcoded default instead).
    pub fn set_chart_type(&mut self, bid: BubbleId, ct: &'static str) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        if bubble.chart_type != ct {
            bubble.chart_type_param = 0.0;
            // ATR(14) mode is meaningful only for the type it was computed
            // against — a genuine chart-type change resets it, same rule as
            // `chart_type_param` itself just above (granularity arc, кусок 2).
            bubble.chart_type_param_mode = mlc_core::chart_type::ParamMode::Fixed;
        }
        bubble.chart_type = ct;
        bubble.history_depth = crate::bubble::data_bubble::HistoryDepth::default();
        true
    }

    /// Update the chart-type numeric parameter (range_bar / volume_bar size)
    /// for a bubble known by id directly.
    pub fn set_chart_type_param(&mut self, bid: BubbleId, param: f64) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.chart_type_param = param;
        true
    }

    /// Update the box-size computation mode (`Fixed`/`Atr14`, renko/kagi
    /// only) for a bubble known by id directly. See
    /// [`super::data_bubble::DataBubble::chart_type_param_mode`].
    pub fn set_chart_type_param_mode(&mut self, bid: BubbleId, mode: mlc_core::chart_type::ParamMode) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.chart_type_param_mode = mode;
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
    ///
    /// Resets the bubble's raw kline data layers (`kline_layers`,
    /// unified-data-plane Stage Э1) when the resulting instrument identity
    /// actually differs from the old one — a genuine symbol/exchange/
    /// account_type reassignment means the previously-buffered raw kline
    /// data belongs to a DIFFERENT instrument and must not linger. A no-op
    /// call (identity unchanged) leaves the layers untouched. Does NOT touch
    /// the shared per-instrument `TradeStore` — see its module doc
    /// ("Instrument-switch semantics").
    pub fn set_instrument(
        &mut self,
        bid: BubbleId,
        symbol: String,
        exchange: String,
        account_type: String,
    ) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        let identity_changed = bubble.address.symbol != symbol
            || bubble.address.exchange != exchange
            || bubble.address.account_type != at;
        if identity_changed {
            log_identity_reset(bubble, bid, &exchange, at, &symbol, "set_instrument");
        }
        // Snapshot the OLD key while `bubble` is still the pre-change
        // instrument — see [`PendingKlineTeardown`]'s doc comment.
        let teardown = identity_changed.then(|| PendingKlineTeardown {
            exchange: bubble.address.exchange.clone(),
            account_type: bubble.address.account_type,
            symbol: bubble.address.symbol.clone(),
            timeframe: bubble.timeframe.name.clone(),
        });
        bubble.address.symbol = symbol;
        bubble.address.exchange = exchange;
        bubble.address.account_type = at;
        if identity_changed {
            bubble.reset_raw_layers();
        }
        if let Some(t) = teardown {
            self.pending_kline_teardowns.push(t);
        }
        true
    }

    /// Update exchange and account_type for a bubble known by id directly
    /// (no chart member required). Used by bare/headless bubble creation —
    /// e.g. the daemon's `POST /v1/bubbles` route, which creates a bubble
    /// with no chart attached yet.
    ///
    /// Resets raw data layers on genuine identity change — see
    /// [`Self::set_instrument`].
    pub fn set_bubble_instrument(&mut self, bid: BubbleId, exchange: String, account_type: String) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        let identity_changed = bubble.address.exchange != exchange || bubble.address.account_type != at;
        if identity_changed {
            let symbol = bubble.address.symbol.clone();
            log_identity_reset(bubble, bid, &exchange, at, &symbol, "set_bubble_instrument");
        }
        // Snapshot the OLD key — see [`PendingKlineTeardown`]'s doc comment.
        let teardown = identity_changed.then(|| PendingKlineTeardown {
            exchange: bubble.address.exchange.clone(),
            account_type: bubble.address.account_type,
            symbol: bubble.address.symbol.clone(),
            timeframe: bubble.timeframe.name.clone(),
        });
        bubble.address.exchange = exchange;
        bubble.address.account_type = at;
        if identity_changed {
            bubble.reset_raw_layers();
        }
        if let Some(t) = teardown {
            self.pending_kline_teardowns.push(t);
        }
        true
    }

    /// Update symbol/exchange/account_type/timeframe for a bubble known by id
    /// directly. Covers the "assign full instrument to a freshly-created
    /// bubble" shape used by chart-seed / new-window flows.
    ///
    /// Resets raw data layers on genuine identity change (symbol/exchange/
    /// account_type — NOT timeframe alone) — see [`Self::set_instrument`].
    pub fn set_bid_instrument_full(
        &mut self,
        bid: BubbleId,
        symbol: String,
        exchange: String,
        account_type: String,
        timeframe: Timeframe,
    ) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        let identity_changed = bubble.address.symbol != symbol
            || bubble.address.exchange != exchange
            || bubble.address.account_type != at;
        if identity_changed {
            log_identity_reset(bubble, bid, &exchange, at, &symbol, "set_bid_instrument_full");
        }
        // Snapshot the OLD (instrument, timeframe) key BEFORE either field
        // is overwritten — see [`PendingKlineTeardown`]'s doc comment. Uses
        // `bubble.timeframe`, not the incoming `timeframe` param: the
        // forwarder being retired was subscribed at whatever TF this
        // bubble held for the OLD instrument.
        let teardown = identity_changed.then(|| PendingKlineTeardown {
            exchange: bubble.address.exchange.clone(),
            account_type: bubble.address.account_type,
            symbol: bubble.address.symbol.clone(),
            timeframe: bubble.timeframe.name.clone(),
        });
        bubble.address.symbol = symbol;
        bubble.address.exchange = exchange;
        bubble.address.account_type = at;
        bubble.timeframe = timeframe;
        if identity_changed {
            bubble.reset_raw_layers();
        }
        if let Some(t) = teardown {
            self.pending_kline_teardowns.push(t);
        }
        true
    }

    /// Update just the exchange field for a bubble known by id directly.
    ///
    /// Resets raw data layers on genuine identity change — see
    /// [`Self::set_instrument`].
    pub fn set_exchange_for_bid(&mut self, bid: BubbleId, exchange: String) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        let identity_changed = bubble.address.exchange != exchange;
        if identity_changed {
            let symbol = bubble.address.symbol.clone();
            log_identity_reset(bubble, bid, &exchange, bubble.address.account_type, &symbol, "set_exchange_for_bid");
        }
        // Snapshot the OLD key — see [`PendingKlineTeardown`]'s doc comment.
        let teardown = identity_changed.then(|| PendingKlineTeardown {
            exchange: bubble.address.exchange.clone(),
            account_type: bubble.address.account_type,
            symbol: bubble.address.symbol.clone(),
            timeframe: bubble.timeframe.name.clone(),
        });
        bubble.address.exchange = exchange;
        if identity_changed {
            bubble.reset_raw_layers();
        }
        if let Some(t) = teardown {
            self.pending_kline_teardowns.push(t);
        }
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
    /// Every restored bubble's Elliott wave links are swept here before
    /// insertion (2026-08-14 report, second attempt — see
    /// `crate::bubble::wave_guards`'s own module doc for why this is the
    /// convergence point rather than a caller-side step): a `link`/`follows`
    /// naming an id nothing in the bubble owns is cleared, then the
    /// `subdivided_slots`/`label_collisions`/context-violation caches
    /// `DrawingManager::refresh_wave_guards` owns are filled. This is
    /// unconditional and cannot be skipped by any caller — this function is
    /// the only place a `DataBubbleSnapshot` list turns back into live
    /// bubbles at all.
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
            super::wave_guards::clear_dangling_wave_links(&mut bubble);
            super::wave_guards::refresh_wave_guards(&mut bubble);
            // Bars are never part of a snapshot (`DataBubbleSnapshot` has no
            // OHLC field at all — bars are re-fetched live, never persisted;
            // see this repo's own client-only-data doctrine), so `bubble.bars`
            // is always empty at this exact point and every bar-derived cache
            // resolves to its own safe default here. Still unconditional, for
            // the same reason `wave_guards` above is: this is the one
            // production entry point that turns a snapshot list back into
            // live bubbles, so a type that DOES derive something meaningful
            // from an empty/short bar slice (unlike today's regression trend)
            // gets a defined answer immediately, with no separate step
            // required — the real (non-empty-bars) fill happens the moment
            // this bubble's first `replace_bars`/`set_bars` call lands, which
            // carries the SAME call below.
            super::bar_derived_cache::refresh_bar_derived_caches(&mut bubble);
            super::markup_guards::refresh_markup_guards(&mut bubble);
            // Footprint counterpart of the same sweep — a documented no-op
            // here today (`DataBubbleSnapshot` carries no footprint field
            // either, same client-only-data doctrine as bars), kept for the
            // same "defined answer immediately, no separate step required"
            // reason the OHLC sweep above states, should a snapshot ever
            // grow one.
            super::bar_derived_cache::refresh_footprint_derived_caches(&mut bubble);
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

    /// History depth backing the bubble's currently-active chart_type. See
    /// [`super::data_bubble::HistoryDepth`]. Unknown bubble reads as the
    /// default `Unknown`.
    pub fn history_depth(&self, bid: BubbleId) -> super::data_bubble::HistoryDepth {
        self.bubbles.get(&bid).map(|b| b.history_depth).unwrap_or_default()
    }

    /// Record a seed/deepen depth report for `chart_type` on `bid`. Returns
    /// `false` for an unknown bubble OR when [`DataBubble::set_history_depth`]
    /// rejected the report as stale (chart_type no longer active).
    pub fn set_history_depth(&mut self, bid: BubbleId, chart_type: &str, depth: super::data_bubble::HistoryDepth) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.set_history_depth(chart_type, depth)
    }

    pub fn bars(&self, bid: BubbleId) -> &[mlc_core::Bar] {
        self.bubbles.get(&bid).map(|b| b.bars.as_slice()).unwrap_or(&[])
    }

    /// Stale-array tripwire stamp (K1 lifecycle groundwork, chart-type-
    /// engine-consolidation B2). See
    /// [`mlc_core::chart_type::BarsGeneration`]'s doc comment. Unknown
    /// bubble reads as `BarsGeneration::NONE`.
    pub fn bars_generation(&self, bid: BubbleId) -> mlc_core::chart_type::BarsGeneration {
        self.bubbles.get(&bid).map(|b| b.bars_generation).unwrap_or_default()
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

    /// Per-venue open-interest history row counts.
    pub fn oi_hist_shape(&self, bid: BubbleId) -> Vec<(String, usize)> {
        self.bubbles
            .get(&bid)
            .map(|b| b.oi_hist.iter().map(|(v, r)| (v.clone(), r.len())).collect())
            .unwrap_or_default()
    }

    /// Projected liquidation levels, if the model has run.
    pub fn liq_projection(
        &self,
        bid: BubbleId,
    ) -> Option<&[mlc_core::chart::liquidation_projection::ProjectedLevel]> {
        self.bubbles.get(&bid)?.liq_projection.as_deref()
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

    /// Realized-liquidation tape, if any prints have landed.
    pub fn liq_tape(&self, bid: BubbleId) -> Option<&mlc_core::chart::liquidations::LiquidationTape> {
        self.bubbles.get(&bid)?.liq_tape.as_ref()
    }

    /// Raw kline layer for timeframe `tf`, if any data has been written for
    /// it yet (unified-data-plane Stage Э1). `None` if the bubble is
    /// missing or the timeframe has never been touched.
    pub fn kline_layer(&self, bid: BubbleId, tf: &str) -> Option<&super::raw_layer::RawLayer<mlc_core::Bar>> {
        self.bubbles.get(&bid)?.kline_layer(tf)
    }

    /// Full chronological snapshot of the raw kline layer for timeframe
    /// `tf`, oldest-first. Empty if the bubble is missing or the timeframe
    /// has never been touched — callers distinguish "no layer yet" from
    /// "empty layer" the same way (both read as `Vec::new()`), since the
    /// only legitimate use (kline-family display rebuild in `tick_writers`)
    /// falls back to the event's own bars in either case.
    pub fn kline_layer_snapshot(&self, bid: BubbleId, tf: &str) -> Vec<mlc_core::Bar> {
        self.bubbles.get(&bid)
            .and_then(|b| b.kline_layer(tf))
            .map(|l| l.snapshot())
            .unwrap_or_default()
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

    /// Box-size computation mode (`Fixed`/`Atr14`, renko/kagi only) of a
    /// bubble known by id directly.
    pub fn chart_type_param_mode(&self, bid: BubbleId) -> Option<mlc_core::chart_type::ParamMode> {
        self.bubbles.get(&bid).map(|b| b.chart_type_param_mode)
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
    ///
    /// Deliberately does NOT call [`super::bar_derived_cache::
    /// refresh_bar_derived_caches`] — see that module's own doc for the
    /// pass this feeds. Every production caller of a NEW-candle append
    /// today reaches `DataBubble::append_bar` directly inside a
    /// `with_bubble_mut` closure (`mlc-app`'s live-trade candle builder,
    /// `mlc-backend`'s own fold-output handlers), bypassing this wrapper
    /// entirely — so hooking this method alone would not even cover them.
    /// More importantly, for the fastest derived-bar chart types
    /// (`tick_bar`/`tick_imbalance_bar`/`volume_imbalance_bar`) a bar can
    /// close many times per second under heavy trade volume, which is
    /// exactly the disallowed "full sweep per tick" — a NEW candle append
    /// is a per-bar-close event, not the per-trade-tick one (the
    /// currently-forming bar's own price walk updates in place via
    /// [`DataBubble::last_bar_mut`], which this function never touches
    /// either, and neither does the cache). The cache instead refreshes on
    /// the coarser [`Self::replace_bars`]/[`Self::set_bars`] batch installs
    /// (REST loads, backfills, chart-type switches, symbol/timeframe
    /// changes) and on [`Self::restore_from_snapshots`] — a stat block
    /// reading it can lag an in-flight candle by up to one more
    /// batch-refresh; the render path never depends on this cache at all
    /// (see the trait method's own contract), so that lag is never a
    /// drawing defect, only a modal number that catches up at the next
    /// refresh.
    pub fn append_bar(&mut self, bid: BubbleId, bar: mlc_core::Bar) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.append_bar(bar);
        }
    }

    /// Replace a bubble's entire OHLC series.
    pub fn replace_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            // Lifecycle tripwire (2026-07-17): a populated display going
            // EMPTY through the choke point is a rare, significant event
            // (fresh-load clear, instrument reset, or — historically — a
            // defect). Keep it visible in the debug log permanently.
            if !b.bars.is_empty() && bars.is_empty() {
                mlc_backend_data_bridge::dlog!(
                    "[display-wipe] bid={:?} {}:{}:{} tf={} {} bars -> 0",
                    bid, b.address.exchange, b.address.account_type.as_label(),
                    b.address.symbol, b.timeframe.name, b.bars.len(),
                );
            }
            b.replace_bars(bars);
            // A batch bar-array install — REST load, backfill merge,
            // reseed/refold replay, chart-type switch — is exactly the
            // "bar data updates" trigger `refresh_bar_derived_cache`'s own
            // contract is for; see [`Self::append_bar`]'s own doc for why
            // the narrower per-new-candle append is deliberately NOT hooked.
            super::bar_derived_cache::refresh_bar_derived_caches(b);
            super::markup_guards::refresh_markup_guards(b);
        }
    }

    /// Clear a bubble's entire OHLC series. Mirrors [`Self::replace_bars`]
    /// — both bump [`DataBubble::bars_generation`] (see
    /// [`mlc_core::chart_type::BarsGeneration`]'s doc comment).
    pub fn clear_bars(&mut self, bid: BubbleId) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.clear_bars();
            // Bars going away must clear a stale fit right along with them —
            // an empty `bars` resolves every bar-derived cache back to its
            // own safe default (see [`Self::replace_bars`]'s own doc).
            super::bar_derived_cache::refresh_bar_derived_caches(b);
            super::markup_guards::refresh_markup_guards(b);
        }
    }

    /// Replace a bubble's entire OHLC series (restore/hydration path — same
    /// underlying write as `replace_bars`, distinct name for call-site intent
    /// clarity between "live tick" and "restore").
    pub fn set_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.replace_bars(bars);
            super::bar_derived_cache::refresh_bar_derived_caches(b);
            super::markup_guards::refresh_markup_guards(b);
        }
    }

    // =========================================================================
    // Raw data layer write intents (unified-data-plane Stage Э1)
    // =========================================================================

    /// Live-tick write into a bubble's raw kline layer for timeframe `tf`
    /// (append, or in-place replace of the current last point by
    /// chronological identity). No-op for an unknown bubble.
    pub fn kline_layer_push_live(&mut self, bid: BubbleId, tf: &str, bar: mlc_core::Bar) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.kline_layer_mut(tf).push_live(bar);
        }
    }

    /// Backfill-batch write into a bubble's raw kline layer for timeframe
    /// `tf` — prepended in front of the existing series with seam dedup.
    /// No-op for an unknown bubble.
    pub fn kline_layer_prepend_batch(&mut self, bid: BubbleId, tf: &str, bars: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.kline_layer_mut(tf).prepend_batch(bars);
        }
    }

    /// Phase-A / re-subscribe write into a bubble's raw kline layer for
    /// timeframe `tf` — see [`super::raw_layer::RawLayer::seed_block`]. Resets
    /// the layer to just `block` when it is disjoint from a stale layer tail
    /// (e.g. a `RampState` that survived a TF round-trip and kept extending
    /// a now-orphaned ladder); otherwise grows the layer in place. No-op for
    /// an unknown bubble.
    pub fn kline_layer_seed_block(&mut self, bid: BubbleId, tf: &str, block: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.kline_layer_seed_block(tf, block);
        }
    }

    /// Drop every point from every kline ring on every bubble in this store,
    /// returning the total point count actually dropped. Distinct from
    /// [`super::data_bubble::DataBubble::reset_raw_layers`]'s "genuine
    /// instrument reassignment" contract (module doc on
    /// [`super::raw_layer`]): that restriction protects an ACTIVE bubble's
    /// data from a chart-type/param/TF switch on the same instrument. This
    /// method targets a whole `BubbleStore` extracted into a parked preset
    /// tab's cache entry (`mlc-app::persistence::live_cache::LivePresetState`)
    /// — already off-screen and already excluded from every live write path,
    /// so "the same instrument, still being looked at" does not apply. Used
    /// by the idle-tab release sweep (`ChartApp::release_idle_parked_presets`)
    /// to give a long-parked tab's memory back; the refill on unpark is
    /// unconditional (an empty ring already forces a full refetch), so there
    /// is nothing here that needs preserving.
    pub fn clear_all_kline_layers(&mut self) -> usize {
        let mut dropped = 0usize;
        for bubble in self.bubbles.values_mut() {
            for layer in bubble.kline_layers.values_mut() {
                dropped += layer.len();
                layer.clear();
            }
        }
        dropped
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
            ..Default::default()
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

#[cfg(test)]
mod kline_layer_display_rebuild_tests {
    //! Store-level coverage for the P0 kline-display-corruption fix
    //! (2026-07-09): `tick_writers`' kline-family display writers now
    //! rebuild `bubble.bars` from the raw kline layer instead of writing a
    //! second, disconnected array. These tests exercise the store methods
    //! `tick_writers` actually calls (`kline_layer_seed_block`,
    //! `kline_layer_prepend_batch`, `kline_layer_snapshot`) directly,
    //! without going through the full `LiveUpdate`/`BubbleEvent` plumbing.

    use super::*;
    use mlc_core::Bar;

    fn bar(ts: i64, close: f64) -> Bar {
        Bar {
            timestamp: ts,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
            ..Default::default()
        }
    }

    /// Layer [t0..t100] (via seed) + ScrollBars-style prepend page
    /// [t-50..t0) → the layer snapshot is one contiguous, oldest-first run
    /// covering both — exactly what the `ScrollBarsLoaded` arm's display
    /// rebuild reads.
    #[test]
    fn prepend_into_seeded_layer_yields_contiguous_snapshot() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());

        let seed: Vec<Bar> = (0..=100).step_by(10).map(|t| bar(t, 100.0 + t as f64)).collect();
        store.kline_layer_seed_block(bid, "1H", seed.clone());

        let older_page: Vec<Bar> = (-50..0).step_by(10).map(|t| bar(t, 50.0 + t as f64)).collect();
        store.kline_layer_prepend_batch(bid, "1H", older_page.clone());

        let snapshot = store.kline_layer_snapshot(bid, "1H");
        assert_eq!(snapshot.len(), older_page.len() + seed.len(), "contiguous merge count");
        assert_eq!(snapshot.first().map(|b| b.timestamp), older_page.first().map(|b| b.timestamp));
        assert_eq!(snapshot.last().map(|b| b.timestamp), seed.last().map(|b| b.timestamp));
        // Strictly increasing timestamps — no hole, no duplicate seam.
        for w in snapshot.windows(2) {
            assert!(w[1].timestamp > w[0].timestamp, "snapshot must be strictly chronological, no gap/dup at the seam");
        }
    }

    /// Phase-A on a bubble whose layer has never been touched: the display
    /// writers fall back to the event's own bars (mirrors the
    /// `display_bars.is_empty()` fallback in `tick_writers`).
    #[test]
    fn empty_layer_snapshot_signals_fallback_to_event_bars() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());

        let snapshot = store.kline_layer_snapshot(bid, "1H");
        assert!(snapshot.is_empty(), "untouched layer must read back empty, driving the event-bars fallback");
    }

    /// Phase-A fresh block disjoint from a stale layer tail (e.g. a
    /// `RampState` that survived a chart-type/TF round-trip and kept
    /// extending a now-orphaned ladder months behind): the layer resets to
    /// just the fresh block — no gapped ring, no split display axis.
    #[test]
    fn disjoint_seed_resets_layer_to_fresh_block() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());

        // Stale ladder: hourly bars, step = 3_600_000 ms, newest at t=7_200_000.
        let stale: Vec<Bar> = (0..3).map(|i| bar(i * 3_600_000, 100.0 + i as f64)).collect();
        store.kline_layer_seed_block(bid, "1H", stale);

        // Fresh PhaseA block: "now", months later — a real gap relative to
        // the established hourly step, not a next-page continuation.
        let fresh_base = 3_600_000_i64 * 24 * 90; // ~90 days later
        let fresh: Vec<Bar> = (0..3).map(|i| bar(fresh_base + i * 3_600_000, 200.0 + i as f64)).collect();
        store.kline_layer_seed_block(bid, "1H", fresh.clone());

        let snapshot = store.kline_layer_snapshot(bid, "1H");
        assert_eq!(snapshot.len(), fresh.len(), "disjoint seed must reset, not accumulate a gapped ring");
        assert_eq!(snapshot.first().map(|b| b.timestamp), Some(fresh_base));
    }
}

#[cfg(test)]
mod bars_generation_tests {
    //! Stale-array tripwire coverage (K1 lifecycle groundwork, chart-type-
    //! engine-consolidation B2) — `BarsGeneration` is bumped by every
    //! `replace_bars`/`clear_bars` call and stamped for the bubble's
    //! CURRENT `chart_type`; a switch that clears/replaces after
    //! `set_chart_type` must leave the new type's name on the stamp, and a
    //! stamp that still names the OLD type (the exact bug class this brick
    //! surfaces) is what `mlc-backend::bubble::render::debug_assert_generation`
    //! detects at render time.

    use super::*;
    use mlc_core::chart_type::BarsGeneration;
    use mlc_core::Bar;

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { timestamp: ts, open: close, high: close, low: close, close, volume: 1.0, ..Default::default() }
    }

    #[test]
    fn fresh_bubble_starts_at_none_generation() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        assert_eq!(store.bars_generation(bid), BarsGeneration::NONE);
    }

    #[test]
    fn replace_bars_stamps_generation_for_current_chart_type() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        store.set_chart_type(bid, "range_bar");
        store.replace_bars(bid, vec![bar(1, 1.0)]);
        let gen = store.bars_generation(bid);
        assert_eq!(gen.chart_type, "range_bar");
        assert_eq!(gen.seq, 1);
    }

    #[test]
    fn chart_type_switch_then_replace_bumps_to_new_type_and_seq() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        store.set_chart_type(bid, "range_bar");
        store.replace_bars(bid, vec![bar(1, 1.0)]);
        let gen_before = store.bars_generation(bid);
        assert_eq!(gen_before.chart_type, "range_bar");

        // Switch: clear_slot (clear_bars) THEN set_chart_type THEN reseed
        // (replace_bars) — mirrors the real `ChangeChartType` handler's
        // ordering (`chart_out_events.rs`: set_chart_type precedes the
        // subscribe/reseed dispatch).
        store.clear_bars(bid);
        store.set_chart_type(bid, "volume_bar");
        store.replace_bars(bid, vec![bar(2, 2.0)]);

        let gen_after = store.bars_generation(bid);
        assert_eq!(gen_after.chart_type, "volume_bar");
        assert!(gen_after.seq > gen_before.seq, "generation must be monotonic across a chart-type switch");
        assert_ne!(gen_after, gen_before);
    }

    #[test]
    fn stamp_reflects_bubble_chart_type_at_call_time_not_a_stale_snapshot() {
        // Mirrors the exact bug class `debug_assert_generation` exists to
        // catch: if a `replace_bars` call ever landed BEFORE `set_chart_type`
        // for the new type, the stamp would (correctly) still name the OLD
        // type — proving the stamp is a live snapshot of `chart_type` at
        // bump time, not a value threaded in from the caller.
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        store.set_chart_type(bid, "candles");
        store.replace_bars(bid, vec![bar(1, 1.0)]);
        assert_eq!(store.bars_generation(bid).chart_type, "candles");

        // Simulate the race: bars replaced BEFORE the chart_type flip lands.
        store.replace_bars(bid, vec![bar(2, 2.0)]);
        assert_eq!(
            store.bars_generation(bid).chart_type, "candles",
            "stamp must still read the bubble's chart_type at the time of the call"
        );
        store.set_chart_type(bid, "line");
        assert_eq!(
            store.bars_generation(bid).chart_type, "candles",
            "set_chart_type alone (no bars write) must not retroactively re-stamp the generation"
        );
    }

    #[test]
    fn clear_bars_bumps_generation_too() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());
        store.set_chart_type(bid, "renko");
        store.replace_bars(bid, vec![bar(1, 1.0)]);
        let after_replace = store.bars_generation(bid);
        store.clear_bars(bid);
        let after_clear = store.bars_generation(bid);
        assert!(after_clear.seq > after_replace.seq);
        assert_eq!(after_clear.chart_type, "renko");
    }

    /// Test the `debug_assert_generation` check fn directly (chart-type-
    /// engine-consolidation B4, T2.4) — it is warn-only (dlog, never
    /// panics), so a print-capture assertion isn't feasible; instead this
    /// pins the exact MISMATCH CONDITION the fn is documented to detect
    /// (`gen.chart_type != bubble.chart_type`, the dlog trigger) by
    /// reproducing the race from `stamp_reflects_bubble_chart_type_at_call_time_not_a_stale_snapshot`
    /// and reading the raw `DataBubble` the fn actually inspects.
    #[test]
    fn debug_assert_generation_detects_stale_chart_type_mismatch() {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble("BTCUSDT".to_string(), Timeframe::h1());

        // Fresh bubble, matching generation (chart_type unset == "" both
        // sides via BarsGeneration::NONE) — no mismatch, no panic.
        let fresh = store.bubble(bid).expect("bubble exists");
        crate::bubble::render::debug_assert_generation(fresh);

        // Install "candles" and stamp a real generation for it.
        store.set_chart_type(bid, "candles");
        store.replace_bars(bid, vec![bar(1, 1.0)]);
        let matched = store.bubble(bid).expect("bubble exists");
        assert_eq!(matched.bars_generation.chart_type, matched.chart_type, "sanity: freshly-stamped bubble must match");
        crate::bubble::render::debug_assert_generation(matched); // no panic — matching stamp

        // Switch chart_type WITHOUT a follow-up bars write — the exact
        // stale-array shape: the stamp still names "candles" while the
        // bubble's live chart_type has already moved to "line".
        store.set_chart_type(bid, "line");
        let stale = store.bubble(bid).expect("bubble exists");
        assert_ne!(
            stale.bars_generation.chart_type, stale.chart_type,
            "the exact mismatch debug_assert_generation's dlog branch fires on",
        );
        crate::bubble::render::debug_assert_generation(stale); // must not panic — warn-only tripwire
    }
}

#[cfg(test)]
mod bar_derived_cache_tests {
    //! Coverage for `super::bar_derived_cache::refresh_bar_derived_caches`'s
    //! own call sites in THIS file — `restore_from_snapshots`, `replace_bars`,
    //! `clear_bars` — using `RegressionTrend`, the first primitive that
    //! overrode `Primitive::refresh_bar_derived_cache` (the volume-profile
    //! family — `FixedVolumeProfile`/`AnchoredVolumeProfile` — now overrides
    //! it too, filling their own `cached_profile`, but that pair has no
    //! `&self`-only reader yet for a test like this one to assert against).
    //! Mirrors `mlc-backend`'s own `wave_link_restore_tests` module
    //! (`drawing/manager.rs`) for the restore half: build, snapshot, restore
    //! into a FRESH store, assert with no separate sweep call in the test
    //! itself.

    use super::*;
    use crate::tag::TagManager;
    use mlc_core::Bar;
    use mlc_core::drawing::primitives_v2::channels::regression_trend::RegressionTrend;
    use mlc_core::drawing::primitives_v2::config::{PropertyValue, REGRESSION_RESULT_STATS_ID};
    use mlc_core::ids::SyncMemberId;
    use mlc_core::tag::Tag;

    fn bar(ts: i64, close: f64) -> Bar {
        Bar { timestamp: ts, open: close, high: close, low: close, close, volume: 1.0, ..Default::default() }
    }

    /// Read a primitive's own declared Slope/R² stat rows — `None` if it
    /// never declared the block at all (wrong id, or not a regression trend).
    fn regression_stat_rows(p: &dyn mlc_core::drawing::primitives_v2::Primitive) -> Option<Vec<mlc_core::drawing::primitives_v2::config::StatRow>> {
        p.style_properties().into_iter().find_map(|prop| {
            if prop.id != REGRESSION_RESULT_STATS_ID { return None; }
            match prop.value {
                PropertyValue::Stat(rows) => Some(rows),
                _ => None,
            }
        })
    }

    /// A bubble carrying one `RegressionTrend` anchored across its own
    /// (not-yet-loaded) bar range, joined with an auto tag — the shape
    /// `snapshot_round_trip` below needs. Pushed straight onto
    /// `bubble.primitives`: this feature needs no `DrawingManager`, unlike
    /// the Elliott restore tests (`wave_link_restore_tests`), which need it
    /// for `WaveLink` bookkeeping this primitive family has none of.
    fn build_bubble_with_regression_trend() -> (BubbleStore, TagManager, BubbleId) {
        let mut store = BubbleStore::new();
        let bid = store.create_bubble_auto("BTCUSDT".to_string(), Timeframe::h1());
        let mut tags = TagManager::new();
        let mut tag = Tag::new_auto(bid, [0.0, 0.0, 0.0, 0.0]);
        tag.members.insert(SyncMemberId::Chart(1));
        tags.insert(tag);

        let trend = RegressionTrend::new(0, 1.0, 5_000, 6.0, "#9C27B0");
        store.bubble_mut(bid).expect("bubble exists").primitives.push(Box::new(trend));

        (store, tags, bid)
    }

    /// Snapshot `bid`, joined with its paired tag, round-tripped through
    /// JSON — the exact bytes a preset reload reads back (mirrors
    /// `drawing::manager::wave_link_restore_tests::snapshot_round_trip`).
    fn snapshot_round_trip(store: &BubbleStore, tags: &TagManager, bid: BubbleId) -> Vec<mlc_core::preset::snapshots::DataBubbleSnapshot> {
        let bubble_ref = store.bubble(bid).expect("bubble exists");
        let view = super::super::BubbleView::new(bubble_ref);
        let tag_ref = tags.get(bid).expect("paired tag exists");
        let synced = tags.synced_panels_snapshot();
        let snap = view.to_snapshot(tag_ref, &synced);
        let json = serde_json::to_string(&vec![snap]).expect("snapshot must serialize");
        serde_json::from_str(&json).expect("snapshot must deserialize")
    }

    /// A batch bar-array install (`replace_bars` — REST load, backfill,
    /// chart-type switch) is the "bar data updates" trigger: the declared
    /// stat rows go from unset to the real fitted numbers with no separate
    /// call in the test.
    #[test]
    fn replace_bars_populates_the_declared_stat_rows() {
        let (mut store, _tags, bid) = build_bubble_with_regression_trend();
        let bars: Vec<Bar> = (0..=5).map(|i| bar(i, 1.0 + i as f64)).collect();

        store.replace_bars(bid, bars.clone());

        let b = store.bubble(bid).expect("bubble exists");
        let rows = regression_stat_rows(b.primitives[0].as_ref()).expect("stat block declared");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].value, "+1.00", "slope: price climbs by 1.0/bar over this fixture");
        assert_eq!(rows[1].value, "1.00", "R\u{b2}: a perfectly linear fixture explains all variance");
    }

    /// `clear_bars` must not leave a stale fit behind — the cache resolves
    /// back to the safe default the moment the bars it was computed from
    /// are gone.
    #[test]
    fn clear_bars_resets_the_stat_rows_to_the_safe_default() {
        let (mut store, _tags, bid) = build_bubble_with_regression_trend();
        let bars: Vec<Bar> = (0..=5).map(|i| bar(i, 1.0 + i as f64)).collect();
        store.replace_bars(bid, bars);
        store.clear_bars(bid);

        let b = store.bubble(bid).expect("bubble exists");
        let rows = regression_stat_rows(b.primitives[0].as_ref()).expect("stat block declared");
        assert_eq!(rows[0].value, "+0.00", "slope resets to the fit(&[]) default once bars are gone");
        assert_eq!(rows[1].value, "0.00");
    }

    /// The full production sequence with NO user gesture anywhere in it: a
    /// preset reload (`restore_from_snapshots`, into a FRESH store — no
    /// bars exist yet) immediately followed by the ordinary async data-plane
    /// bar load every reload triggers next (`replace_bars` — never a click,
    /// drag, or settings-modal open). By the time an analyst could possibly
    /// open the settings modal, the stat block already carries the real fit.
    #[test]
    fn restore_then_the_first_bar_load_populates_the_stat_rows_with_no_gesture() {
        let (store, tags, bid) = build_bubble_with_regression_trend();
        let snaps = snapshot_round_trip(&store, &tags, bid);

        let mut restored = BubbleStore::new();
        let restored_ids = restored.restore_from_snapshots(&snaps);
        assert_eq!(restored_ids, vec![bid]);

        // Immediately after restore alone (no bars have arrived yet): the
        // stat block is still declared, its rows read the safe default —
        // never a panic, never a stale leftover, never "not declared at
        // all" — matching this primitive's own bars-empty `fit(&[])`.
        {
            let b = restored.bubble(bid).expect("bubble restored");
            let rows = regression_stat_rows(b.primitives[0].as_ref()).expect("stat block declared even before any bars arrive");
            assert_eq!(rows[0].value, "+0.00");
            assert_eq!(rows[1].value, "0.00");
        }

        // The bars arrive — the ordinary post-reload data-plane event.
        let bars: Vec<Bar> = (0..=5).map(|i| bar(i, 1.0 + i as f64)).collect();
        restored.replace_bars(bid, bars);

        let b = restored.bubble(bid).expect("bubble restored");
        let rows = regression_stat_rows(b.primitives[0].as_ref()).expect("stat block declared");
        assert_eq!(rows[0].value, "+1.00", "no gesture needed — restore + the ordinary bar load alone produced this");
        assert_eq!(rows[1].value, "1.00");
    }

    /// A type that derives nothing (the default hook) survives every one of
    /// these choke points untouched — restore, a bar-array replace, and a
    /// clear — proving the sweep's own unconditional walk over every
    /// primitive costs nothing observable for the overwhelming majority of
    /// types that never override `refresh_bar_derived_cache`.
    #[test]
    fn a_primitive_with_no_override_is_unaffected_by_every_bar_choke_point() {
        use mlc_core::drawing::primitives_v2::lines::trend_line::TrendLine;

        let mut store = BubbleStore::new();
        let bid = store.create_bubble_auto("BTCUSDT".to_string(), Timeframe::h1());
        let line = TrendLine::new(0, 100.0, 5_000, 110.0, "#2196F3");
        let before_json = mlc_core::drawing::primitives_v2::Primitive::to_json(&line);
        store.bubble_mut(bid).expect("bubble exists").primitives.push(Box::new(line));

        store.replace_bars(bid, vec![bar(0, 100.0), bar(1000, 105.0)]);
        store.clear_bars(bid);

        let b = store.bubble(bid).expect("bubble exists");
        assert_eq!(b.primitives[0].to_json(), before_json, "a type with no override must be byte-for-byte unaffected");
    }
}
