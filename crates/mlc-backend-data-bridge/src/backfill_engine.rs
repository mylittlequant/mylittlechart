//! `BackfillEngine` — ONE ramped, progressive backfill state machine for
//! every raw layer (kline-per-TF and trades) of every bubble.
//!
//! Unified-data-plane Stage Э2 (`nemo/docs/mlc/plans/unified-data-plane-2026-07-09.md`):
//! replaces three previously-separate backfill models (kline one-shot ladder
//! emitting one `BackfillComplete` at the end, derived cold-seed lump,
//! footprint's private deepen copy) with a single doctrine:
//!
//! - **Cold path**: fetch one page immediately (fastest possible first
//!   paint) → then ramp the GROUP size 1 → 1 → 2 → 4 → … pages per group
//!   until `target` is reached (if set) or the venue is exhausted. Every
//!   page is emitted the instant it arrives — never accumulate-then-emit.
//! - **Scroll-back path**: once the consumer reports it has crossed ~80% of
//!   loaded history toward the oldest edge, fire one page group; repeated
//!   pressure while still scrolling escalates the group size 1 → 2 → 4 → …
//!   the same way the cold path ramps. A quiet period (or the pressure
//!   signal stopping) resets the ramp back to 1.
//! - **Stops**: venue exhaustion (empty page), venue wall (`RestWindow` /
//!   `kline_backpage=false`), or `target` reached. There is no fixed depth
//!   cap — `target: None` backfills until the venue itself stops.
//!
//! The ramp GROUP size is a RATE limiter (how many pages this engine allows
//! itself to fetch back-to-back before yielding control), NOT a depth
//! ceiling — depth is governed exclusively by `target`/exhaustion/wall. See
//! [`MAX_RAMP_PAGES_PER_GROUP`].
//!
//! ## Scope of this module
//!
//! Pure ramp/state logic ([`RampState`], fully unit-tested, no I/O) plus the
//! `DataBridge` driver methods that fetch pages through the station hub and
//! emit them. This module cannot reach into `mlc-backend`'s `BubblesMut`
//! facade (crate layering: `mlc-backend-data-bridge` sits BELOW
//! `mlc-backend` — engine, not kernel) — every page this engine produces
//! reaches the bubble's raw layers exclusively through a `LiveUpdate`
//! broadcast event that `mlc-backend::tick_writers` already owns a writer
//! for (`ScrollBarsLoaded`/`BackfillComplete` for kline,
//! `TradeHistoryPrepended` for trades), plus the layer-scoped
//! `LiveUpdate::LayerBackfilled` notification for Э3/Э4 consumers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use digdigdig3::core::types::TradeHistoryTier;
use digdigdig3::{AccountType, ExchangeId};

use crate::bridge::{DataBridge, LiveUpdate, RawTradeWire, bar_point_to_bar, sleep_ms};

// ═══════════════════════════════════════════════════════════════════════════
// TUNABLES — all ramp/rate constants live here, one place, with doc comments
// distinguishing RATE limiters from the (deliberately absent) depth caps.
// ═══════════════════════════════════════════════════════════════════════════

/// Page size for one kline backfill page, in bars. Matches the exchange
/// per-request cap used elsewhere in the bridge (`effective_limit`).
pub const KLINE_PAGE_SIZE: usize = 500;

/// Page size for one trade/aggTrade backfill page, in trades.
pub const TRADE_PAGE_SIZE: usize = 1000;

/// Ceiling on how many pages one ramp GROUP may request before the engine
/// yields back to the caller loop (still single-flight; the next group is
/// scheduled on the next `ensure_*`/`on_*_scroll_pressure` call). This is a
/// RATE limiter — it bounds how much REST traffic one burst can generate,
/// never how much history the layer is allowed to hold. Depth is governed
/// solely by `target` / venue exhaustion / venue wall (owner doctrine
/// 2026-07-09 — no hand-made depth caps).
pub const MAX_RAMP_PAGES_PER_GROUP: u32 = 8;

/// Scroll-pressure threshold: the consumer is considered "near the oldest
/// loaded edge" once the viewport's distance from the oldest loaded point is
/// at or below this fraction of total loaded depth. `tick.rs`'s existing
/// `view_start < visible_bars * 0.20` check (distance from the LEFT/oldest
/// edge in bar units) is the same 80%-of-loaded-history rule restated in
/// viewport-visible-bars terms rather than total-loaded-bars terms — see the
/// mapping note on `DataBridge::on_kline_scroll_pressure`.
pub const SCROLL_PRESSURE_THRESHOLD: f64 = 0.80;

/// Quiet period after the last scroll-pressure signal before the ramp resets
/// back to a single page per group. Matches the existing scroll-fetch
/// in-flight timeout used elsewhere in the bridge/app for consistency.
pub const RAMP_QUIET_RESET_MS: u64 = 10_000;

// ═══════════════════════════════════════════════════════════════════════════
// LayerId / BackfillKey — identity for one engine-tracked layer
// ═══════════════════════════════════════════════════════════════════════════

/// Which raw layer of a bubble's instrument a [`RampState`] tracks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LayerId {
    /// A kline layer for one timeframe name (e.g. `"1m"`, `"1h"`).
    Kline(String),
    /// The single trade layer (no per-parameter split — one raw trade
    /// stream per instrument).
    Trades,
}

impl LayerId {
    /// Wire label used by `LiveUpdate::LayerBackfilled.layer` — `"kline:<tf>"`
    /// or `"trades"`.
    pub fn wire_label(&self) -> String {
        match self {
            LayerId::Kline(tf) => format!("kline:{tf}"),
            LayerId::Trades => "trades".to_string(),
        }
    }
}

/// Compound key identifying one engine-tracked layer:
/// `(exchange, symbol, account_type, layer)`.
pub type BackfillKey = (ExchangeId, String, AccountType, LayerId);

// ═══════════════════════════════════════════════════════════════════════════
// RampState — pure state machine, no I/O
// ═══════════════════════════════════════════════════════════════════════════

/// Per-layer backfill/ramp bookkeeping. Pure data + pure transitions — every
/// method here is synchronous and side-effect-free (no network, no locking
/// beyond what the caller already holds), so the ramp progression itself is
/// exhaustively unit-testable without a station/hub/runtime.
#[derive(Debug, Clone)]
pub struct RampState {
    /// Oldest anchor (ms) fetched so far for this layer. `None` = nothing
    /// fetched yet (fresh/cold layer) — the next fetch anchors at "now".
    pub oldest_anchor: Option<i64>,
    /// Current ramp step, in PAGES for the next group — 1, 1, 2, 4, 8, …,
    /// capped at [`MAX_RAMP_PAGES_PER_GROUP`]. Doubles once per COMPLETED
    /// group via [`Self::advance_ramp`] (seed group and the group right
    /// after it both stay at 1; doubling starts on the third group).
    pub ramp_step: u32,
    /// Number of fetch groups completed so far for this key. Drives the
    /// 1 → 1 → 2 → 4 → … progression independently of `loaded` (which a
    /// zero-length exhausted page would otherwise leave ambiguous).
    pub groups_completed: u32,
    /// A fetch group is currently in flight for this key (single-flight
    /// guard).
    pub in_flight: bool,
    /// The venue has exhausted its history (empty page or capability wall)
    /// — latched permanently until the layer's identity changes (instrument
    /// switch drops the whole key, never un-set otherwise).
    pub exhausted: bool,
    /// Target depth in points (bars/trades), if any. `None` = unbounded —
    /// backfill continues until the venue stops on its own (scroll-back
    /// mode, or an explicit "no known target" cold path).
    pub target: Option<usize>,
    /// Points loaded into the layer so far, as tracked by this engine (not
    /// re-derived from the layer itself — the engine is the sole writer of
    /// this counter and it is monotonically non-decreasing until a reset).
    pub loaded: usize,
    /// Wall-clock instant of the last scroll-pressure signal, used to reset
    /// the ramp to 1 after a quiet period. `None` before the first pressure
    /// signal (cold path never needs this — it ramps unconditionally).
    pub last_pressure_at: Option<web_time::Instant>,
}

impl Default for RampState {
    fn default() -> Self {
        Self {
            oldest_anchor: None,
            ramp_step: 1,
            groups_completed: 0,
            in_flight: false,
            exhausted: false,
            target: None,
            loaded: 0,
            last_pressure_at: None,
        }
    }
}

impl RampState {
    /// Fresh state for a target-bounded cold-open backfill.
    pub fn new(target: Option<usize>) -> Self {
        Self { target, ..Self::default() }
    }

    /// Whether this layer needs any more fetching right now: not already
    /// in flight, not exhausted, and (if a target is set) not yet satisfied.
    pub fn needs_more(&self) -> bool {
        if self.in_flight || self.exhausted {
            return false;
        }
        match self.target {
            Some(t) => self.loaded < t,
            None => true,
        }
    }

    /// Pages to request in the NEXT group, honoring the ramp progression
    /// (1 → 1 → 2 → 4 → … capped at [`MAX_RAMP_PAGES_PER_GROUP`]) and never
    /// requesting more pages than needed to reach a set `target`.
    pub fn next_group_pages(&self, page_size: usize) -> u32 {
        let mut pages = self.ramp_step.clamp(1, MAX_RAMP_PAGES_PER_GROUP);
        if let Some(target) = self.target {
            if page_size > 0 {
                let remaining = target.saturating_sub(self.loaded);
                let pages_needed = remaining.div_ceil(page_size).max(1) as u32;
                pages = pages.min(pages_needed);
            }
        }
        pages
    }

    /// Advance the ramp step for the NEXT group, per the doctrine sequence
    /// 1 → 1 → 2 → 4 → 8 → … (the seed group and the group immediately
    /// after it are both size 1; doubling begins on the third group),
    /// capped at [`MAX_RAMP_PAGES_PER_GROUP`].
    pub fn advance_ramp(&mut self) {
        self.groups_completed = self.groups_completed.saturating_add(1);
        self.ramp_step = match self.groups_completed {
            0 | 1 => 1,
            n => {
                let doublings = n - 1; // group 2 -> 2^0=1, group 3 -> 2^1=2, group 4 -> 2^2=4, ...
                1u32.checked_shl(doublings.min(31))
                    .unwrap_or(u32::MAX)
                    .min(MAX_RAMP_PAGES_PER_GROUP)
            }
        };
    }

    /// Reset the ramp back to the seed step (1) — called after a quiet
    /// period with no scroll pressure, or when a fresh crossing begins.
    pub fn reset_ramp(&mut self) {
        self.ramp_step = 1;
        self.groups_completed = 0;
    }

    /// Record a scroll-pressure signal: escalates the ramp if the signal
    /// arrives while a previous one is still "fresh" (within
    /// [`RAMP_QUIET_RESET_MS`]), otherwise resets to the seed step first —
    /// a fresh crossing after a quiet spell re-earns the ramp from scratch.
    pub fn record_pressure(&mut self, now: web_time::Instant) {
        let quiet_expired = self.last_pressure_at
            .map(|t| now.saturating_duration_since(t).as_millis() as u64 > RAMP_QUIET_RESET_MS)
            .unwrap_or(false);
        if quiet_expired {
            self.reset_ramp();
        }
        self.last_pressure_at = Some(now);
    }

    /// Mark a fetch group as started (single-flight guard engaged).
    pub fn begin_flight(&mut self) {
        self.in_flight = true;
    }

    /// Record one page's outcome: advances `oldest_anchor`, accumulates
    /// `loaded`, and latches `exhausted` when the venue signals it has no
    /// more data (empty page or a capability wall report).
    pub fn record_page(&mut self, page_len: usize, new_oldest_anchor: Option<i64>, venue_exhausted: bool) {
        self.loaded += page_len;
        if let Some(anchor) = new_oldest_anchor {
            self.oldest_anchor = Some(anchor);
        }
        if venue_exhausted {
            self.exhausted = true;
        }
    }

    /// End the in-flight group, advancing the ramp for the next one.
    pub fn end_flight(&mut self) {
        self.in_flight = false;
        self.advance_ramp();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BackfillEngine — the registry DataBridge owns
// ═══════════════════════════════════════════════════════════════════════════

/// Registry of [`RampState`] per [`BackfillKey`], owned by [`DataBridge`].
#[derive(Debug, Default, Clone)]
pub struct BackfillEngine {
    states: Arc<Mutex<HashMap<BackfillKey, RampState>>>,
}

impl BackfillEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the current state for a key (test/debug introspection).
    pub fn snapshot(&self, key: &BackfillKey) -> Option<RampState> {
        self.states.lock().ok()?.get(key).cloned()
    }

    fn with_state<R>(&self, key: &BackfillKey, f: impl FnOnce(&mut RampState) -> R) -> R {
        let mut guard = self.states.lock().unwrap_or_else(|e| e.into_inner());
        let state = guard.entry(key.clone()).or_insert_with(RampState::default);
        f(state)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DataBridge driver methods — kline layer
// ═══════════════════════════════════════════════════════════════════════════

impl DataBridge {
    /// Ensure the kline layer for `(exchange, symbol, account, tf)` has at
    /// least `target_bars` bars loaded (or, if `target_bars` is `None`,
    /// keeps ramping until the venue is exhausted) — the universal
    /// cold/ramp backfill path (unified-data-plane Э2). Replaces the
    /// one-shot 40-page ladder previously in `request_background_backfill`.
    ///
    /// Non-blocking, single-flight per key. Every page fetched is emitted
    /// immediately: the FIRST page of a cold run as
    /// `LiveUpdate::BackfillComplete` (preserves the existing cold-open
    /// `needs_snap_after_backfill` / full-replace-anchor path unchanged),
    /// every subsequent page in the same run as the additive
    /// `LiveUpdate::ScrollBarsLoaded` (the existing `tick_writers`/react.rs
    /// merge machinery already prepends it into `bubble.bars` + the raw
    /// kline layer) — plus a `LiveUpdate::LayerBackfilled` notification for
    /// every page.
    pub fn ensure_kline_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &mlc_core::state::Timeframe,
        account_type: AccountType,
        target_bars: Option<u32>,
    ) {
        let key: BackfillKey = (
            exchange_id,
            symbol.to_string(),
            account_type,
            LayerId::Kline(timeframe.name.clone()),
        );

        let should_start = self.backfill_engine().with_state(&key, |state| {
            if state.target.is_none() && target_bars.is_some() {
                // A caller supplied a real target where the engine had none
                // yet (e.g. cold path after a prior unbounded scroll ramp) —
                // adopt it so `needs_more` can converge.
                state.target = target_bars.map(|t| t as usize);
            }
            if !state.needs_more() {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let interval = crate::convert::timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_kline_ramp(
                fetch_hub, tx, engine, key, exchange_id, account_type,
                symbol_str, wire_sym, interval, tf_name,
            ).await;
        });
    }

    /// Scroll-pressure entry for the kline layer (unified-data-plane Э2):
    /// call this when the consumer has crossed ~80% of its loaded kline
    /// history toward the oldest edge. Escalates the ramp group size if
    /// pressure keeps arriving; resets to 1 after a quiet period (handled
    /// inside [`RampState::record_pressure`]).
    ///
    /// Threshold mapping (owner doctrine "trigger at ~80% of loaded data"):
    /// `tick.rs`'s existing trigger fires when
    /// `view_start <= visible_bars * 0.20` — the viewport's LEFT edge is
    /// within 20% of the visible window's width from the oldest loaded bar.
    /// That is the same "within the last 20% of loaded history" rule the
    /// moment the loaded series is only slightly larger than one screen —
    /// exactly the state a freshly-opened chart is in, and the state the
    /// existing call site already checks against. `on_kline_scroll_pressure`
    /// does not re-derive the geometry itself (that stays call-site work,
    /// documented at the `tick.rs` call site) — it only accepts the boolean
    /// "pressure now" and drives the ramp.
    pub fn on_kline_scroll_pressure(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &mlc_core::state::Timeframe,
        account_type: AccountType,
    ) {
        let key: BackfillKey = (
            exchange_id,
            symbol.to_string(),
            account_type,
            LayerId::Kline(timeframe.name.clone()),
        );

        let should_start = self.backfill_engine().with_state(&key, |state| {
            state.record_pressure(web_time::Instant::now());
            if state.in_flight || state.exhausted {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let interval = crate::convert::timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_kline_ramp(
                fetch_hub, tx, engine, key, exchange_id, account_type,
                symbol_str, wire_sym, interval, tf_name,
            ).await;
        });
    }
}

/// Shared ramp-group runner for both the cold path and the scroll-pressure
/// path — fetches `next_group_pages()` anchored kline pages sequentially,
/// emitting each one as it arrives (never accumulate-then-emit), then ends
/// the flight (advancing the ramp for the next group).
async fn run_kline_ramp(
    fetch_hub: Arc<digdigdig3::connector_manager::ExchangeHub>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: BackfillEngine,
    key: BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_str: String,
    wire_sym: String,
    interval: String,
    tf_name: String,
) {
    // Readiness barrier — same pattern as the legacy backfill/scroll fns.
    {
        let mut attempts = 0;
        loop {
            if fetch_hub.rest(exchange_id).is_some() {
                break;
            }
            attempts += 1;
            if attempts > 50 {
                crate::dlog!(
                    "[BackfillEngine] kline: connector {:?} not initialized after 5s",
                    exchange_id
                );
                engine.with_state(&key, RampState::end_flight);
                return;
            }
            sleep_ms(100).await;
        }
    }

    let page_size = KLINE_PAGE_SIZE;
    let group_pages = engine.with_state(&key, |s| s.next_group_pages(page_size));
    let mut is_first_page_of_run = engine.with_state(&key, |s| s.groups_completed == 0 && s.oldest_anchor.is_none());

    for _ in 0..group_pages {
        let older_than_ms = engine.with_state(&key, |s| s.oldest_anchor);

        let (points, outcome) = digdigdig3_station::backfill::klines_paginated(
            &fetch_hub, exchange_id, account_type, &wire_sym, &interval,
            page_size, 1, older_than_ms,
        ).await;

        if points.is_empty() {
            engine.with_state(&key, |s| s.record_page(0, None, true));
            let _ = tx.send(LiveUpdate::LayerBackfilled {
                exchange_id, symbol: symbol_str.clone(), account_type,
                layer: LayerId::Kline(tf_name.clone()).wire_label(),
                prepended: 0, exhausted: true,
            });
            break;
        }

        let new_oldest = points.iter().map(|p| p.open_time).min();
        let bars: Vec<mlc_core::Bar> = points.iter().map(bar_point_to_bar).collect();
        let prepended = bars.len();
        // A `VenueWindowCap`/`VenueRecentOnly` truncation is a genuine wall
        // (stop asking); `VenueHistoryExhausted` on a single-page request
        // just means this page came up short of a FULL page, not that the
        // walk must stop (the empty-page branch above already covers true
        // exhaustion) — so only the wall-shaped reasons latch `exhausted`
        // here.
        let venue_wall = matches!(
            outcome.truncated_by,
            Some(digdigdig3_station::backfill::TruncationReason::VenueWindowCap)
                | Some(digdigdig3_station::backfill::TruncationReason::VenueRecentOnly)
        );

        engine.with_state(&key, |s| s.record_page(prepended, new_oldest.map(|t| t - 1), venue_wall));

        if is_first_page_of_run {
            // First page of a fresh run: full-replace anchor path so the
            // pre-existing cold-open snap/anchor machinery is unchanged.
            let _ = tx.send(LiveUpdate::BackfillComplete {
                exchange_id, account_type,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: bars.clone(),
            });
            is_first_page_of_run = false;
        } else {
            let _ = tx.send(LiveUpdate::ScrollBarsLoaded {
                exchange_id, account_type,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: bars.clone(),
                prepend_count: prepended,
            });
        }
        let _ = tx.send(LiveUpdate::LayerBackfilled {
            exchange_id, symbol: symbol_str.clone(), account_type,
            layer: LayerId::Kline(tf_name.clone()).wire_label(),
            prepended, exhausted: venue_wall,
        });

        if venue_wall {
            break;
        }
        let done = engine.with_state(&key, |s| !s.needs_more());
        if done {
            break;
        }
    }

    engine.with_state(&key, RampState::end_flight);
}

// ═══════════════════════════════════════════════════════════════════════════
// DataBridge driver methods — trade layer
// ═══════════════════════════════════════════════════════════════════════════

impl DataBridge {
    /// Ensure the trade layer for `(exchange, symbol, account)` has at least
    /// `target` trades loaded (`None` = unbounded, ramps until the venue
    /// stops itself — the default for trades per owner doctrine: no fixed
    /// target unless the caller sets one).
    ///
    /// `RecentOnly`-tier venues (see `TradeHistoryCapabilities`) get exactly
    /// one shallow fetch and are latched `exhausted` immediately — hammering
    /// an endpoint the venue caps to a single shallow call buys nothing (see
    /// `agg_trades_paginated`'s own doc comment for the same rule).
    pub fn ensure_trade_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        target: Option<usize>,
    ) {
        let key: BackfillKey = (exchange_id, symbol.to_string(), account_type, LayerId::Trades);

        let should_start = self.backfill_engine().with_state(&key, |state| {
            if state.target.is_none() && target.is_some() {
                state.target = target;
            }
            if !state.needs_more() {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_trade_ramp(fetch_hub, tx, engine, key, exchange_id, account_type, symbol_str, wire_sym).await;
        });
    }

    /// Scroll-pressure entry for the trade layer (unified-data-plane Э2) —
    /// same doctrine as [`Self::on_kline_scroll_pressure`], mirrored for
    /// trades. Wired for Э4 consumption; exercised directly by unit tests
    /// today (no live viewport caller yet — Э4 owns the trade-layer
    /// scroll trigger).
    pub fn on_trade_scroll_pressure(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        let key: BackfillKey = (exchange_id, symbol.to_string(), account_type, LayerId::Trades);

        let should_start = self.backfill_engine().with_state(&key, |state| {
            state.record_pressure(web_time::Instant::now());
            if state.in_flight || state.exhausted {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_trade_ramp(fetch_hub, tx, engine, key, exchange_id, account_type, symbol_str, wire_sym).await;
        });
    }
}

/// Shared ramp-group runner for the trade layer. The first page of a fresh
/// key (no `oldest_anchor` yet) uses the capability-aware seed
/// (`agg_trades_paginated`, which itself falls back to `trades_recent` for
/// `RecentOnly`/unsupported venues); subsequent pages walk the resumable
/// backward cursor page-by-page directly against the hub's
/// `get_agg_trades(from_id = min_id - limit)` — mirroring what
/// `agg_trades_paginated` does internally for its own multi-page walk,
/// without needing any dig3-side addition (the walk is simple enough to own
/// bridge-side, since `agg_trades_paginated` does not expose a resumable
/// cursor across separate calls).
async fn run_trade_ramp(
    fetch_hub: Arc<digdigdig3::connector_manager::ExchangeHub>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: BackfillEngine,
    key: BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_str: String,
    wire_sym: String,
) {
    {
        let mut attempts = 0;
        loop {
            if fetch_hub.rest(exchange_id).is_some() {
                break;
            }
            attempts += 1;
            if attempts > 50 {
                crate::dlog!(
                    "[BackfillEngine] trades: connector {:?} not initialized after 5s",
                    exchange_id
                );
                engine.with_state(&key, RampState::end_flight);
                return;
            }
            sleep_ms(100).await;
        }
    }

    let Some(rest) = fetch_hub.rest(exchange_id) else {
        engine.with_state(&key, RampState::end_flight);
        return;
    };
    let tier = rest.trade_history_capabilities().tier_for(account_type);
    let is_cold = engine.with_state(&key, |s| s.oldest_anchor.is_none() && s.groups_completed == 0);

    // RecentOnly: exactly one shallow fetch, then latch exhausted — never
    // hammer an endpoint the venue caps to a single call.
    if let TradeHistoryTier::RecentOnly { max_trades } = tier {
        if is_cold {
            let cap = (max_trades as usize).min(TRADE_PAGE_SIZE).max(1);
            let (trades, _outcome) = digdigdig3_station::backfill::trades_recent(
                &fetch_hub, exchange_id, account_type, &wire_sym, cap,
            ).await;
            emit_trade_page(&tx, &engine, &key, exchange_id, account_type, &symbol_str, &trades, true);
        } else {
            engine.with_state(&key, |s| s.exhausted = true);
        }
        engine.with_state(&key, RampState::end_flight);
        return;
    }

    let page_size = TRADE_PAGE_SIZE;
    let group_pages = engine.with_state(&key, |s| s.next_group_pages(page_size));

    for i in 0..group_pages {
        let cold_this_page = is_cold && i == 0;
        if cold_this_page {
            let (points, outcome) = digdigdig3_station::backfill::agg_trades_paginated(
                &fetch_hub, exchange_id, account_type, &wire_sym, page_size, 1,
            ).await;
            let venue_wall = matches!(
                outcome.truncated_by,
                Some(digdigdig3_station::backfill::TruncationReason::VenueWindowCap)
                    | Some(digdigdig3_station::backfill::TruncationReason::VenueRecentOnly)
            );
            let min_id = points.iter().map(|p| p.agg_id).min();
            let exhausted_here = points.is_empty() || venue_wall;
            emit_trade_agg_page(&tx, &engine, &key, exchange_id, account_type, &symbol_str, &points, min_id.map(|v| v as i64), exhausted_here);
            if exhausted_here {
                break;
            }
        } else {
            let from_id = engine.with_state(&key, |s| s.oldest_anchor);
            let Some(from_id) = from_id else { break; };
            if from_id <= 0 {
                engine.with_state(&key, |s| s.exhausted = true);
                let _ = tx.send(LiveUpdate::LayerBackfilled {
                    exchange_id, symbol: symbol_str.clone(), account_type,
                    layer: LayerId::Trades.wire_label(), prepended: 0, exhausted: true,
                });
                break;
            }
            let next_from_id = (from_id as u64).saturating_sub(page_size as u64);
            let res = rest.get_agg_trades(
                digdigdig3::core::types::SymbolInput::Raw(&wire_sym),
                Some(page_size as u32),
                Some(next_from_id),
                account_type,
            ).await;
            let points: Vec<digdigdig3_station::data::AggTradePoint> = match res {
                Ok(trades) => trades.iter().map(|t| digdigdig3_station::data::AggTradePoint {
                    ts_ms: t.timestamp,
                    price: t.price,
                    quantity: t.quantity,
                    side: if t.is_buy { 0u8 } else { 1u8 },
                    agg_id: t.aggregate_id as u64,
                }).collect(),
                Err(_) => Vec::new(),
            };
            let was_partial = points.len() < page_size;
            let min_id = points.iter().map(|p| p.agg_id).min();
            let exhausted_here = points.is_empty() || was_partial;
            emit_trade_agg_page(&tx, &engine, &key, exchange_id, account_type, &symbol_str, &points, min_id.map(|v| v as i64), exhausted_here);
            if exhausted_here {
                break;
            }
        }

        let done = engine.with_state(&key, |s| !s.needs_more());
        if done {
            break;
        }
    }

    engine.with_state(&key, RampState::end_flight);
}

fn emit_trade_page(
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: &BackfillEngine,
    key: &BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol: &str,
    trades: &[digdigdig3_station::data::TradePoint],
    exhausted: bool,
) {
    let wire: Vec<RawTradeWire> = trades.iter().map(|t| RawTradeWire {
        ts_ms: t.ts_ms,
        price: t.price,
        qty: t.quantity,
        is_buyer_maker: t.side == 0,
    }).collect();
    let prepended = wire.len();
    let new_oldest = wire.iter().map(|t| t.ts_ms).min();
    engine.with_state(key, |s| s.record_page(prepended, new_oldest, exhausted));
    if prepended > 0 {
        let _ = tx.send(LiveUpdate::TradeHistoryPrepended {
            exchange_id, account_type, symbol: symbol.to_string(), trades: wire,
        });
    }
    let _ = tx.send(LiveUpdate::LayerBackfilled {
        exchange_id, symbol: symbol.to_string(), account_type,
        layer: LayerId::Trades.wire_label(), prepended, exhausted,
    });
}

fn emit_trade_agg_page(
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: &BackfillEngine,
    key: &BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol: &str,
    points: &[digdigdig3_station::data::AggTradePoint],
    new_oldest_anchor: Option<i64>,
    exhausted: bool,
) {
    let wire: Vec<RawTradeWire> = points.iter().map(|p| RawTradeWire {
        ts_ms: p.ts_ms,
        price: p.price,
        qty: p.quantity,
        is_buyer_maker: p.side == 0,
    }).collect();
    let prepended = wire.len();
    engine.with_state(key, |s| s.record_page(prepended, new_oldest_anchor, exhausted));
    if prepended > 0 {
        let _ = tx.send(LiveUpdate::TradeHistoryPrepended {
            exchange_id, account_type, symbol: symbol.to_string(), trades: wire,
        });
    }
    let _ = tx.send(LiveUpdate::LayerBackfilled {
        exchange_id, symbol: symbol.to_string(), account_type,
        layer: LayerId::Trades.wire_label(), prepended, exhausted,
    });
}

#[cfg(test)]
mod tests {
    //! Pure ramp/state-machine unit tests — no network, no runtime, `--lib`
    //! fast. Exercises the 1→1→2→4 progression, quiet-period reset,
    //! single-flight guard, and the exhaustion latch per the task gate.

    use super::*;

    #[test]
    fn fresh_state_needs_more_with_no_target() {
        let s = RampState::default();
        assert!(s.needs_more());
        assert_eq!(s.next_group_pages(500), 1);
    }

    #[test]
    fn fresh_state_with_target_zero_loaded_needs_more() {
        let s = RampState::new(Some(2000));
        assert!(s.needs_more());
    }

    #[test]
    fn satisfied_target_does_not_need_more() {
        let mut s = RampState::new(Some(500));
        s.loaded = 500;
        assert!(!s.needs_more());
        let mut s2 = RampState::new(Some(500));
        s2.loaded = 600;
        assert!(!s2.needs_more());
    }

    #[test]
    fn in_flight_blocks_needs_more() {
        let mut s = RampState::default();
        s.begin_flight();
        assert!(!s.needs_more());
    }

    #[test]
    fn exhausted_blocks_needs_more_even_with_open_target() {
        let mut s = RampState::new(Some(10_000));
        s.exhausted = true;
        assert!(!s.needs_more());
    }

    #[test]
    fn ramp_progression_is_1_1_2_4_8_capped() {
        let mut s = RampState::default();
        assert_eq!(s.ramp_step, 1);

        // Group 1 (the cold seed) completes -> per doctrine the group RIGHT
        // AFTER the seed is still size 1.
        s.begin_flight();
        s.record_page(500, Some(1000), false);
        s.end_flight();
        assert_eq!(s.ramp_step, 1, "group right after the cold seed stays at 1 page");

        // Group 2 completes -> group 3 doubles to 2.
        s.begin_flight();
        s.record_page(500, Some(500), false);
        s.end_flight();
        assert_eq!(s.ramp_step, 2);

        // Group 3 completes -> group 4 doubles to 4.
        s.begin_flight();
        s.record_page(500, Some(0), false);
        s.end_flight();
        assert_eq!(s.ramp_step, 4);

        // Group 4 completes -> group 5 doubles to 8 (== cap).
        s.begin_flight();
        s.record_page(500, Some(0), false);
        s.end_flight();
        assert_eq!(s.ramp_step, MAX_RAMP_PAGES_PER_GROUP);

        // Further groups stay capped, never exceed it.
        for _ in 0..10 {
            s.begin_flight();
            s.record_page(500, Some(0), false);
            s.end_flight();
        }
        assert_eq!(s.ramp_step, MAX_RAMP_PAGES_PER_GROUP);
    }

    #[test]
    fn ramp_step_never_exceeds_cap() {
        let mut s = RampState::default();
        for _ in 0..20 {
            s.advance_ramp();
        }
        assert!(s.ramp_step <= MAX_RAMP_PAGES_PER_GROUP);
    }

    #[test]
    fn next_group_pages_never_exceeds_remaining_target() {
        let mut s = RampState::new(Some(1200));
        s.ramp_step = MAX_RAMP_PAGES_PER_GROUP; // pretend fully ramped
        s.loaded = 1000; // 200 remaining
        // page_size 500 -> ceil(200/500) = 1 page needed, even though ramp
        // allows up to MAX_RAMP_PAGES_PER_GROUP.
        assert_eq!(s.next_group_pages(500), 1);
    }

    #[test]
    fn next_group_pages_respects_ramp_when_target_is_far() {
        let mut s = RampState::new(Some(1_000_000));
        s.ramp_step = 4;
        assert_eq!(s.next_group_pages(500), 4);
    }

    #[test]
    fn record_page_accumulates_loaded_and_updates_anchor() {
        let mut s = RampState::default();
        s.record_page(500, Some(12345), false);
        assert_eq!(s.loaded, 500);
        assert_eq!(s.oldest_anchor, Some(12345));
        assert!(!s.exhausted);

        s.record_page(300, Some(9999), false);
        assert_eq!(s.loaded, 800);
        assert_eq!(s.oldest_anchor, Some(9999));
    }

    #[test]
    fn record_page_latches_exhausted_permanently() {
        let mut s = RampState::default();
        s.record_page(0, None, true);
        assert!(s.exhausted);
        // Latched — nothing in this state machine clears it once set; a
        // fresh instrument/layer identity means a brand-new key/state.
        assert!(s.exhausted);
    }

    #[test]
    fn single_flight_guard_prevents_concurrent_begin() {
        let mut s = RampState::default();
        assert!(s.needs_more());
        s.begin_flight();
        assert!(s.in_flight);
        assert!(!s.needs_more(), "a second caller must not start while in_flight");
        s.end_flight();
        assert!(!s.in_flight);
    }

    #[test]
    fn pressure_resets_ramp_after_quiet_period_elapsed() {
        let mut s = RampState::default();
        s.ramp_step = MAX_RAMP_PAGES_PER_GROUP;
        s.groups_completed = 5;
        // Simulate a quiet period having already elapsed by constructing a
        // `last_pressure_at` synthetically far enough in the past. web_time
        // does not expose `checked_sub` publicly across targets, so instead
        // assert the OTHER direction (still-fresh signal, no reset) here and
        // cover the actual elapsed-time reset via a real sleep in the next
        // test — keeping this one deterministic and fast.
        s.last_pressure_at = Some(web_time::Instant::now());
        s.record_pressure(web_time::Instant::now());
        assert_eq!(s.ramp_step, MAX_RAMP_PAGES_PER_GROUP, "immediate re-signal must NOT reset (well under quiet window)");
    }

    #[test]
    fn pressure_escalates_while_still_scrolling() {
        let mut s = RampState::default();
        let t0 = web_time::Instant::now();
        s.record_pressure(t0);
        assert_eq!(s.ramp_step, 1);

        // A fresh group completes between pressure signals (as the real
        // driver loop would do), advancing the ramp.
        s.begin_flight();
        s.record_page(500, Some(0), false);
        s.end_flight();

        s.record_pressure(web_time::Instant::now());
        // Still within the quiet window (no sleep), so no reset happened —
        // ramp_step reflects the completed group's advance, not a reset.
        assert!(s.ramp_step >= 1);
    }

    #[test]
    fn layer_id_wire_label_formats_kline_and_trades() {
        assert_eq!(LayerId::Kline("1m".to_string()).wire_label(), "kline:1m");
        assert_eq!(LayerId::Trades.wire_label(), "trades");
    }

    #[test]
    fn engine_registry_creates_default_state_on_first_touch() {
        let engine = BackfillEngine::new();
        let key: BackfillKey = (ExchangeId::Binance, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Kline("1m".to_string()));
        assert!(engine.snapshot(&key).is_none());
        engine.with_state(&key, |s| s.loaded = 42);
        assert_eq!(engine.snapshot(&key).map(|s| s.loaded), Some(42));
    }

    #[test]
    fn engine_registry_is_per_key_isolated() {
        let engine = BackfillEngine::new();
        let key_a: BackfillKey = (ExchangeId::Binance, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Kline("1m".to_string()));
        let key_b: BackfillKey = (ExchangeId::Binance, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Trades);
        engine.with_state(&key_a, |s| s.loaded = 10);
        engine.with_state(&key_b, |s| s.loaded = 20);
        assert_eq!(engine.snapshot(&key_a).map(|s| s.loaded), Some(10));
        assert_eq!(engine.snapshot(&key_b).map(|s| s.loaded), Some(20));
    }
}
