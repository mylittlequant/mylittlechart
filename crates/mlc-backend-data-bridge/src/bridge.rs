//! `DataBridge` — live-data bridge backed by digdigdig3-station.
//!
//! ## Runtime ownership (2026-06-24 centralised fix)
//!
//! `DataBridge::new()` is async, so it runs inside SOME tokio runtime —
//! the caller's. The bridge captures that runtime's `Handle` at
//! construction time and stores it in `self.runtime_handle`. Every
//! background task `DataBridge` spawns goes through `self.spawn_bg(...)` /
//! `self.spawn_bg_join(...)`, which `enter()` the captured handle around
//! the `tokio::spawn` call. This way callers do NOT have to maintain a
//! tokio context around `bridge.*` invocations — the bridge is a sync
//! façade from the caller's point of view, and all knowledge of tokio
//! dies on the spawn-helper boundary.
//!
//! Before this fix the spawn helpers were free functions that
//! unconditionally called `tokio::spawn`, forcing every caller path
//! (winit `window_event`, `about_to_wait`, `user_event`, `resumed`,
//! `start_promote_async`, `mlc-app-shell-headless::main`, alert delivery, agent
//! server) to wrap calls in `let _g = handle.enter();` — a forgotten
//! one anywhere crashed with "no reactor running". The wasm cutover
//! (commit 491f9576) made it worse by removing the runtime from
//! `DataBridge` without re-establishing the cfg-split here.
//!
//! On wasm32 the helpers map to `wasm_bindgen_futures::spawn_local`,
//! which needs no runtime context — `runtime_handle` does not exist on
//! that target.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use digdigdig3_station::SeriesKey as StationSeriesKey;
use digdigdig3_station::series::Kind as StationKind;
use digdigdig3_station::series::Series as StationSeries;
use digdigdig3_station::data::{BarPoint, TradePoint};

/// Type alias for the public trade-series handle returned by `subscribe_trades`.
///
/// After Phase 2B step 2 (trades), panels hold the station's own
/// `Arc<tokio::sync::RwLock<Series<TradePoint>>>` directly; the mlc-side
/// `TradeSeries` wrapper is gone.
pub type SharedTradeSeries = Arc<tokio::sync::RwLock<StationSeries<TradePoint>>>;

/// Type alias for the per-key live orderbook reconstructor returned by `subscribe_orderbook`.
///
/// After Phase 2B step 3 (orderbook), panels hold the station's own
/// `Arc<tokio::sync::RwLock<OrderBookTracker>>` directly; the mlc-side
/// `OrderbookSeries` wrapper is gone.
pub type SharedOrderBookTracker = Arc<tokio::sync::RwLock<digdigdig3_station::orderbook::OrderBookTracker>>;

/// Shared per-instrument footprint series.
///
/// The bridge owns one `Vec<FootprintPoint>` per `(exchange, account, symbol)`.
/// The station's derived forwarder for `Stream::Footprint` re-emits each event
/// through the broadcast channel; the bridge's forwarder writes the point into
/// this Vec BEFORE broadcasting so the render-side reader always sees the bar
/// by the time the `FootprintUpdate` event is observed.
pub type SharedFootprintSeries = Arc<tokio::sync::RwLock<Vec<digdigdig3_station::data::FootprintPoint>>>;

use tokio::sync::{broadcast, mpsc};

use digdigdig3::{
    ExchangeId, AccountType, SymbolInfo,
    Order, Balance, Position,
    TradingCapabilities, AccountCapabilities,
    ExchangeError,
};
use digdigdig3::core::{
    OrderRequest, CancelRequest, PlaceOrderResponse, OrderHistoryFilter,
    BalanceQuery, AccountInfo, PositionQuery, PositionModification,
    CancelScope, CancelAllResponse, AmendRequest, OrderResult,
};
use digdigdig3::connector_manager::ExchangeHub;
use mlc_core::Bar;
use mlc_core::state::Timeframe;

use digdigdig3::core::websocket::KlineInterval;
use crate::convert::timeframe_to_interval;

/// Deep warm-seed depth for derived-chart-type subscriptions on native
/// targets, passed to `SubscriptionSet::add_with_warm`.
///
/// See `nemo/docs/mlc/plans/dig3-station-0325-derived-seeding-2026-07-04.md`
/// ("mlc follow-ups after publish", item 3) — native has no CORS/latency
/// constraint on REST paging, so chart-open can afford a much deeper
/// aggTrade/kline-approx backfill than the station-wide default.
pub(crate) const DERIVED_WARM_N_NATIVE: usize = 500_000;

/// Deep warm-seed depth for derived-chart-type subscriptions on wasm32.
///
/// Smaller than [`DERIVED_WARM_N_NATIVE`] — browser REST calls pay CORS
/// preflight + proxy latency per page (see design-doc risk register), so
/// the same page count costs proportionally more wall-clock time on the
/// wasm target. 10_000 keeps the chart-open loading window bounded while
/// still covering far more history than the station-wide shallow default.
pub(crate) const DERIVED_WARM_N_WASM: usize = 100_000;

/// Warm-seed depth to request for a fresh derived-chart-type subscription,
/// picked by target arch (see [`DERIVED_WARM_N_NATIVE`] / [`DERIVED_WARM_N_WASM`]).
#[inline]
pub(crate) const fn derived_warm_n() -> usize {
    if cfg!(target_arch = "wasm32") {
        DERIVED_WARM_N_WASM
    } else {
        DERIVED_WARM_N_NATIVE
    }
}

/// First-paint warm depth for a fresh FOOTPRINT subscription.
///
/// Owner rule 2026-07-05: cold-open must show clusters in seconds — seed a
/// shallow 10k-trade window first (≈10 REST pages, ~1-3s), then a single
/// background deepen re-seeds straight to the full [`derived_warm_n`] depth
/// (`request_footprint`'s lazy task). One teardown, one deep re-page — NOT
/// a doubling chain (10→20→40→... would re-fetch the newest pages on every
/// step and burn ~3x the REST weight of one direct deep seed).
pub(crate) const FOOTPRINT_INITIAL_WARM_N: usize = 10_000;

/// First-paint warm depth for a fresh subscription of any of the other 9
/// prepend-fold-capable derived kinds (renko/pnf/kagi/three_line_break/
/// range_bar/volume_bar/tick_bar/dollar_bar/cvd/tpo).
///
/// Same rationale and value as [`FOOTPRINT_INITIAL_WARM_N`] — cold-open
/// paints a shallow window immediately, then a background deepen re-seeds
/// straight to the full [`derived_warm_n`] depth via
/// `Station::rewarm_derived`, in place (see
/// `nemo/docs/mlc/plans/derived-prepend-fold-plan-2026-07-07.md` Phase 2).
/// `TickImbalanceBar`/`VolumeImbalanceBar`/`RunBar` cannot rewarm (EMA state
/// over the full history) — they keep cold-opening at full `derived_warm_n`
/// depth in one shot, unchanged.
pub(crate) const DERIVED_INITIAL_WARM_N: usize = 10_000;

/// Delay before the cold-open lazy deepen kicks in — long enough for the
/// shallow seed to finish + first clusters to paint, short enough that the
/// full history arrives while the user is still orienting.
pub(crate) const FOOTPRINT_LAZY_DEEPEN_DELAY_MS: u64 = 4_000;

/// Hard ceiling for [`DataBridge::request_deeper_derived_seed`]'s doubling —
/// once a key's warm depth reaches this, further scroll-left requests for
/// deeper history are marked exhausted rather than doubling forever (REST
/// page count / cold-start wall-clock time both scale linearly with this
/// value — see [`DERIVED_WARM_N_NATIVE`]/[`DERIVED_WARM_N_WASM`] doc for the
/// same native/wasm rationale).
pub(crate) const WARM_CAP_NATIVE: usize = 5_000_000;

/// Wasm counterpart of [`WARM_CAP_NATIVE`] — smaller because each REST page
/// costs proportionally more wall-clock time on the browser target (CORS
/// preflight + proxy latency per page).
pub(crate) const WARM_CAP_WASM: usize = 1_000_000;

/// Hard ceiling on the doubled warm-seed depth, picked by target arch.
#[inline]
pub(crate) const fn warm_cap() -> usize {
    if cfg!(target_arch = "wasm32") {
        WARM_CAP_WASM
    } else {
        WARM_CAP_NATIVE
    }
}

/// Extra per-stream wire-format inputs [`DataBridge::request_deeper_derived_seed`]
/// needs beyond the plain `(exchange, symbol, account, stream)` key, for the
/// one derived kind whose forwarder carries an extra display-only value not
/// recoverable from `Stream` alone.
#[derive(Debug, Clone, Copy, Default)]
pub enum DerivedSeedExtra {
    /// No extra input needed — every derived kind except P&F.
    #[default]
    None,
    /// P&F carries a pre-computed `box_size: f64` (human units) alongside
    /// the fixed-point `Stream::PnfBar(box_size_units, reversal_count)` —
    /// `spawn_pnf_sub` emits it on every `LiveUpdate::PnfBarUpdate` so
    /// consumers don't have to re-derive `box_size_units as f64 / 1e8`.
    PnfBoxSize(f64),
}

// `spawn_bg` / `spawn_bg_join` are inherent methods on `DataBridge` (see
// the bottom of `impl DataBridge`). They live there so the native path can
// `enter()` the captured `runtime_handle` before `tokio::spawn` —
// freeing every caller path of having to maintain its own runtime context.

/// Cross-platform handle to a spawned background task.
///
/// On native it wraps `tokio::task::JoinHandle<()>`; on wasm it wraps a
/// `futures_util::future::AbortHandle` (paired with the abortable future via
/// `spawn_local`).
///
/// Only `.abort()` is exposed — that is the only operation the bridge uses
/// (no await-for-completion).
pub(crate) struct BgJoinHandle {
    #[cfg(not(target_arch = "wasm32"))]
    inner: tokio::task::JoinHandle<()>,
    #[cfg(target_arch = "wasm32")]
    abort: futures_util::future::AbortHandle,
}

impl BgJoinHandle {
    pub(crate) fn abort(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.inner.abort();
        #[cfg(target_arch = "wasm32")]
        self.abort.abort();
    }
}

/// Cross-platform `sleep(Duration)`.
///
/// Native: `tokio::time::sleep` (needs the tokio time driver, which only the
/// native bin's `Runtime` provides). Wasm: `gloo_timers::future::TimeoutFuture`
/// (uses `setTimeout`, no tokio time driver needed). On wasm `tokio::time::sleep`
/// would call `tokio::time::Instant::now()` → `std::time::Instant::now()` →
/// panic at `std/sys/time/unsupported.rs:13` ("time not implemented").
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub(crate) async fn sleep_ms(ms: u64) {
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

#[cfg(target_arch = "wasm32")]
#[inline]
pub(crate) async fn sleep_ms(ms: u64) {
    gloo_timers::future::TimeoutFuture::new(ms as u32).await;
}

// `spawn_bg_join` is an inherent method on `DataBridge` — see comment above
// near where `spawn_bg` used to live.

/// Spawn a background future and return a [`BgJoinHandle`] WITHOUT entering
/// any runtime — for use from inside an already-spawned task where the
/// runtime is guaranteed to be active. Unlike `DataBridge::spawn_bg_join`
/// this does not need `&self`, so it can be called from inside a
/// `spawn_bg_join(async move { ... })` body where capturing `self` would
/// make the future non-`'static`.
#[cfg(not(target_arch = "wasm32"))]
#[inline]
pub(crate) fn spawn_bg_join_nested<F>(fut: F) -> BgJoinHandle
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    BgJoinHandle { inner: tokio::spawn(fut) }
}

#[cfg(target_arch = "wasm32")]
#[inline]
pub(crate) fn spawn_bg_join_nested<F>(fut: F) -> BgJoinHandle
where
    F: std::future::Future<Output = ()> + 'static,
{
    let (abort, reg) = futures_util::future::AbortHandle::new_pair();
    let abortable = futures_util::future::Abortable::new(fut, reg);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = abortable.await;
    });
    BgJoinHandle { abort }
}

/// Identifies whether an [`LiveUpdate::OrderbookSnapshot`] originated from the
/// one-shot REST bootstrap (deep, up to 1000 levels) or from the WebSocket
/// stream (partial snapshot — only used by exchanges that lack a diff stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderbookSource {
    Rest,
    Ws,
}

/// Overlay kind — used to key `overlay_subs` so each overlay is independent.
///
/// Overlays are per-instrument sub-pane / strip subscriptions that coexist
/// with any chart_type. They pair with derived / kline charts, not replace
/// them (e.g. Footprint + CVD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OverlayKind {
    Cvd,
    OpenInterest,
    FundingRate,
}

/// Updates sent from async tasks to the sync chart thread.
#[derive(Debug, Clone)]
pub enum LiveUpdate {
    /// Historical bars loaded from the exchange REST API.
    BarsLoaded {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        timeframe: String,
        bars: Vec<Bar>,
    },
    /// Live bar update from WebSocket (last bar in-place update or new bar).
    ///
    BarUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        bar: Bar,
    },
    /// Batch of OLDER bars spliced in front of the live window by
    /// [`digdigdig3_station::Station::rewarm_derived`], for the counted-bar
    /// family (range_bar/volume_bar/tick_bar/dollar_bar — all share
    /// `RewarmedPoints::Bar(Vec<BarPoint>)` on the station side, converted
    /// with the same `BarPoint` → `Bar` mapping [`LiveUpdate::BarUpdate`]'s
    /// live path uses). Same one-pass front-merge rationale as
    /// [`LiveUpdate::FootprintHistoryPrepended`] — NOT used for kline scroll
    /// (that has its own dedicated [`LiveUpdate::ScrollBarsLoaded`]).
    DerivedBarsPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        bars: Vec<Bar>,
    },
    /// Live trade update from WebSocket.
    TradeUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        price: f64,
        quantity: f64,
        timestamp: i64,
        is_buyer_maker: bool,
    },
    /// Mini ticker update from WebSocket (24h stats for watchlist).
    ///
    /// Fields other than `last_price` are `Option` so that BBO-only events
    /// (e.g. KuCoin `trade.ticker`) do not overwrite 24h stats that were
    /// previously set by a snapshot event.
    MiniTickerUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        last_price: f64,
        /// `None` when the event does not carry 24h stats (BBO-only update).
        price_change_percent: Option<f64>,
        /// `None` when the event does not carry 24h stats.
        high_price: Option<f64>,
        /// `None` when the event does not carry 24h stats.
        low_price: Option<f64>,
        /// `None` when the event does not carry 24h stats.
        volume: Option<f64>,
    },
    /// Exchange symbol list loaded from REST API.
    SymbolsLoaded {
        exchange_id: ExchangeId,
        symbols: Vec<SymbolInfo>,
    },
    /// A connector was successfully initialized.
    ConnectorReady {
        exchange_id: ExchangeId,
    },
    /// Backfill complete — initial historical bars fetched for a newly loaded symbol/TF.
    ///
    /// Distinct from `BarsLoaded` so consumers can apply post-backfill logic
    /// (e.g. unlocking scroll-fetch) without re-checking every `BarsLoaded`.
    BackfillComplete {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        timeframe: String,
        bars: Vec<Bar>,
    },
    /// Older bars loaded in response to a scroll-left (historical extension) request.
    ///
    /// `prepend_count` is how many bars should be prepended before the existing
    /// series so the consumer can keep the viewport position stable.
    ScrollBarsLoaded {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        timeframe: String,
        bars: Vec<Bar>,
        prepend_count: usize,
    },
    /// Full orderbook snapshot — REST bootstrap (deep, up to 1000 levels) or WS partial snapshot.
    OrderbookSnapshot {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
        timestamp: i64,
        /// Whether the snapshot originated from the REST poller or the WS stream.
        source: OrderbookSource,
    },
    /// Incremental orderbook delta from WebSocket.
    OrderbookDelta {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
        timestamp: i64,
    },
    /// An error occurred during an async data fetch.
    ///
    /// `account_type`, `symbol`, and `timeframe` identify the instrument that
    /// failed (empty string when the error is not tied to a specific series,
    /// e.g. a warmup or connector-dial failure).
    Error {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        timeframe: String,
        message: String,
    },
    /// Connector metrics snapshot collected on-demand (e.g. when a metrics panel renders).
    ConnectorMetrics {
        exchange_id: ExchangeId,
        /// Number of active WebSocket tasks for this exchange.
        ws_active: usize,
        /// Total HTTP requests made since the connector was created.
        http_requests: u64,
        /// Total HTTP errors (non-2xx responses or transport failures).
        http_errors: u64,
        /// Round-trip latency of the most recent HTTP request, in milliseconds.
        last_latency_ms: u64,
        /// Number of rate-limit tokens consumed in the current window.
        rate_used: u32,
        /// Maximum rate-limit tokens allowed in the window (0 = unknown).
        rate_max: u32,
    },
    /// Private stream: order lifecycle event (new, partial fill, fill, cancel).
    OrderUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        event: digdigdig3::OrderUpdateEvent,
    },
    /// Private stream: account balance change event.
    BalanceUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        event: digdigdig3::BalanceUpdateEvent,
    },
    /// Private stream: position change event (futures/margin).
    PositionUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        event: digdigdig3::PositionUpdateEvent,
    },

    /// Live footprint bar update from a `Stream::Footprint` subscription.
    ///
    /// Emitted on every trade that closes or updates the current footprint bar.
    /// The consumer (Phase 4 render) receives the updated bar state with
    /// per-price-level buy/sell breakdown.
    ///
    /// Phase 3 note: the tick.rs consumer is a no-op for now (Phase 4 render
    /// not yet implemented).  The variant is delivered into the channel so
    /// the plumbing is complete; the render arm is marked with a TODO.
    FootprintUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: digdigdig3_station::data::FootprintPoint,
    },

    /// Batch of OLDER footprint bars spliced in front of the live window by
    /// the lazy deepen (`Station::rewarm_footprint`). Points are oldest→
    /// newest and all at/older than the pre-deepen window head — the
    /// consumer merges them at the FRONT of its store in ONE pass (one
    /// cumulative-delta rebuild, one viewport prepend-compensation) instead
    /// of receiving thousands of per-point `FootprintUpdate`s that each
    /// trigger an O(n) out-of-order upsert in the same frame drain.
    FootprintHistoryPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        points: Vec<digdigdig3_station::data::FootprintPoint>,
    },

    /// Live Renko brick close from a `Stream::RenkoBar` subscription.
    /// Each event is one fully-closed brick (no in-progress emit —
    /// bricks are atomic, see TradeToRenkoBarDerived).
    RenkoBarUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: digdigdig3_station::data::RenkoBrickPoint,
    },

    /// Batch of OLDER renko bricks spliced in front of the live window by
    /// [`digdigdig3_station::Station::rewarm_derived`]. Same one-pass
    /// front-merge rationale as [`LiveUpdate::FootprintHistoryPrepended`] —
    /// points are oldest→newest and all at/older than the pre-deepen window
    /// head.
    RenkoHistoryPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        points: Vec<digdigdig3_station::data::RenkoBrickPoint>,
    },

    /// Live Point-and-Figure column update from a `Stream::PnfBar`
    /// subscription. Emitted on every trade that extends the current
    /// column or rolls into a new one — readers group by `column_id`.
    PnfBarUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: digdigdig3_station::data::PnfColumnPoint,
        /// Fixed box size in native price units (`box_size_units as f64 / 1e8`).
        /// Derived from the user-supplied `Kind::PnfBar(box_size_units, _)` at
        /// subscription time — passed through so the renderer never needs to
        /// infer box size from column geometry.
        box_size: f64,
    },

    /// Batch of OLDER P&F columns spliced in front of the live window by
    /// [`digdigdig3_station::Station::rewarm_derived`]. Same one-pass
    /// front-merge rationale as [`LiveUpdate::FootprintHistoryPrepended`].
    PnfHistoryPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        points: Vec<digdigdig3_station::data::PnfColumnPoint>,
        /// Same fixed box size as [`LiveUpdate::PnfBarUpdate`] — every point
        /// in the batch shares one subscription's box_size.
        box_size: f64,
    },

    /// Live Kagi segment / connector close from a `Stream::KagiBar`
    /// subscription. One event per closed segment.
    KagiBarUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: digdigdig3_station::data::KagiSegmentPoint,
    },

    /// Batch of OLDER kagi segments spliced in front of the live window by
    /// [`digdigdig3_station::Station::rewarm_derived`]. Same one-pass
    /// front-merge rationale as [`LiveUpdate::FootprintHistoryPrepended`].
    KagiHistoryPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        points: Vec<digdigdig3_station::data::KagiSegmentPoint>,
    },

    /// Cumulative Volume Delta scalar update from `Stream::CvdLine`.
    /// One event per upstream trade.
    CvdLineUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: digdigdig3_station::data::ScalarBarPoint,
    },

    /// Batch of OLDER CVD scalar samples spliced in front of the live window
    /// by [`digdigdig3_station::Station::rewarm_derived`]. Same one-pass
    /// front-merge rationale as [`LiveUpdate::FootprintHistoryPrepended`].
    CvdHistoryPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        points: Vec<digdigdig3_station::data::ScalarBarPoint>,
    },

    /// TPO Market Profile session snapshot from `Stream::TpoProfile`.
    /// Upserted on `open_time = session_date_ms`; the receiver typically
    /// keeps a single in-progress session plus a small ring of closed
    /// sessions for the historical-sessions view.
    TpoProfileUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: digdigdig3_station::data::TpoSessionPoint,
    },

    /// Batch of OLDER TPO sessions spliced in front of the live window by
    /// [`digdigdig3_station::Station::rewarm_derived`]. Same one-pass
    /// front-merge rationale as [`LiveUpdate::FootprintHistoryPrepended`].
    TpoHistoryPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        points: Vec<digdigdig3_station::data::TpoSessionPoint>,
    },

    /// Live Three Line Break line close from a `Stream::ThreeLineBreak`
    /// subscription. Each event is one completed line.
    ThreeLineBreakUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: digdigdig3_station::data::ThreeLineBreakLinePoint,
    },

    /// Batch of OLDER three-line-break lines spliced in front of the live
    /// window by [`digdigdig3_station::Station::rewarm_derived`]. Same
    /// one-pass front-merge rationale as
    /// [`LiveUpdate::FootprintHistoryPrepended`].
    TlbHistoryPrepended {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        points: Vec<digdigdig3_station::data::ThreeLineBreakLinePoint>,
    },

    /// Live Open Interest event from a `Stream::OpenInterest` subscription.
    /// Each event carries the current absolute OI value; the delta is
    /// computed in the forwarder by subtracting the previous OI level.
    OpenInterestUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: mlc_core::types::OiDeltaBar,
    },

    /// Live Funding Rate event from a `Stream::FundingRate` subscription.
    /// `interval_hours` defaults to 8 (standard perp swap schedule).
    FundingRateUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        point: mlc_core::types::FundingRateCell,
    },

    /// DOM heatmap snapshot for `chart_type == "dom_heatmap"` (C12).
    ///
    /// Sourced from an `OrderBook` station stream: each full book snapshot
    /// is forwarded as bids/asks slices.  The tick consumer calls
    /// `DomHeatmapBuffer::push_snapshot` to bucket and store the depth.
    DomHeatmapSnapshot {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        ts_ms: i64,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },

    /// Trade dot for the DOM heatmap overlay (C12).
    ///
    /// One event per trade tick.  The consumer appends to
    /// `ChartContainer::dom_trade_dots` (ring-capped at 2 000).
    DomTradeDot {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        dot: mlc_core::chart::dom_heatmap::DomTradeDot,
    },

    /// Per-event liquidation for the `liquidation_heatmap` chart-type (C13).
    ///
    /// Each `LiquidationPoint` from the station `Stream::Liquidation` is forwarded
    /// here.  The tick consumer pushes it into `LiquidationHeatmapBuffer` AND
    /// appends a `LiquidationDot` to the dot overlay ring.
    LiquidationEvent {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        ts_ms: i64,
        price: f64,
        value_usd: f64,
        /// 0 = short liquidated (green), 1 = long liquidated (red) — mirrors dig3 encoding.
        side: u8,
    },

    /// Bucketed period-aggregate liquidation for the `liquidation_heatmap` chart-type (C13).
    ///
    /// Sourced from `Stream::LiquidationBucket` (GateIO stats endpoint).
    /// Carries no per-price information — feeds the time-strip aggregate layer.
    LiquidationBucketUpdate {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        ts_ms: i64,
        long_liq_usd: f64,
        short_liq_usd: f64,
    },

    /// Trade-history depth for a `DeepTrades` chart_type's active series,
    /// derived from a station seed/deepen outcome (dig3 0.3.27
    /// `SeedOutcome`/`RewarmOutcome`).
    ///
    /// Emitted on cold subscribe (the real chart-open seed, `respawn_n ==
    /// 0`) by every derived-stream forwarder body, and again whenever an
    /// in-place rewarm/deepen flips the depth (e.g. Full → Capped mid-
    /// session). `mlc-backend`'s `tick_writers` converts [`HistoryDepthWire`]
    /// into the real `mlc_backend::bubble::HistoryDepth` and stamps it onto
    /// the owning bubble via `DataBubble::set_history_depth` — see
    /// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 1.
    HistoryDepthReported {
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: String,
        /// chart_type id this depth applies to (e.g. `"footprint"`,
        /// `"tick_bar"`) — `tick_writers` ignores this event if it doesn't
        /// match the owning bubble's CURRENTLY active `chart_type` (stale
        /// event from a just-switched-away subscribe racing the switch).
        chart_type: &'static str,
        depth: HistoryDepthWire,
    },
}

/// Live-data bridge backed by digdigdig3-station.
///
/// # Usage
///
/// ```ignore
/// let (bridge, mut rx, _ready_rx) = DataBridge::new().await;
/// let bridge = Arc::new(bridge);
///
/// bridge.ensure_connector(ExchangeId::Binance);
///
/// // Each frame, drain updates:
/// while let Ok(update) = rx.try_recv() { ... }
/// ```
pub struct DataBridge {
    hub: ExchangeHub,
    tx: broadcast::Sender<LiveUpdate>,
    /// Dedicated mpsc sender for `ConnectorReady` events.
    ///
    /// The app-level `tick_app_state` listens on the paired receiver instead
    /// of subscribing to the broadcast channel. This prevents the broadcast
    /// buffer from filling up when the app-level consumer falls behind, which
    /// would otherwise stall all other broadcast receivers.
    connector_ready_tx: mpsc::UnboundedSender<ExchangeId>,
    /// In-flight bar fetch keys.
    ///
    /// Prevents duplicate concurrent fetches for the same `(exchange, account_type,
    /// symbol, timeframe-or-role)` combination. A key is inserted before the task
    /// is spawned and removed at every exit point of the task. The symbol field
    /// is prefixed with `"backfill:"` or `"scroll:"` for those fetch roles.
    active_fetches: Arc<Mutex<HashSet<FetchKey>>>,
    /// Live WS ping RTT handles, keyed by exchange.
    ///
    /// Populated when a WebSocket task creates a connector that exposes a
    /// shared RTT arc (currently OKX only). `collect_metrics` reads from
    /// these handles via `try_lock` so it never blocks.
    ws_rtt_handles: Arc<Mutex<HashMap<ExchangeId, Arc<tokio::sync::Mutex<u64>>>>>,
    /// Oldest bar timestamp fetched per `(exchange, account, symbol, tf)`.
    ///
    /// Tracks how far back in history the bridge has already fetched so that
    /// scroll-left requests can request the correct older window without
    /// re-fetching data that is already cached.
    oldest_fetched_ts: Arc<Mutex<HashMap<FetchKey, i64>>>,
    /// Current warm-seed depth (`warm_n` last requested from the station) for
    /// each derived-chart-type subscription, keyed by
    /// `(ExchangeId, symbol, AccountType, Stream)`. `Stream` (not a bespoke
    /// enum) is reused as the flavor discriminant — it already carries the
    /// exact per-kind parameters (box_size, alpha, etc.) that make two
    /// differently-parametrized subscriptions of the "same" derived family
    /// (e.g. two Renko box sizes) distinct series, so it is a correct and
    /// non-duplicated identity for this map. Absent entry = never reseeded
    /// yet, fall back to [`derived_warm_n()`]. Populated by
    /// [`DataBridge::request_deeper_derived_seed`]; read by
    /// [`DataBridge::derived_warm_for_key`].
    derived_warm_state: Arc<Mutex<HashMap<(ExchangeId, String, AccountType, digdigdig3_station::Stream), usize>>>,
    /// Keys that have already reached [`warm_cap()`] — further
    /// `request_deeper_derived_seed` calls for these are a no-op. Same key
    /// shape as `derived_warm_state`.
    derived_seed_exhausted: Arc<Mutex<HashSet<(ExchangeId, String, AccountType, digdigdig3_station::Stream)>>>,
    /// The currently-active derived `Stream` for each instrument, keyed by
    /// `(ExchangeId, symbol, AccountType)` — same key shape as `derived_subs`.
    ///
    /// Every `request_*` derived-subscribe method records its `Stream` here
    /// right alongside inserting into `derived_subs`. This lets a caller that
    /// only knows the INSTRUMENT (not which derived kind/parameters are
    /// currently active — e.g. the generic scroll-left trigger in
    /// `mlc-app/frame_lifecycle/tick.rs`, which has no access to per-chart-type
    /// UI parameter state) ask for a deeper reseed via
    /// [`Self::request_deeper_seed_for_active`] without needing to reconstruct
    /// box_size/alpha/interval/etc. Cleared implicitly by being overwritten on
    /// every chart-type switch (never explicitly removed — a stale entry for
    /// an instrument with no live subscription is harmless: the generic
    /// dispatch just re-subscribes the same kind, which is idempotent).
    derived_active_stream: Arc<Mutex<HashMap<(ExchangeId, String, AccountType), digdigdig3_station::Stream>>>,
    /// Per-instrument footprint interval, alongside `derived_active_stream`
    /// for the one derived kind whose reseed needs the special-cased
    /// [`Self::request_deeper_footprint_seed`] entry point (interval as
    /// `&str`, not embedded in a `Stream` the generic dispatcher can replay).
    footprint_active_interval: Arc<Mutex<HashMap<(ExchangeId, String, AccountType), String>>>,
    /// Single-flight guard for the footprint lazy deepen: keys with an
    /// in-place `rewarm_footprint` pager currently running. Prevents a TF
    /// flip-flop (or any re-entrant `request_footprint` on the same
    /// interval) from stacking multiple 100-page REST walks for one key.
    footprint_deepen_in_flight: Arc<Mutex<HashSet<(ExchangeId, String, AccountType)>>>,
    /// Single-flight guard for the OTHER 9 prepend-fold-capable derived
    /// kinds' lazy deepen (renko/pnf/kagi/three_line_break/range_bar/
    /// volume_bar/tick_bar/dollar_bar/cvd/tpo) — same role as
    /// `footprint_deepen_in_flight`, keyed by `Stream` (not just the
    /// instrument) since a chart-type switch can leave a different derived
    /// kind's pager running for the same instrument.
    derived_deepen_in_flight: Arc<Mutex<HashSet<(ExchangeId, String, AccountType, digdigdig3_station::Stream)>>>,
    /// Per-key live orderbook reconstructors.
    ///
    /// Created on the first `subscribe_orderbook`/`subscribe_depth` call for a given
    /// `(ExchangeId, AccountType, symbol)` key, then shared by all subsequent
    /// subscribers for the same key. The station forwarder task is the sole writer
    /// (`apply_snapshot` / `apply_delta`); panels read concurrently via
    /// `blocking_read()`.
    ob_trackers: Arc<dashmap::DashMap<(digdigdig3::ExchangeId, digdigdig3::AccountType, String), SharedOrderBookTracker>>,
    /// Per-key live footprint series.
    ///
    /// Created on the first `request_footprint` call for a given
    /// `(ExchangeId, AccountType, symbol)` key, then shared by all subsequent
    /// readers. The station footprint forwarder task is the sole writer
    /// (append per `Event::Footprint`); panels read concurrently via
    /// `blocking_read()`. Survives chart-type / TF switches as long as a
    /// subscriber exists.
    footprint_series: Arc<dashmap::DashMap<(digdigdig3::ExchangeId, digdigdig3::AccountType, String), SharedFootprintSeries>>,
    /// One-shot guard for the all-connector warm-up. Set the first time
    /// `warmup_all_connectors` runs so the warm-up fires exactly once over the
    /// lifetime of the (shared, long-lived) bridge — NOT once per ChartApp. The
    /// bridge outlives the skeleton→live window promote, so the connectors dialed
    /// during the skeleton screen are reused; the live window's tick sees the flag
    /// already set and does nothing (no re-dial, no stale per-window cursor).
    warmup_started: AtomicBool,
    /// digdigdig3-station instance — owns warm-start, on-disk persistence
    /// (dig3_storage DiskStore), reconnect, and gap-heal for all market data.
    ///
    /// Constructed in [`Self::new`] via `.await`.
    /// Phase 2B (2026-06-10): COMPLETE for bars — live klines subscribe via
    /// Station; ALL REST history (Phase A fresh / Phase B heal / background
    /// backfill / scroll-left / blocking startup) goes through dig3
    /// `backfill::{klines_recent, fetch_history}` on `station.hub()` — one
    /// shared connector pool, one dial-wave. Trades / orderbook / ticker
    /// live-subscribe via Station too. Still raw-hub (NOT station): private
    /// streams (OrderUpdate / BalanceUpdate / PositionUpdate — auth required)
    /// and exchange_info.
    station: Arc<digdigdig3_station::Station>,
    /// C3: tracks which `(exchange_id, account_type, symbol)` keys already have
    /// a live station trade-subscriber task spawned.
    ///
    /// Prevents spawning N parallel station forwarders when `subscribe_trades`
    /// is called multiple times for the same key (one panel + chart header + etc.).
    /// Guarded by `std::sync::Mutex` — only held briefly (never across `.await`).
    station_trade_subscribed: Arc<Mutex<HashSet<(ExchangeId, AccountType, String)>>>,
    /// C4: tracks which `(ExchangeId, AccountType, symbol)` tuples already have a live
    /// station orderbook-subscriber task spawned.
    ///
    /// Prevents spawning N parallel station forwarders when `subscribe_orderbook`
    /// is called multiple times for the same key. Guarded by `std::sync::Mutex` —
    /// only held briefly (never across `.await`).
    station_orderbook_subscribed: Arc<Mutex<HashSet<(digdigdig3::ExchangeId, digdigdig3::AccountType, String)>>>,
    /// C5: tracks which `(ExchangeId, String, AccountType)` keys already have a
    /// live station ticker-subscriber task spawned.
    ///
    /// Prevents spawning N parallel station forwarders when `subscribe_mini_ticker`
    /// is called multiple times for the same symbol key.
    station_ticker_subscribed: Arc<Mutex<HashSet<(ExchangeId, String, AccountType)>>>,
    /// C5: tracks which `(ExchangeId, AccountType)` pairs already have a live
    /// station private-subscriber task spawned.
    ///
    /// One authenticated WS covers all symbols for that account — keyed by
    /// `(exchange, account_type)` not per-symbol.
    station_private_subscribed: Arc<Mutex<HashSet<(ExchangeId, AccountType)>>>,
    /// Exchanges with a targeted symbols-warmup currently in flight
    /// (`request_symbols` cold path). Prevents concurrent/looping re-warmups
    /// of the same venue: every pass would re-issue exchange_info REST calls
    /// with no backoff, which on a persistently-failing venue snowballed into
    /// an OOM-level request storm.
    symbols_warmup_inflight: Arc<Mutex<HashSet<ExchangeId>>>,
    /// Active derived-bar / footprint subscription task handles, keyed by
    /// instrument `(ExchangeId, symbol, AccountType)`.
    ///
    /// Unlike kline (which the station dedups internally), each derived-bar
    /// subscription spawns a fresh forwarder task. Switching tick→1m or
    /// 100t→1000t must ABORT the previous task (dropping its station handle so
    /// the 30s grace unsubscribe fires) — otherwise stale TickBar forwarders
    /// keep emitting BarUpdate into the same window. Guarded by `std::sync::Mutex`.
    derived_subs: Arc<Mutex<HashMap<(ExchangeId, String, AccountType), BgJoinHandle>>>,
    /// Active overlay subscription task handles — CVD, Open Interest, Funding
    /// Rate. Keyed by `(ExchangeId, symbol, AccountType, OverlayKind)`.
    ///
    /// Overlays are NOT chart types: they coexist with any chart_type on the
    /// same instrument (e.g. footprint + CVD sub-pane is footprint's canonical
    /// pairing). Before this slot existed they shared `derived_subs` and
    /// clobbered each other — enabling CVD right after `request_footprint`
    /// aborted the just-spawned footprint forwarder (both keyed by the plain
    /// instrument triplet). Kind is part of the key so different overlays on
    /// the same instrument are independent.
    overlay_subs: Arc<Mutex<HashMap<(ExchangeId, String, AccountType, OverlayKind), BgJoinHandle>>>,
    /// Tracks which `(ExchangeId, AccountType, symbol, timeframe)` keys already
    /// have a live `Stream::Kline` forwarder spawned by `request_bars`.
    ///
    /// Without this guard, every `request_bars` call (including the cache-fresh
    /// fast path) spawned a fresh subscriber → N parallel WS subscriptions to
    /// the same kline channel, N duplicate `LiveUpdate::BarUpdate` events per
    /// tick, and Binance rejecting the burst with HTTP 403.
    station_kline_subscribed: Arc<Mutex<HashSet<FetchKey>>>,
    /// Memoised `wire_symbol()` resolutions. The miss-path clones the full
    /// `Vec<SymbolInfo>` from station (~2500 entries × `serde_json::Value`
    /// `extra` field on Binance = ~10MB allocation) and runs 2-3 linear
    /// scans with per-element `format!()` + `to_uppercase`, costing
    /// 130-170ms per call. The catalogue is effectively immutable after
    /// startup, so a HashMap by (eid, at, requested) collapses every
    /// repeat lookup (preset switch, request_bars, request_footprint,
    /// subscribe_depth/trades/orderbook/mini_ticker, etc.) to O(1).
    wire_symbol_cache: Arc<Mutex<HashMap<(ExchangeId, AccountType, String), String>>>,
    /// Per-exchange last seeded symbol count. Each SymbolsLoaded event
    /// arrives multiple times per exchange (one per account_type as REST
    /// completes). Without this guard, an exchange with N symbols and 4
    /// account_types ran the prewarm O(N) scan 4 times (= 500ms total for
    /// Binance). Skip when count hasn't grown since the last successful
    /// prewarm.
    wire_symbol_prewarmed_count: Arc<Mutex<HashMap<ExchangeId, usize>>>,
    /// Active `Stream::Kline` forwarder task handles keyed by
    /// `(ExchangeId, AccountType, symbol, timeframe)`.
    ///
    /// Stored so that an explicit `unsubscribe_kline_bars` (TF / symbol /
    /// preset switch) can `.abort()` the task and drop the consumer's
    /// SubscriptionHandle inside the station — that triggers the 30s grace
    /// window and prevents the old `(BTCUSDT, 1h)` forwarder from staying
    /// live after the user switches to `(BTCUSDT, 1m)`.
    station_kline_handles: Arc<Mutex<HashMap<FetchKey, BgJoinHandle>>>,
    /// Captured tokio runtime handle (native only).
    ///
    /// Filled in `new()` via `Handle::current()` — that always succeeds
    /// because `new()` is `async` and therefore already runs inside the
    /// caller's runtime. Every `tokio::spawn` site inside `DataBridge`
    /// goes through `self.spawn_bg(...)` / `self.spawn_bg_join(...)`,
    /// which `enter()` this handle around `tokio::spawn` so synchronous
    /// caller paths (winit event handlers, headless tick loop, alert
    /// delivery init, agent server bootstrap) do NOT need to wrap each
    /// bridge call in their own runtime context.
    #[cfg(not(target_arch = "wasm32"))]
    runtime_handle: tokio::runtime::Handle,
}

impl DataBridge {
    /// Create a new bridge.
    ///
    /// Must be called from within an async context (or wrapped in a `block_on`
    /// at the top-level entry point).  The caller is responsible for providing
    /// and keeping alive the tokio runtime; the bridge itself does not own one.
    ///
    /// Returns:
    /// - `DataBridge`: the bridge itself
    /// - `broadcast::Receiver<LiveUpdate>`: receives ALL live updates (bars, symbols, errors, …)
    /// - `mpsc::UnboundedReceiver<ExchangeId>`: receives ONLY `ConnectorReady` exchange IDs
    ///
    /// The separate mpsc receiver lets the app-level consumer handle `ConnectorReady`
    /// events without subscribing to the full broadcast channel.  This prevents the
    /// broadcast buffer from backing up when the app-level consumer is slow.
    ///
    /// Additional broadcast receivers can be created with [`add_listener`]. The channel
    /// capacity is 4096 messages; if a slow receiver falls behind, old messages are
    /// dropped for that receiver only.
    pub async fn new() -> (Self, broadcast::Receiver<LiveUpdate>, mpsc::UnboundedReceiver<ExchangeId>) {
        // Anchor the process-relative clock here so every [STARTUP-WAVE] timestamp
        // is measured from bridge construction (≈ app start), giving a stable t=0
        // shared by the bridge (connect wave) and the main thread (promote).
        let _ = crate::elapsed_ms();

        let (tx, rx) = broadcast::channel(65536);
        let (connector_ready_tx, connector_ready_rx) = mpsc::unbounded_channel();

        // Build the Station. Persistence is on, warm-start = 5000 most-recent
        // points per series, gap-heal default.
        // Storage lives under <APPDATA>/mylittlechart/dig3_storage/.
        // NOTE: the old %APPDATA%/mylittlechart/{bars,trades,orderbook}/
        // trees are dead — no code reads or writes them anymore (the legacy
        // store readers/writers were removed before Phase 2B); they are just
        // leftover files safe to wipe on user machines.
        let storage_root = mlc_core::app_data_dir().join("dig3_storage");
        let station = digdigdig3_station::Station::builder()
            .storage_root(storage_root)
            .persistence(digdigdig3_station::PersistenceConfig::on())
            // 5000 trade-ring depth so derived bars (range/volume/tick/
            // footprint) have enough historical trade data to seed at
            // least a few closed bars at typical parameters. Audit
            // 2026-06-17 (research/station-derived-prewarm-audit) showed
            // the previous 1000-cap covered only ~1-5 seconds on BTC,
            // never enough to produce a single closed range bar.
            .warm_start(5000)
            .unsubscribe_grace(std::time::Duration::from_secs(30))
            .orderbook_rest_seed(true)
            .orderbook_seed_depth(1000)
            .build()
            .await
            .expect("failed to build digdigdig3-station");
        let station = Arc::new(station);

        // The bridge's hub IS the station's hub. `ExchangeHub::clone` is O(1)
        // and SHARES the underlying Arc-backed connector pools, so every
        // legacy path (trading methods, private streams, ensure_connector,
        // the sidebar's `pool()` view) operates on the same connectors the
        // station dials during warm-up — one pool, one dial-wave. Before this
        // unification the bridge kept a second, never-dialed hub, which left
        // trading/sidebar looking at an empty pool after Phase 2B moved the
        // startup dial onto the station.
        let hub = (*station.hub()).clone();

        // Stream the station's connector lifecycle events (ConnectorReady per
        // exchange as Phase-1 connect lands, SymbolsLoaded per exchange as
        // Phase-2 exchange_info lands) into the bridge's channels AS THEY
        // ARRIVE. Without this forwarder the app would only learn about
        // connectors after `Station::warmup` finishes for the WHOLE fleet —
        // and the fleet pass is serialized over ~21 exchanges where
        // geo-blocked venues (MOEX, Dukascopy) sit on long timeouts, so
        // fast venues' symbols would be held hostage for minutes.
        {
            let station_events = station.connector_events();
            let fwd_tx = tx.clone();
            let fwd_ready_tx = connector_ready_tx.clone();
            let fwd_station = station.clone();
            // Inside `new()` we are already inside the caller's tokio
            // runtime (this fn is `async`), so spawn directly without going
            // through the `self.spawn_bg` helper which needs `&self` (the
            // struct does not yet exist at this point).
            let fwd_task = async move {
                let mut events = station_events;
                // Per-exchange dedup. Station emits ConnectorReady on EVERY
                // warmup pass over an exchange, including the targeted
                // re-warmups request_symbols spawns — forwarding repeats
                // would loop: ready → app calls request_symbols → cold cache
                // → targeted warmup → ready again, hammering REST with no
                // backoff (observed as a 6 GB / OOM exchange_info storm on
                // venues whose exchange_info request kept failing).
                let mut ready_seen: HashSet<ExchangeId> = HashSet::new();
                loop {
                    match events.recv().await {
                        Ok(digdigdig3_station::Event::ConnectorReady { exchange }) => {
                            if ready_seen.insert(exchange) {
                                let _ = fwd_tx.send(LiveUpdate::ConnectorReady { exchange_id: exchange });
                                let _ = fwd_ready_tx.send(exchange);
                            }
                        }
                        Ok(digdigdig3_station::Event::SymbolsLoaded { exchange, .. }) => {
                            // Emit the MERGED snapshot across account types
                            // cached so far (the app overwrites its per-
                            // exchange entry on every SymbolsLoaded, so each
                            // later emit must be a superset of the earlier).
                            let symbols = fwd_station.symbols(exchange);
                            if !symbols.is_empty() {
                                let _ = fwd_tx.send(LiveUpdate::SymbolsLoaded {
                                    exchange_id: exchange,
                                    symbols,
                                });
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            };
            #[cfg(not(target_arch = "wasm32"))]
            tokio::spawn(fwd_task);
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(fwd_task);
        }

        (Self {
            hub,
            tx,
            connector_ready_tx,
            active_fetches: Arc::new(Mutex::new(HashSet::new())),
            ws_rtt_handles: Arc::new(Mutex::new(HashMap::new())),
            oldest_fetched_ts: Arc::new(Mutex::new(HashMap::new())),
            derived_warm_state: Arc::new(Mutex::new(HashMap::new())),
            derived_seed_exhausted: Arc::new(Mutex::new(HashSet::new())),
            derived_active_stream: Arc::new(Mutex::new(HashMap::new())),
            footprint_active_interval: Arc::new(Mutex::new(HashMap::new())),
            footprint_deepen_in_flight: Arc::new(Mutex::new(HashSet::new())),
            derived_deepen_in_flight: Arc::new(Mutex::new(HashSet::new())),
            ob_trackers: Arc::new(dashmap::DashMap::new()),
            footprint_series: Arc::new(dashmap::DashMap::new()),
            warmup_started: AtomicBool::new(false),
            station,
            station_trade_subscribed: Arc::new(Mutex::new(HashSet::new())),
            station_orderbook_subscribed: Arc::new(Mutex::new(HashSet::new())),
            station_ticker_subscribed: Arc::new(Mutex::new(HashSet::new())),
            station_private_subscribed: Arc::new(Mutex::new(HashSet::new())),
            symbols_warmup_inflight: Arc::new(Mutex::new(HashSet::new())),
            derived_subs: Arc::new(Mutex::new(HashMap::new())),
            overlay_subs: Arc::new(Mutex::new(HashMap::new())),
            station_kline_subscribed: Arc::new(Mutex::new(HashSet::new())),
            wire_symbol_cache: Arc::new(Mutex::new(HashMap::new())),
            wire_symbol_prewarmed_count: Arc::new(Mutex::new(HashMap::new())),
            station_kline_handles: Arc::new(Mutex::new(HashMap::new())),
            // `new()` is async — `Handle::current()` ALWAYS succeeds here
            // because it is being driven by the caller's runtime.
            #[cfg(not(target_arch = "wasm32"))]
            runtime_handle: tokio::runtime::Handle::current(),
        }, rx, connector_ready_rx)
    }

    // ── spawn helpers (centralised tokio runtime entry, see crate-top doc) ──
    //
    // These were free functions before 2026-06-24. Each spawn site inside
    // `DataBridge` now calls `self.spawn_bg(...)` / `self.spawn_bg_join(...)`.
    // The native impl `enter()`s `self.runtime_handle` for the duration of
    // the `tokio::spawn` call so synchronous callers don't have to.

    /// Spawn a background future with no return value.
    #[cfg(not(target_arch = "wasm32"))]
    #[inline]
    pub(crate) fn spawn_bg<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let _g = self.runtime_handle.enter();
        tokio::spawn(fut);
    }

    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub(crate) fn spawn_bg<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        wasm_bindgen_futures::spawn_local(fut);
    }

    /// Spawn a background future and return a [`BgJoinHandle`] that can
    /// cancel it.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn spawn_bg_join<F>(&self, fut: F) -> BgJoinHandle
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let _g = self.runtime_handle.enter();
        BgJoinHandle { inner: tokio::spawn(fut) }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn spawn_bg_join<F>(&self, fut: F) -> BgJoinHandle
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        let (abort, reg) = futures_util::future::AbortHandle::new_pair();
        let abortable = futures_util::future::Abortable::new(fut, reg);
        wasm_bindgen_futures::spawn_local(async move {
            let _ = abortable.await;
        });
        BgJoinHandle { abort }
    }

    /// Access to the underlying [`digdigdig3_station::Station`].
    ///
    /// Returned as an `Arc` so callers can spawn long-lived consumer tasks
    /// (subscription drainers, replay UIs) without juggling lifetimes against
    /// the bridge itself. Currently used only by phase-2 adapters inside this
    /// crate; downstream chart code keeps consuming the legacy
    /// [`LiveUpdate`] broadcast.
    pub fn station(&self) -> Arc<digdigdig3_station::Station> {
        self.station.clone()
    }

    /// Create a new update receiver that receives all future updates.
    ///
    /// Use this to attach additional windows (spawned with `new_empty`) to the
    /// same `DataBridge` without spinning up a second tokio runtime or connector
    /// pool. Each receiver gets its own independent read position — messages are
    /// not consumed from other receivers when one drains its queue.
    pub fn add_listener(&self) -> broadcast::Receiver<LiveUpdate> {
        self.tx.subscribe()
    }

    /// Test-only: clone of the broadcast sender, so harness tests can inject
    /// synthetic `LiveUpdate`s without going through a real WebSocket / Station.
    /// Production code MUST NOT call this — emit events through the bridge's
    /// public request API instead.
    #[doc(hidden)]
    pub fn debug_broadcast_tx(&self) -> broadcast::Sender<LiveUpdate> {
        self.tx.clone()
    }

    /// Snapshot the bridge's per-exchange symbol cache.
    ///
    /// Returns every exchange whose REST `exchange_info` has already resolved
    /// during this process — i.e. every `SymbolsLoaded` event ever broadcast.
    /// New `ChartApp` instances (notably the live-window instance created
    /// during skeleton→live promote) use this to seed their `exchange_symbols`
    /// map without racing against the broadcast channel: the skeleton's
    /// receiver caught every `SymbolsLoaded` between connect and promote, but
    /// dropping the skeleton ChartApp dropped that map with it; the cache
    /// itself lives on the bridge and survives.
    pub fn symbols_snapshot(&self) -> HashMap<ExchangeId, Vec<SymbolInfo>> {
        // Phase 2B: read straight from the station's exchange_info cache
        // (populated by Station::warmup / request_symbols) — the bridge no
        // longer keeps its own symbol cache.
        let hub = self.station.hub();
        let mut out = HashMap::new();
        for eid in hub.list_connected() {
            let syms = self.station.symbols(eid);
            if !syms.is_empty() {
                out.insert(eid, syms);
            }
        }
        out
    }

    /// Preload `wire_symbol_cache` for every symbol in `exchange_id`'s
    /// catalogue across every account_type variant we'll dispatch on
    /// (Spot/Margin/Futures/FuturesIso). Eliminates the 130-180ms first-call
    /// hit on `request_bars` / `request_footprint` / `subscribe_*` at the
    /// cost of one O(N) scan right after `SymbolsLoaded` arrives.
    ///
    /// Idempotent — the cache `.insert()` overwrites, but since the input
    /// is the source catalogue itself the value is stable.
    pub fn prewarm_wire_symbol_cache(&self, exchange_id: ExchangeId) {
        let _t0 = web_time::Instant::now();
        let symbols = self.station.symbols(exchange_id);
        if symbols.is_empty() {
            return;
        }
        let n = symbols.len();
        // Per-exchange idempotency: skip when the catalogue hasn't grown
        // since the last successful prewarm. SymbolsLoaded fires once per
        // account_type as REST resolves — without this guard a Binance
        // catalogue with 4389 symbols × 5 account_types ran the O(N) scan
        // 5 times (~500ms total). The catalogue length is monotone, so
        // a strict equality skip is safe.
        {
            let mut counts = match self.wire_symbol_prewarmed_count.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if counts.get(&exchange_id).copied().unwrap_or(0) >= n {
                return;
            }
            counts.insert(exchange_id, n);
        }
        let Ok(mut cache) = self.wire_symbol_cache.lock() else { return; };
        // For each SymbolInfo, the wire form is `s.symbol`. wire_symbol's
        // case (1) returns `requested` unchanged on an exact native match,
        // so seeding (eid, at, s.symbol) → s.symbol is the answer for all
        // canonical-form-equals-native exchanges (Binance and most others).
        // The `at` we cache against is `s.account_type` — that's the only
        // bucket that would ever resolve to this s.symbol via cases (2)+(3)
        // of wire_symbol, so caching by it costs nothing and saves the same
        // miss-path scan on the actual lookup.
        for s in &symbols {
            cache.insert(
                (exchange_id, s.account_type, s.symbol.clone()),
                s.symbol.clone(),
            );
            // Also seed the canonical lookup (base+quote concatenated,
            // uppercased) — this is what mlc passes when the preset's
            // bubble carries the canonical form. wire_symbol case (2) /
            // (3) would otherwise pay the linear scan on first use.
            if !s.base_asset.is_empty() && !s.quote_asset.is_empty() {
                let canonical = format!("{}{}", s.base_asset, s.quote_asset).to_uppercase();
                if canonical != s.symbol {
                    cache.insert(
                        (exchange_id, s.account_type, canonical),
                        s.symbol.clone(),
                    );
                }
            }
        }
        crate::dlog!(
            "[Bridge::prewarm_wire_symbol_cache] {:?}: seeded {} symbols in {}us",
            exchange_id, n, _t0.elapsed().as_micros(),
        );
    }

    /// Get a reference to the connector hub.
    pub fn pool(&self) -> &ExchangeHub {
        &self.hub
    }

    /// Trade-history depth capability for `exchange_id`, per account type.
    ///
    /// Thin wrapper over `ExchangeHub::trade_history_capabilities` — `None`
    /// if the exchange isn't connected yet (mirrors the hub's own `None`
    /// contract). Consumed by the footprint hybrid switch point
    /// (`mlc-backend::chart_type::engine::subscribe_footprint`) to decide
    /// whether a `RecentOnly` venue keeps the kline backdrop alive instead
    /// of taking over `bubble.bars` — see
    /// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 3.
    pub fn trade_history_capabilities(
        &self,
        exchange_id: ExchangeId,
    ) -> Option<digdigdig3::core::types::TradeHistoryCapabilities> {
        self.hub.trade_history_capabilities(exchange_id)
    }

    /// Native kline-interval capability for `exchange_id`, per account type.
    ///
    /// Thin wrapper over `ExchangeHub::kline_interval_capabilities` — `None`
    /// if the exchange isn't connected yet (mirrors the hub's own `None`
    /// contract). Consumed by the TF dropdown per-item availability marking
    /// (`mlc-app::ChartApp::timeframe_data_modes`) to distinguish natively-
    /// served intervals from ones only reachable via live trade aggregation
    /// (`TradeToBarDerived`, no REST history) — see
    /// `nemo/docs/mlc/plans/resolution-availability-2026-07-08.md` Phase 3.
    pub fn kline_interval_capabilities(
        &self,
        exchange_id: ExchangeId,
    ) -> Option<digdigdig3::core::types::KlineIntervalCapabilities> {
        self.hub.kline_interval_capabilities(exchange_id)
    }

    /// Record the oldest bar timestamp seen for a given key.
    ///
    /// Only updates the stored value when `ts` is strictly older (smaller) than
    /// whatever was previously recorded. Safe to call from any thread.
    pub fn record_oldest_ts(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: &str,
        timeframe: &str,
        ts: i64,
    ) {
        if let Ok(mut map) = self.oldest_fetched_ts.lock() {
            let key: FetchKey = (exchange_id, account_type, symbol.to_string(), timeframe.to_string());
            let entry = map.entry(key).or_insert(i64::MAX);
            if ts < *entry {
                *entry = ts;
            }
        }
    }

    /// Return the oldest bar timestamp fetched for the given key, or `None` if
    /// no fetch has been recorded yet.
    pub fn get_oldest_fetched_ts(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: &str,
        timeframe: &str,
    ) -> Option<i64> {
        let key: FetchKey = (exchange_id, account_type, symbol.to_string(), timeframe.to_string());
        self.oldest_fetched_ts.lock().ok()?.get(&key).copied()
    }

    /// Warm up every connector in `exchanges`, exactly ONCE over the bridge's
    /// lifetime (guarded by an internal one-shot flag).
    ///
    /// Phase 2B: warm-up now runs through dig3 `Station::warmup`, which dials
    /// `connect_public` on the STATION's `ExchangeHub` (Phase 1: concurrent
    /// JoinSet) and loads symbol lists (Phase 2). Routing the dial through the
    /// station hub means historical backfill (which also goes through
    /// `station.hub()` now) and live WS subscriptions share ONE connector pool
    /// — one dial-wave, one rate-limit budget, no duplicate parallel hub.
    ///
    /// Per-exchange readiness streams LIVE through the connector-events
    /// forwarder spawned in [`Self::new`]: the station broadcasts
    /// `Event::ConnectorReady` the moment each exchange's Phase-1 connect
    /// lands and `Event::SymbolsLoaded` as each Phase-2 exchange_info lands;
    /// the forwarder relays both into the mlc `LiveUpdate` broadcast + the
    /// `connector_ready` mpsc. This function therefore only kicks the warm-up
    /// off and reports terminal failures — it does NOT gate readiness on the
    /// whole fleet finishing (geo-blocked venues sit on long timeouts).
    ///
    /// Idempotent at two levels: the one-shot flag prevents a second pass, and
    /// `Station::warmup` itself short-circuits already-connected exchanges.
    pub fn warmup_all_connectors(&self, exchanges: &[ExchangeId]) {
        // compare_exchange: only the first caller proceeds; all later calls (e.g.
        // the promoted live window) short-circuit.
        if self
            .warmup_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        crate::dlog!(
            "[STARTUP-WAVE bridge] warmup_all_connectors START t={}ms n_exchanges={}",
            crate::elapsed_ms(),
            exchanges.len()
        );

        let station = self.station.clone();
        let tx = self.tx.clone();
        let exchanges = exchanges.to_vec();
        self.spawn_bg(async move {
            let report = station.warmup(&exchanges).await;
            crate::dlog!(
                "[STARTUP-WAVE bridge] station warmup DONE t={}ms ok={} failed={}",
                crate::elapsed_ms(),
                report.ok.len(),
                report.failed.len()
            );
            // Successes already streamed via the connector-events forwarder.
            for (eid, err) in report.failed {
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id: eid,
                    account_type: digdigdig3::AccountType::Spot,
                    symbol: String::new(),
                    timeframe: String::new(),
                    message: format!("warmup failed: {}", err),
                });
            }
        });
    }

    /// Ensure a connector is initialized for the given exchange.
    ///
    /// Non-blocking — spawns an async task. On completion, sends either
    /// `ConnectorReady` or `Error` through the update channel.
    pub fn ensure_connector(&self, exchange_id: ExchangeId) {
        if self.hub.is_connected(exchange_id) {
            return;
        }
        let hub = self.hub.clone();
        let tx = self.tx.clone();
        let connector_ready_tx = self.connector_ready_tx.clone();
        crate::dlog!(
            "[STARTUP-WAVE bridge] ensure_connector SPAWN {:?} t={}ms",
            exchange_id,
            crate::elapsed_ms()
        );
        self.spawn_bg(async move {
            let _connect_t0 = crate::elapsed_ms();
            // Creating connector asynchronously. No stagger — the connector
            // tokio runtime is independent of the UI thread, so a startup burst
            // of TCP/TLS handshakes does not block rendering (it never did; the
            // measured present-stall came from a different path). Eager connect
            // gives the fullest symbol list and fastest first data.
            match hub.connect_public(exchange_id, false).await {
                Ok(()) => {
                    crate::dlog!(
                        "[STARTUP-WAVE bridge] connect_public DONE {:?} t={}ms took={}ms (firing ConnectorReady)",
                        exchange_id,
                        crate::elapsed_ms(),
                        crate::elapsed_ms().saturating_sub(_connect_t0)
                    );
                    let _ = tx.send(LiveUpdate::ConnectorReady { exchange_id });
                    // Also notify the app-level mpsc consumer so it doesn't
                    // need to hold a broadcast subscription open.
                    let _ = connector_ready_tx.send(exchange_id);
                    // Connector ready.
                }
                Err(e) => {
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        account_type: digdigdig3::AccountType::Spot,
                        symbol: String::new(),
                        timeframe: String::new(),
                        message: format!("{}", e),
                    });
                    // Connector init failed (error sent via channel).
                }
            }
        });
    }

    /// Request historical bars (klines) from an exchange.
    ///
    /// Non-blocking — spawns an async task. On completion, sends either
    /// `BarsLoaded` or `Error` through the update channel.
    ///
    /// **3-level bar loading:**
    ///
    /// Level 1 — instant render: if the station's `Series<BarPoint>` already
    /// holds bars for this key — from a previous subscribe this session or from
    /// the dig3 Station warm-start (last-1000 replay out of dig3_storage on
    /// subscribe + cross-session disk read_tail) — they are sent as an immediate
    /// `BarsLoaded` below, before any network round-trip.
    ///
    /// Level 2 (Phase A) — quick fresh fetch: always fetches 300 bars from now
    /// backward, merges with any session cache, sends `BarsLoaded`. Viewport
    /// enters Follow mode.
    ///
    /// Level 3 (Phase B) — async heal: if disk cache existed before Phase A and
    /// there was a gap between the disk tail and the fresh 300 bars, paginate
    /// backward to fill 2× the gap, merge, send a second `BarsLoaded`.
    ///
    /// The `limit` and `total_bars` parameters are retained for API compatibility
    /// but are no longer used — Phase A always fetches 300 bars.
    pub fn request_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,
        _limit: Option<u16>,
        _total_bars: Option<usize>,
        force: bool,
    ) {
        let _rb_total = web_time::Instant::now();
        // Phase 2B: historical fetch (Phase A + Phase B) runs through the
        // STATION's shared hub so it reuses the same connector pool as live
        // WS subscriptions (one dial-wave, one rate budget). `station.hub()`
        // is an O(1) Arc clone of that pool.
        let _rb_t_hub = web_time::Instant::now();
        let fetch_hub = self.station.hub();
        let rb_hub_us = _rb_t_hub.elapsed().as_micros();
        let tx = self.tx.clone();
        // Canonical key — used for cache keys and event symbols so app-side
        // lookups always match bubble.symbol (which is now canonical).
        let symbol_str = symbol.to_string();
        // Wire string — venue-native form required by dig3 connectors.
        // Resolved eagerly for the station kline subscriber; the main REST task
        // retries resolution inside the connector-wait loop if the symbol list
        // is still empty at spawn time (Fix 2 — MUUSDC@hyperliquid race).
        let _rb_t_wire = web_time::Instant::now();
        let wire_sym = self.wire_symbol(exchange_id, account_type, symbol);
        let rb_wire_us = _rb_t_wire.elapsed().as_micros();
        // Station handle cloned for the resolution retry inside the main task.
        let station_for_retry = self.station.clone();
        let interval = timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let tf_name_for_log = tf_name.clone();
        let active_fetches = self.active_fetches.clone();

        let cache_key: FetchKey = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());
        let station_bar_seriskey = station_bar_key(exchange_id, account_type, &symbol_str, &tf_name);
        // `station_bar_seriskey` is consumed by the forwarder spawn below; the
        // dedup hot-path never touches the station ring. Snapshotting bars
        // here (the old `cached_bars` path) cost ~100µs per bar × 1500-2000
        // bars = ~150ms on a non-force LoadPreset call where dedup-L2 would
        // throw the result away anyway. Bug 10 in handoff-2026-06-17.
        let _ = &station_bar_seriskey;

        crate::dlog!(
            "[Bridge::request_bars enter] {:?} {} at={:?} tf={} force={}",
            exchange_id, symbol_str, account_type, tf_name, force,
        );

        // ── Two-layer dedup ───────────────────────────────────────────────────
        //
        // Layer 1 (active_fetches): fetch currently in flight on the runtime
        // — drop the duplicate caller flat. Same as pre-Phase 2B.
        //
        // Layer 2 (station_kline_subscribed): a Stream::Kline forwarder for
        // this (eid, at, sym, tf) is already live. force=true callers
        // (ChangeTimeframe, ChangeSymbol, LoadPreset) still get a fresh
        // Phase A REST and a new BarsLoaded — the user explicitly asked for
        // a refresh. force=false callers (ConnectorReady sweep,
        // warm_one_background_tab, etc.) silently exit — the subscriber is
        // already piping live ticks via BarUpdate; another Phase A would
        // overwrite window.bars with a slightly-different fresh_300 and
        // bounce the viewport every time the secondary trigger fires.
        {
            let mut af = active_fetches.lock().unwrap_or_else(|e| e.into_inner());
            if af.contains(&cache_key) {
                crate::dlog!("[Bridge::request_bars dedup-L1] in-flight, skip {:?} {} tf={}", exchange_id, symbol_str, tf_name);
                return;
            }
            af.insert(cache_key.clone());
        }

        let already_subscribed = {
            let subs = self.station_kline_subscribed.lock().unwrap_or_else(|e| e.into_inner());
            subs.contains(&cache_key)
        };
        if already_subscribed && !force {
            crate::dlog!("[Bridge::request_bars dedup-L2] subscriber live, non-force skip {:?} {} tf={}", exchange_id, symbol_str, tf_name);
            if let Ok(mut af) = active_fetches.lock() { af.remove(&cache_key); }
            return;
        }

        // Phase B gap-detection was REMOVED in commit bea348e. The
        // `disk_last_ts` / `had_disk_cache` values it consumed no longer
        // serve a purpose — see the `let _ = had_disk_cache;` no-op below.
        let disk_last_ts: Option<i64> = None;
        let had_disk_cache: bool = false;

        // Spawn the Stream::Kline subscriber ONCE per (eid, at, sym, tf) key.
        // The forwarder lives until the bridge is dropped — station owns the
        // WS lifetime via its own grace window.
        let need_spawn_kline_sub = {
            let mut subs = self.station_kline_subscribed.lock().unwrap_or_else(|e| e.into_inner());
            if subs.contains(&cache_key) { false } else { subs.insert(cache_key.clone()); true }
        };

        // Level 1 — REMOVED in the Phase 2B aftermath.
        //
        // Pre-Phase 2B the bridge maintained its own mlc-side ring buffer
        // (bar_cache: SharedSeriesMap). The Stream::Kline subscriber wrote
        // INTO it, and cached_bars read FROM the same ring. Re-emitting
        // BarsLoaded with cached_bars was safe — the ring was in lockstep
        // with what the app had already received.
        //
        // After Phase 2B cached_bars comes from station's own Series<BarPoint>,
        // which is fed not only by live WS ticks but also by warm-seed
        // (REST replay-1000 on subscribe) and cross-session disk read_tail.
        // That means the station ring is a DIFFERENT timestamp range than
        // what the upcoming Phase A fetch_history is about to return — so
        // emitting Level-1 BarsLoaded here, then Phase A BarsLoaded a moment
        // later, sets window.bars to two different ranges back-to-back, which
        // caused the viewport to "jump a month back every second".
        //
        // Phase A always runs (cache-fresh skip short-circuits later but
        // emits no BarsLoaded), so the chart's initial paint is the
        // freshly fetched 300 bars; live ticks arrive via LiveUpdate::BarUpdate.

        if need_spawn_kline_sub {
            crate::dlog!(
                "[Bridge::request_bars spawn-kline-fwd] {:?} {} at={:?} tf={}",
                exchange_id, symbol_str, account_type, tf_name,
            );
            let station = self.station.clone();
            let station_tx = self.tx.clone();
            // Canonical form — emitted back to the app so bubble.symbol lookups match.
            let station_symbol_canonical = symbol_str.clone();
            // Wire form — passed to dig3 station for the actual subscription.
            let station_symbol_wire = wire_sym.clone();
            let station_interval = interval.clone();
            let station_tf_name = tf_name.clone();
            // Dedup-gate guards, cloned so the task can evict its own key on
            // every exit path — mirrors the trade path (subscribe_trades'
            // task tail). Without this a dead forwarder leaves the key
            // "subscribed" forever and every later request_bars(force=false)
            // silently no-ops at dedup-L2 (audit D4).
            let station_kline_subscribed = self.station_kline_subscribed.clone();
            let station_kline_handles = self.station_kline_handles.clone();
            let cache_key_task = cache_key.clone();
            let handle = self.spawn_bg_join(async move {
                let kline_iv = KlineInterval::new(&station_interval);
                let sub_set = digdigdig3_station::SubscriptionSet::new().add(
                    exchange_id,
                    station_symbol_canonical.clone(),
                    account_type,
                    [digdigdig3_station::Stream::Kline(kline_iv)],
                );
                let mut report = match station.subscribe(sub_set).await {
                    Ok(r) => r,
                    Err(e) => {
                        crate::dlog!("[Bridge] station kline subscribe failed for {:?} sym={} tf={}: {:?}",
                            exchange_id, station_symbol_wire, station_tf_name, e);
                        // Remove gate keys so a future request_bars can retry.
                        if let Ok(mut subs) = station_kline_subscribed.lock() {
                            subs.remove(&cache_key_task);
                        }
                        if let Ok(mut h) = station_kline_handles.lock() {
                            h.remove(&cache_key_task);
                        }
                        return;
                    }
                };
                if !report.failed.is_empty() {
                    crate::dlog!("[Bridge] station kline partial-fail {:?} sym={} tf={}: {} failed",
                        exchange_id, station_symbol_wire, station_tf_name, report.failed.len());
                }
                let mut bar_n: u64 = 0;
                while let Some(ev) = report.handle.recv().await {
                    if let digdigdig3_station::Event::Bar { point, .. } = ev {
                        let bar = bar_point_to_bar(&point);
                        bar_n = bar_n.wrapping_add(1);
                        // Sample-log to confirm forwarder is alive without flooding.
                        if bar_n <= 3 || bar_n % 50 == 0 {
                            crate::dlog!(
                                "[Bridge::kline-fwd emit BarUpdate] n={} {:?} {} at={:?} tf={} bar_ts={}",
                                bar_n, exchange_id, station_symbol_canonical, account_type, station_tf_name, bar.timestamp,
                            );
                        }
                        let _ = station_tx.send(LiveUpdate::BarUpdate {
                            exchange_id,
                            account_type,
                            // Emit canonical form so app-side symbol == bubble.symbol.
                            symbol: station_symbol_canonical.clone(),
                            bar,
                        });
                    }
                }
                // Forwarder handle dropped (unsubscribe/grace expired/bg-tab
                // throttle killed it). Remove from both gates so a future
                // request_bars can re-spawn instead of no-op'ing at dedup-L2
                // forever (audit D4).
                if let Ok(mut subs) = station_kline_subscribed.lock() {
                    subs.remove(&cache_key_task);
                }
                if let Ok(mut h) = station_kline_handles.lock() {
                    h.remove(&cache_key_task);
                }
                crate::dlog!(
                    "[Bridge] station kline forwarder exited {:?} sym={} tf={}, gate removed",
                    exchange_id, station_symbol_wire, station_tf_name
                );
            });
            if let Ok(mut h) = self.station_kline_handles.lock() {
                h.insert(cache_key.clone(), handle);
            }
        }

        self.spawn_bg(async move {
            // Helper macro to remove the in-flight key on every exit path.
            macro_rules! finish_fetch {
                () => {
                    if let Ok(mut af) = active_fetches.lock() {
                        af.remove(&cache_key);
                    }
                };
            }

            // Wait for the connector to be dialed and, if necessary, for the
            // symbol list to arrive so wire_symbol can resolve canonical →
            // native (e.g. "MUUSDC" → "MU/USDC" on Hyperliquid Spot).
            //
            // Both conditions share the same 150-step × 100 ms = 15 s budget.
            // The dig3 backfill helpers resolve hub.rest themselves, so this
            // is purely a readiness + resolution barrier.
            let mut wire_sym = wire_sym; // shadow with mutable local
            {
                const MAX_ATTEMPTS: u32 = 150; // 15 s total budget
                let mut attempts = 0u32;
                let mut connector_ready = false;
                let mut symbol_resolved = false;
                loop {
                    if !connector_ready && fetch_hub.rest(exchange_id).is_some() {
                        connector_ready = true;
                    }
                    if connector_ready && !symbol_resolved {
                        // Attempt a confident resolution now that the symbol
                        // list may have arrived from the warmup task.
                        if let Some(resolved) = try_resolve_wire_symbol(
                            &station_for_retry, exchange_id, account_type, &symbol_str,
                        ) {
                            wire_sym = resolved;
                            symbol_resolved = true;
                        }
                    }
                    if connector_ready && symbol_resolved {
                        break;
                    }
                    // If connector is ready but symbol list is still empty, keep
                    // polling until the budget runs out — then proceed with the
                    // passthrough fallback (symbol_str unchanged) which matches
                    // venues where canonical == native (e.g. Binance).
                    if connector_ready && attempts >= MAX_ATTEMPTS {
                        crate::dlog!(
                            "[Bridge] wire_symbol unresolved after {}ms: exchange={:?} sym={} at={:?} — using passthrough",
                            attempts * 100, exchange_id, symbol_str, account_type
                        );
                        break;
                    }
                    if !connector_ready && attempts >= MAX_ATTEMPTS {
                        let _ = tx.send(LiveUpdate::Error {
                            exchange_id,
                            account_type,
                            symbol: symbol_str.clone(),
                            timeframe: tf_name.clone(),
                            message: format!("connector {:?} not initialized", exchange_id),
                        });
                        finish_fetch!();
                        return;
                    }
                    attempts += 1;
                    sleep_ms(100).await;
                }
            }

            // ── Phase A: Quick fresh fetch ───────────────────────────────────
            //
            // The pre-Phase-2B cache-fresh short-circuit (`disk_last_ts within
            // 2× interval`) is REMOVED. After Phase 2B `disk_last_ts` comes
            // from station's cross-session DiskStore tail, not from the
            // app's view of the data. That made the freshness gate trigger
            // on every request_bars call within minutes of starting the app,
            // which caused `finish_fetch!()` to run in ~1ms and clear the
            // active_fetches dedup key — so the NEXT request_bars from the
            // same UI event (ConnectorReady sweep / warm walk / etc.) fired
            // Phase A again, and again, and the chart oscillated between
            // the three returned-but-slightly-different fresh_300 windows.
            //
            // The fix at the bridge-level is to always fetch on a fresh key
            // and rely on the request_bars-side dedup (active_fetches +
            // station_kline_subscribed) to suppress repeat callers within a
            // single UI event. Repeat callers from later UI events get a
            // legitimate refresh.
            crate::dlog!("[Bridge] Phase A: {:?} sym={} wire={} interval={} fetching 300 fresh bars",
                exchange_id, symbol_str, wire_sym, interval);

            // Phase 2B: route Phase A historical fetch through dig3-station's
            // `backfill::fetch_history` (REST `get_klines` under the hood;
            // oldest→newest, deduped+sorted inside dig3). `fetch_history` is
            // used rather than `klines_recent` because it RETURNS the error —
            // `klines_recent` swallows REST failures into an empty vec, which
            // left the chart silently stuck on "Loading..." for symbols whose
            // raw string the connector can't resolve. The wire_sym is the
            // venue-native string (resolved from canonical by wire_symbol()).
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(i64::MAX);
            let phase_a_result = digdigdig3_station::backfill::fetch_history(
                &fetch_hub,
                exchange_id,
                &wire_sym,
                account_type,
                &KlineInterval::new(interval.as_str()),
                now_ms,
                300,
            )
            .await;

            let fresh_300: Vec<Bar> = match phase_a_result {
                Ok(points) => {
                    let n = points.len();
                    crate::dlog!("[Bridge] Phase A: got {} bars (dig3)", n);
                    let bars = points.iter().map(bar_point_to_bar).collect::<Vec<_>>();
                    crate::dlog!("[Bridge] Phase A ok: {:?} {} at={:?} tf={} -> {} bars", exchange_id, symbol_str, account_type, tf_name, n);
                    bars
                }
                Err(e) => {
                    crate::dlog!("[Bridge] Phase A error: {:?} sym={} {}", exchange_id, symbol_str, e);
                    let _ = tx.send(LiveUpdate::Error {
                        exchange_id,
                        account_type,
                        symbol: symbol_str.clone(),
                        timeframe: tf_name.clone(),
                        message: format!("get_klines failed: {}", e),
                    });
                    finish_fetch!();
                    return;
                }
            };

            let fresh_first_ts: Option<i64> = fresh_300.first().map(|b| b.timestamp);
            let fresh_last_ts: Option<i64> = fresh_300.last().map(|b| b.timestamp);

            if let (Some(ft), Some(lt)) = (fresh_first_ts, fresh_last_ts) {
                crate::dlog!("[Bridge] Phase A fresh range: {} -> {} ({} bars)", ft, lt, fresh_300.len());
            }

            // Emit Phase A BarsLoaded directly — the station's Series<BarPoint>
            // forwarder receives the same WS klines and upsert_by_ts deduplicates
            // against any cached bars, so the bridge does not need a parallel
            // merge layer.
            crate::dlog!(
                "[Bridge::PhaseA emit BarsLoaded] {:?} {} at={:?} tf={} bars={} first_ts={} last_ts={}",
                exchange_id, symbol_str, account_type, tf_name, fresh_300.len(),
                fresh_first_ts.unwrap_or(0), fresh_last_ts.unwrap_or(0),
            );
            let _ = tx.send(LiveUpdate::BarsLoaded {
                exchange_id,
                account_type,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: fresh_300.clone(),
            });

            // Phase B (legacy heal-loop between disk-cache and Phase A fresh
            // bars) removed. Background backfill (kicked from tick.rs after
            // the BarsLoaded arrives) does the same job correctly now:
            // pages backwards from now_ms with a self-sufficient payload,
            // so the consumer's `update_bars` replacement is safe by the
            // source-of-truth rule. Keeping Phase B duplicated the work and
            // — because it used station_ring as the gap floor — risked
            // emitting an OLD-only BackfillComplete on a freshly-spawned
            // station forwarder, snapping the viewport a month back.
            let _ = had_disk_cache;
            let _ = disk_last_ts;

            finish_fetch!();
        });
        crate::dlog!(
            "[Bridge::request_bars sync-exit] {:?} tf={} total={}us hub={}us wire={}us",
            exchange_id, tf_name_for_log,
            _rb_total.elapsed().as_micros(),
            rb_hub_us, rb_wire_us,
        );
    }

    /// Subscribe to a tick-bar stream (`count` trades per bar) for a tick TF.
    ///
    /// The station `Stream::TickBar(count)` auto-starts the upstream Trade stream
    /// and emits `Event::Bar`; the bridge re-emits as `LiveUpdate::BarUpdate`
    /// (same variant as kline live bars). No REST backfill — live accumulation
    /// only (cold start fills as trades arrive). Range/Volume/Footprint are NOT
    /// here: they are chart_type render transforms of the current TF's bars.
    pub fn request_tick_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        count: u32,
    ) {
        crate::dlog!(
            "[Bridge::request_tick_bars enter] {:?} {} at={:?} count={}",
            exchange_id, symbol, account_type, count,
        );
        // Abort any prior tick subscription for this instrument so a TF switch
        // (100t→1000t, or tick→time) doesn't leave a stale forwarder emitting
        // BarUpdate. Dropping the JoinHandle aborts the task → 30s grace unsub.
        self.abort_derived_sub(exchange_id, symbol, account_type);

        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);

        let stream = digdigdig3_station::Stream::TickBar(count);
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_derived_bar_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        // Track the new task so the next TF switch can abort it.
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Abort the active tick subscription for an instrument, when switching away.
    pub fn abort_tick_bars(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
    }

    /// Abort the active `Stream::Kline` forwarder for an instrument + TF
    /// when the chart switches away (TF change, symbol change, preset swap).
    ///
    /// `.abort()` cancels the forwarder task, which drops the consumer's
    /// `SubscriptionHandle` inside station — that triggers the 30s grace
    /// window after which station unsubscribes the underlying WS channel.
    /// Without this call the old subscriber stays live indefinitely after
    /// a TF switch (logs show e.g. 1H BarUpdates arriving on the broadcast
    /// while the user is already viewing 1m).
    pub fn abort_kline_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        timeframe: &str,
    ) {
        let key: FetchKey = (exchange_id, account_type, symbol.to_string(), timeframe.to_string());
        let mut aborted = false;
        if let Ok(mut h) = self.station_kline_handles.lock() {
            if let Some(handle) = h.remove(&key) {
                handle.abort();
                aborted = true;
            }
        }
        if let Ok(mut subs) = self.station_kline_subscribed.lock() {
            subs.remove(&key);
        }
        crate::dlog!(
            "[Bridge::abort_kline_bars] {:?} {} at={:?} tf={} aborted={}",
            exchange_id, symbol, account_type, timeframe, aborted,
        );
    }

    /// Check whether a `Stream::Kline` forwarder for this instrument + TF is
    /// currently registered as subscribed.
    ///
    /// Read-only liveness probe used by callers (e.g. the unpark warm-cure)
    /// that need to distinguish "bars present + live sub" from "bars present
    /// + dead sub" — post audit-D4 the dedup gate honestly reflects forwarder
    /// liveness (it self-evicts on exit), so this check is now meaningful.
    pub fn is_kline_alive(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        timeframe: &str,
    ) -> bool {
        let key: FetchKey = (exchange_id, account_type, symbol.to_string(), timeframe.to_string());
        self.station_kline_subscribed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&key)
    }

    /// Subscribe to a range-bar stream (`price_units` price-move per bar, ×1e8 fixed-point).
    ///
    /// The station's `Stream::RangeBar(u64)` aggregates a Trade stream into a
    /// new BarPoint whenever |close - bar_open| ≥ `price_units / 1e8`. Cold
    /// start fills as trades arrive (or instantly from the Trade RAM ring +
    /// disk read_tail when station's warm-seed is enabled).
    pub fn request_range_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        price_units: u64,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);

        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);

        let stream = digdigdig3_station::Stream::RangeBar(price_units);
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        // Two-phase cold-open: shallow first paint, one background deepen
        // to full depth (see DERIVED_INITIAL_WARM_N doc).
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_derived_bar_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to a volume-bar stream (`volume_units` total volume per bar, ×1e8 fixed-point).
    pub fn request_volume_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        volume_units: u64,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);

        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);

        let stream = digdigdig3_station::Stream::VolumeBar(volume_units);
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_derived_bar_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to a footprint stream — aggregates trades into per-price-level
    /// buy/sell breakdown bound to a kline interval (`1m`, `5m`, …).
    ///
    /// Unlike Range/Volume/Tick which emit `Event::Bar`, Footprint emits
    /// `Event::Footprint { point: FootprintPoint }` — a separate forwarder task
    /// handles the re-emit into `LiveUpdate::FootprintUpdate`.
    pub fn request_footprint(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        interval: &str,
    ) {
        crate::dlog!(
            "[Bridge::request_footprint enter] {:?} {} at={:?} interval={}",
            exchange_id, symbol, account_type, interval,
        );

        // Note (2026-07-05): the previous version fired a parallel 500-bar
        // kline REST backfill so `bubble.bars` could be used as a candle
        // backdrop under the cluster grid. Owner: klines have nothing to do
        // with footprint — footprint IS its own candle series (each
        // FootprintPoint carries its own OHLC in .open/.high/.low/.close).
        // Backdrop candles are rendered by the footprint renderer directly
        // from bubble.footprint_bars, so no kline sub is needed.

        self.abort_derived_sub(exchange_id, symbol, account_type);

        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let kline_iv = KlineInterval::new(interval);
        let stream = digdigdig3_station::Stream::Footprint(kline_iv);
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        // Two-phase cold-open: shallow first paint, one background deepen to
        // full depth (see FOOTPRINT_INITIAL_WARM_N doc).
        let warm_n = full_warm.min(FOOTPRINT_INITIAL_WARM_N);
        let interval_changed = if let Ok(mut m) = self.footprint_active_interval.lock() {
            let prev = m.insert((exchange_id, symbol.to_string(), account_type), interval.to_string());
            prev.is_some_and(|p| p != interval)
        } else {
            false
        };

        // Get-or-create the per-key footprint Vec BEFORE spawning the
        // forwarder so the first event has a place to land. The key carries
        // no interval — on a bucket-interval change the old points belong to
        // a different timeline, so start the Vec over instead of upserting
        // 5m points after 1m ones.
        let series: SharedFootprintSeries = self.footprint_series
            .entry((exchange_id, account_type, symbol.to_string()))
            .or_insert_with(|| Arc::new(tokio::sync::RwLock::new(Vec::new())))
            .clone();
        if interval_changed {
            if let Ok(mut w) = series.try_write() {
                w.clear();
            }
        }

        let handle = self.spawn_footprint_sub(
            station.clone(), tx.clone(), series.clone(), exchange_id, account_type,
            symbol_str, symbol_wire.clone(), stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }

        // Lazy deepen: after the shallow seed painted, deepen IN PLACE via
        // `Station::rewarm_footprint` — the phase-1 sub keeps running (live
        // bars keep flowing) while the deeper trade window is paged and
        // derived off to the side; the older bars are then spliced in front
        // and delivered downstream as ONE `FootprintHistoryPrepended` batch.
        // The previous teardown+respawn version left the chart with NO live
        // updates for the whole re-warm (minutes on a 100k window in a
        // browser) and re-ran the full pager on every respawn cycle.
        // Guarded by footprint_active_interval — a chart-type switch clears
        // the entry (`abort_derived_chart`), a bucket-interval change
        // replaces it, both making this task a no-op. Single-flight per key:
        // overlapping deepens (e.g. a TF flip-flop re-entering the same
        // interval) collapse to one pager.
        if full_warm > warm_n {
            let interval_owned = interval.to_string();
            let symbol_owned = symbol.to_string();
            let fp_interval_map = self.footprint_active_interval.clone();
            let warm_state = self.derived_warm_state.clone();
            let deepen_in_flight = self.footprint_deepen_in_flight.clone();
            self.spawn_bg(async move {
                sleep_ms(FOOTPRINT_LAZY_DEEPEN_DELAY_MS).await;
                let still_active = fp_interval_map
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&(exchange_id, symbol_owned.clone(), account_type)).cloned())
                    .is_some_and(|iv| iv == interval_owned);
                if !still_active {
                    crate::dlog!(
                        "[Bridge::footprint lazy-deepen] {:?} {} at={:?} — footprint no longer active, skipping",
                        exchange_id, symbol_owned, account_type,
                    );
                    return;
                }
                let flight_key = (exchange_id, symbol_owned.clone(), account_type);
                {
                    let Ok(mut inflight) = deepen_in_flight.lock() else { return; };
                    if !inflight.insert(flight_key.clone()) {
                        crate::dlog!(
                            "[Bridge::footprint lazy-deepen] {:?} {} at={:?} — deepen already in flight, skipping",
                            exchange_id, symbol_owned, account_type,
                        );
                        return;
                    }
                }
                crate::dlog!(
                    "[Bridge::footprint lazy-deepen] {:?} {} at={:?} interval={} — in-place rewarm {} → {}",
                    exchange_id, symbol_owned, account_type, interval_owned, warm_n, full_warm,
                );
                let key = derived_stream_series_key(exchange_id, account_type, &symbol_wire, &stream);
                // The delay races the phase-1 subscribe: its own cold seed
                // pages ~10 REST calls, so at +4s the ring may not be
                // registered yet and rewarm no-ops. Retry on a short timer
                // until the ring exists (bounded; still_active re-checked so
                // a chart-type/TF switch cancels the wait).
                let mut older = Vec::new();
                for _attempt in 0u32..15 {
                    let active = fp_interval_map
                        .lock()
                        .ok()
                        .and_then(|m| m.get(&(exchange_id, symbol_owned.clone(), account_type)).cloned())
                        .is_some_and(|iv| iv == interval_owned);
                    if !active {
                        break;
                    }
                    older = station.rewarm_footprint(&key, &symbol_wire, full_warm).await;
                    if !older.is_empty() {
                        break;
                    }
                    if station
                        .series::<digdigdig3_station::data::FootprintPoint>(&key)
                        .is_some()
                    {
                        // Ring exists but nothing older was produced —
                        // genuine no-op (venue history exhausted / already
                        // at depth), not the startup race.
                        break;
                    }
                    sleep_ms(2000).await;
                }
                crate::dlog!(
                    "[Bridge::footprint lazy-deepen] {:?} {} at={:?} — rewarm spliced {} older bars",
                    exchange_id, symbol_owned, account_type, older.len(),
                );
                if !older.is_empty() {
                    if let Ok(mut ws) = warm_state.lock() {
                        ws.insert(
                            (exchange_id, symbol_owned.clone(), account_type, stream.clone()),
                            full_warm,
                        );
                    }
                    // Mirror the splice into the bridge's own shared Vec so
                    // render-side readers of `footprint_series_for` see the
                    // same deepened window.
                    {
                        let mut w = series.write().await;
                        let old = std::mem::take(&mut *w);
                        let head = old.first().map(|p| p.open_time);
                        let boundary_replaced = head.is_some_and(|h| {
                            older.last().is_some_and(|p| p.open_time == h)
                        });
                        w.reserve(older.len() + old.len());
                        w.extend(older.iter().cloned());
                        w.extend(old.into_iter().skip(usize::from(boundary_replaced)));
                    }
                    let _ = tx.send(LiveUpdate::FootprintHistoryPrepended {
                        exchange_id,
                        account_type,
                        symbol: symbol_owned.clone(),
                        points: older,
                    });
                }
                if let Ok(mut inflight) = deepen_in_flight.lock() {
                    inflight.remove(&flight_key);
                }
            });
        }
    }

    /// Lookup the shared footprint Vec for `(exchange, account, symbol)`.
    ///
    /// Returns `Some(handle)` if a `request_footprint` has been issued for the
    /// key (the Vec lives in `footprint_series` until the bridge is dropped).
    /// Readers call `handle.blocking_read()` to access the points.
    pub fn footprint_series_for(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<SharedFootprintSeries> {
        let key = (exchange_id, account_type, symbol.to_string());
        self.footprint_series.get(&key).map(|r| r.clone())
    }

    /// Abort the active range/volume/footprint subscription, when switching away.
    ///
    /// Single entry point for all non-tick derived subscriptions — they share
    /// the `derived_subs` slot per instrument (one source per chart).
    pub fn abort_derived_chart(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        // Footprint is no longer the active derived mode for this instrument
        // — clear its interval record so the cold-open lazy-deepen task and
        // the scroll-left deepen entry point both see "not active" instead
        // of resurrecting a footprint sub over whatever replaced it.
        if let Ok(mut m) = self.footprint_active_interval.lock() {
            m.remove(&(exchange_id, symbol.to_string(), account_type));
        }
    }

    /// Cancel the CVD overlay subscription for an instrument.
    ///
    /// Called when the last CVD indicator instance for an instrument is
    /// removed (`ChartApp::unsubscribe_cvd_line_if_last`). No-op when no
    /// CVD sub is live.
    pub fn unsubscribe_cvd_line(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_overlay_sub(exchange_id, symbol, account_type, OverlayKind::Cvd);
    }

    /// Cancel the Open Interest overlay subscription for an instrument.
    pub fn unsubscribe_open_interest(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_overlay_sub(exchange_id, symbol, account_type, OverlayKind::OpenInterest);
    }

    /// Cancel the Funding Rate overlay subscription for an instrument.
    pub fn unsubscribe_funding_rate(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_overlay_sub(exchange_id, symbol, account_type, OverlayKind::FundingRate);
    }

    /// Subscribe to a Renko brick stream (`box_size_units` price-move per
    /// brick, ×1e8 fixed-point; `reversal_count` = bricks required to
    /// flip direction, industry standard = 2).
    pub fn request_renko_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        box_size_units: u64,
        reversal_count: u8,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::RenkoBar(box_size_units, reversal_count);
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_renko_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to a Point-and-Figure column stream.
    pub fn request_pnf_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        box_size_units: u64,
        reversal_count: u8,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::PnfBar(box_size_units, reversal_count);
        let box_size = box_size_units as f64 / 1e8;
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_pnf_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), box_size, warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream,
            DerivedSeedExtra::PnfBoxSize(box_size), warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to a Kagi segment stream (`reversal_units` × 1e8).
    pub fn request_kagi_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        reversal_units: u64,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::KagiBar(reversal_units);
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_kagi_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to the Cumulative Volume Delta line for an instrument.
    ///
    /// CVD is an overlay — it does NOT compete with derived chart types
    /// (footprint/renko/pnf/etc.) for the `derived_subs` slot. It lives in
    /// `overlay_subs` keyed by `(instrument, OverlayKind::Cvd)`, so
    /// `request_footprint` followed by `request_cvd_line` (the auto-pair on
    /// chart_type=footprint) leaves BOTH subs alive. The previous version
    /// aborted the just-spawned footprint sub the moment CVD was enabled —
    /// footprint stopped loading (owner-reported 2026-07-05).
    pub fn request_cvd_line(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_overlay_sub(exchange_id, symbol, account_type, OverlayKind::Cvd);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type, OverlayKind::Cvd);
        let stream = digdigdig3_station::Stream::CvdLine;
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        let handle = self.spawn_cvd_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.overlay_subs.lock() {
            subs.insert(sub_key.clone(), handle);
        }
        // CVD is an overlay, not a primary chart_type — it never populates
        // `derived_active_stream` (see `record_active_stream`'s doc comment
        // and this method's own doc comment above), so its still-active
        // check reads `overlay_subs` membership directly instead of using
        // `Self::derived_active_still`.
        let overlay_subs = self.overlay_subs.clone();
        let still_active = move || overlay_subs.lock().is_ok_and(|m| m.contains_key(&sub_key));
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to a TPO Market Profile session stream.
    ///
    /// `freq_minutes` = letter-bucket width (industry default = 30).
    /// `source` selects the upstream channel (Trade or 1m kline) — see
    /// `digdigdig3_station::TpoSource`.
    pub fn request_tpo(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        freq_minutes: u16,
        source: digdigdig3_station::TpoSource,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::TpoProfile(freq_minutes, source);
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_tpo_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    // ── DOM Heatmap subscription (C12) ───────────────────────────────────────

    /// Subscribe to the combined OrderBook + Trade stream for the DOM heatmap
    /// chart-type (C12).
    ///
    /// Spawns two tasks:
    /// 1. `OrderBook` station stream → `LiveUpdate::DomHeatmapSnapshot` per event.
    /// 2. `Trade` station stream → `LiveUpdate::DomTradeDot` per event.
    ///
    /// Both tasks share the same `derived_subs` slot via a combined `JoinHandle`
    /// that cancels both on abort.  Calling `abort_derived_sub` (triggered by any
    /// chart_type switch) stops both tasks.
    pub fn request_dom_heatmap(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_canonical = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);

        let tx_clone = tx.clone();
        let sym_ob = symbol_canonical.clone();
        let sym_wire_ob = symbol_wire.clone();
        let station_ob = station.clone();

        let handle = self.spawn_bg_join(async move {
            // Task 1 — OrderBook snapshots → DomHeatmapSnapshot
            let ob_set = digdigdig3_station::SubscriptionSet::new().add(
                exchange_id, sym_ob.clone(), account_type,
                [digdigdig3_station::Stream::Orderbook],
            );
            let mut ob_report = match station_ob.subscribe(ob_set).await {
                Ok(r) => r,
                Err(e) => {
                    crate::dlog!("[Bridge::dom-heatmap-ob subscribe FAILED] {:?} {} wire={}: {:?}",
                        exchange_id, sym_ob, sym_wire_ob, e);
                    return;
                }
            };

            // Task 2 — Trade ticks → DomTradeDot  (spawned inside, shared lifetime).
            // We are already inside a spawned task → the tokio runtime is
            // entered → spawn directly without going through self.spawn_bg_join
            // (which would require `self` to be 'static — it isn't).
            let tx_t = tx_clone.clone();
            let sym_tr = sym_ob.clone();
            let sym_wire_tr = sym_wire_ob.clone();
            let station_t = station_ob.clone();
            let trade_handle = crate::bridge::spawn_bg_join_nested(async move {
                let tr_set = digdigdig3_station::SubscriptionSet::new().add(
                    exchange_id, sym_tr.clone(), account_type,
                    [digdigdig3_station::Stream::Trade],
                );
                let mut tr_report = match station_t.subscribe(tr_set).await {
                    Ok(r) => r,
                    Err(e) => {
                        crate::dlog!("[Bridge::dom-heatmap-trade subscribe FAILED] {:?} {} wire={}: {:?}",
                            exchange_id, sym_tr, sym_wire_tr, e);
                        return;
                    }
                };
                while let Some(ev) = tr_report.handle.recv().await {
                    if let digdigdig3_station::Event::Trade { point, .. } = ev {
                        let dot = mlc_core::chart::dom_heatmap::DomTradeDot {
                            ts_ms: point.ts_ms,
                            price: point.price,
                            quantity: point.quantity,
                            side: if point.side == 0 { 0 } else { 1 },
                        };
                        let _ = tx_t.send(LiveUpdate::DomTradeDot {
                            exchange_id, account_type,
                            symbol: sym_tr.clone(),
                            dot,
                        });
                    }
                }
            });

            // Drive the order-book stream; cancel trade task when ob exits.
            while let Some(ev) = ob_report.handle.recv().await {
                match ev {
                    digdigdig3_station::Event::OrderbookSnapshot { point, .. } => {
                        // bids/asks are already Vec<(f64, f64)> in dig3-station.
                        let bids_f: Vec<(f64, f64)> = point.bids.iter()
                            .map(|(p, q)| (*p, *q))
                            .collect();
                        let asks_f: Vec<(f64, f64)> = point.asks.iter()
                            .map(|(p, q)| (*p, *q))
                            .collect();
                        let _ = tx_clone.send(LiveUpdate::DomHeatmapSnapshot {
                            exchange_id, account_type,
                            symbol: sym_ob.clone(),
                            ts_ms: point.ts_ms,
                            bids: bids_f,
                            asks: asks_f,
                        });
                    }
                    digdigdig3_station::Event::OrderbookDelta { point, .. } => {
                        // Treat deltas as thin snapshots — the consumer's buffer
                        // coalesces them into the current bucket.
                        let bids_f: Vec<(f64, f64)> = point.bid_changes.iter()
                            .map(|(p, q)| (*p, *q))
                            .collect();
                        let asks_f: Vec<(f64, f64)> = point.ask_changes.iter()
                            .map(|(p, q)| (*p, *q))
                            .collect();
                        let _ = tx_clone.send(LiveUpdate::DomHeatmapSnapshot {
                            exchange_id, account_type,
                            symbol: sym_ob.clone(),
                            ts_ms: point.ts_ms,
                            bids: bids_f,
                            asks: asks_f,
                        });
                    }
                    _ => {}
                }
            }
            trade_handle.abort();
            crate::dlog!("[Bridge::dom-heatmap-ob exited] {:?} {}", exchange_id, sym_ob);
        });

        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
    }

    // ── Liquidation Heatmap subscription (C13) ───────────────────────────────

    /// Subscribe to `Stream::Liquidation` and `Stream::LiquidationBucket` for the
    /// liquidation heatmap chart-type (C13).
    ///
    /// Two tasks are spawned inside a single `JoinHandle`:
    /// 1. `Stream::Liquidation` → `LiveUpdate::LiquidationEvent` per event.
    /// 2. `Stream::LiquidationBucket` → `LiveUpdate::LiquidationBucketUpdate` per poll.
    ///
    /// If an exchange only supports one stream, the other will receive `WireAbsent`
    /// and exit silently.  Both tasks share the same `derived_subs` slot.
    pub fn request_liquidation_heatmap(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_canonical = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);

        let tx_liq = tx.clone();
        let sym_liq = symbol_canonical.clone();
        let sym_wire_liq = symbol_wire.clone();
        let station_liq = station.clone();

        let handle = self.spawn_bg_join(async move {
            // Task 1 — per-event liquidation → LiquidationEvent
            let liq_set = digdigdig3_station::SubscriptionSet::new().add(
                exchange_id, sym_liq.clone(), account_type,
                [digdigdig3_station::Stream::Liquidation],
            );
            let mut liq_report = match station_liq.subscribe(liq_set).await {
                Ok(r) => r,
                Err(e) => {
                    crate::dlog!("[Bridge::liq-heatmap-liq subscribe FAILED] {:?} {} wire={}: {:?}",
                        exchange_id, sym_liq, sym_wire_liq, e);
                    return;
                }
            };

            // Task 2 — bucketed liquidation stats → LiquidationBucketUpdate.
            // Nested spawn: we are already inside a spawned task → use the
            // free `spawn_bg_join_nested` helper which does not require `&self`.
            let tx_bkt = tx_liq.clone();
            let sym_bkt = sym_liq.clone();
            let sym_wire_bkt = sym_wire_liq.clone();
            let station_bkt = station_liq.clone();
            let bucket_handle = crate::bridge::spawn_bg_join_nested(async move {
                let bkt_set = digdigdig3_station::SubscriptionSet::new().add(
                    exchange_id, sym_bkt.clone(), account_type,
                    [digdigdig3_station::Stream::LiquidationBucket],
                );
                let mut bkt_report = match station_bkt.subscribe(bkt_set).await {
                    Ok(r) => r,
                    Err(e) => {
                        crate::dlog!("[Bridge::liq-heatmap-bucket subscribe FAILED] {:?} {} wire={}: {:?}",
                            exchange_id, sym_bkt, sym_wire_bkt, e);
                        return;
                    }
                };
                while let Some(ev) = bkt_report.handle.recv().await {
                    if let digdigdig3_station::Event::LiquidationBucket { point, .. } = ev {
                        let _ = tx_bkt.send(LiveUpdate::LiquidationBucketUpdate {
                            exchange_id, account_type,
                            symbol: sym_bkt.clone(),
                            ts_ms: point.ts_ms,
                            long_liq_usd: point.long_liq_usd,
                            short_liq_usd: point.short_liq_usd,
                        });
                    }
                }
            });

            // Drive the per-event liquidation stream; cancel bucket task when it exits.
            while let Some(ev) = liq_report.handle.recv().await {
                if let digdigdig3_station::Event::Liquidation { point, .. } = ev {
                    let _ = tx_liq.send(LiveUpdate::LiquidationEvent {
                        exchange_id, account_type,
                        symbol: sym_liq.clone(),
                        ts_ms: point.ts_ms,
                        price: point.price,
                        value_usd: point.value,
                        side: point.side,
                    });
                }
            }
            bucket_handle.abort();
            crate::dlog!("[Bridge::liq-heatmap-liq exited] {:?} {}", exchange_id, sym_liq);
        });

        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
    }

    // ── López de Prado quant bar subscriptions (C9) ───────────────────────────

    /// Subscribe to a Dollar Bar stream (López de Prado, AFML ch.2).
    ///
    /// Bar closes when cumulative `price × quantity` ≥ `dollar_threshold`
    /// (whole dollars). Station emits `Event::Bar`; re-emitted here as
    /// `LiveUpdate::BarUpdate` via the same respawn-loop as RangeBar.
    pub fn request_dollar_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        dollar_threshold: u64,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::DollarBar { dollar_threshold };
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_derived_bar_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to a Tick Imbalance Bar stream (López de Prado, AFML ch.2).
    ///
    /// `alpha_x100`: EMA smoothing × 100 (e.g. 20 → 0.20). `min_ticks`: floor.
    pub fn request_tick_imbalance_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        alpha_x100: u16,
        min_ticks: u32,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::TickImbalanceBar { alpha_x100, min_ticks };
        let warm_n = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_derived_bar_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream, warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
    }

    /// Subscribe to a Volume Imbalance Bar stream (López de Prado, AFML ch.2).
    pub fn request_volume_imbalance_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        alpha_x100: u16,
        min_ticks: u32,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::VolumeImbalanceBar { alpha_x100, min_ticks };
        let warm_n = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_derived_bar_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream, warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
    }

    /// Subscribe to a Run Bar stream (López de Prado, AFML ch.2).
    pub fn request_run_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        alpha_x100: u16,
        min_ticks: u32,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::RunBar { alpha_x100, min_ticks };
        let warm_n = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_derived_bar_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream, warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
    }

    /// Subscribe to a Three Line Break stream (`lines_back` = number of
    /// recent lines a new price must exceed before a line prints; default 3).
    pub fn request_three_line_break(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        lines_back: u8,
    ) {
        self.abort_derived_sub(exchange_id, symbol, account_type);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let stream = digdigdig3_station::Stream::ThreeLineBreak { lines_back };
        let full_warm = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let warm_n = full_warm.min(DERIVED_INITIAL_WARM_N);
        self.record_active_stream(exchange_id, symbol, account_type, stream.clone());
        let handle = self.spawn_three_line_break_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream.clone(), warm_n,
        );
        if let Ok(mut subs) = self.derived_subs.lock() {
            subs.insert(sub_key, handle);
        }
        let still_active = self.derived_active_still(exchange_id, symbol, account_type, stream.clone());
        self.spawn_derived_lazy_deepen(
            exchange_id, symbol, account_type, stream, DerivedSeedExtra::None, warm_n, full_warm, still_active,
        );
    }

    /// Subscribe to the Open Interest stream for an instrument (C15).
    ///
    /// Events arrive as `Event::OpenInterest { point: OpenInterestPoint }`.
    /// The forwarder computes Δ = oi_now − oi_prev per event, yielding an
    /// `OiDeltaBar` (delta = 0.0 for the first point — canonical seed value).
    pub fn request_open_interest(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_overlay_sub(exchange_id, symbol, account_type, OverlayKind::OpenInterest);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type, OverlayKind::OpenInterest);
        let stream = digdigdig3_station::Stream::OpenInterest;
        let handle = self.spawn_open_interest_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream,
        );
        if let Ok(mut subs) = self.overlay_subs.lock() {
            subs.insert(sub_key, handle);
        }
    }

    /// Subscribe to the Funding Rate stream for an instrument (C15).
    pub fn request_funding_rate(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        self.abort_overlay_sub(exchange_id, symbol, account_type, OverlayKind::FundingRate);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type, OverlayKind::FundingRate);
        let stream = digdigdig3_station::Stream::FundingRate;
        let handle = self.spawn_funding_rate_sub(
            station, tx, exchange_id, account_type,
            symbol_str, symbol_wire, stream,
        );
        if let Ok(mut subs) = self.overlay_subs.lock() {
            subs.insert(sub_key, handle);
        }
    }

    /// Record `stream` as the active derived subscription for this
    /// instrument, so [`Self::request_deeper_seed_for_active`] (used by the
    /// generic scroll-left trigger, which has no access to per-chart-type UI
    /// parameter state) can reseed it later without needing the caller to
    /// reconstruct box_size/alpha/reversal_count/etc. Called by every
    /// `request_*` derived-subscribe method right alongside registering the
    /// forwarder handle in `derived_subs`.
    fn record_active_stream(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        stream: digdigdig3_station::Stream,
    ) {
        if let Ok(mut m) = self.derived_active_stream.lock() {
            m.insert((exchange_id, symbol.to_string(), account_type), stream);
        }
    }

    /// Build a `still_active` predicate for [`Self::spawn_derived_lazy_deepen`]
    /// backed by `derived_active_stream` — true while `stream` remains the
    /// instrument's recorded active derived subscription (a chart-type
    /// switch overwrites or never sets this entry for the new kind). Used by
    /// every `spawn_derived_lazy_deepen` caller EXCEPT `request_cvd_line`
    /// (CVD is an overlay, not a primary chart_type — see that call site).
    fn derived_active_still(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        stream: digdigdig3_station::Stream,
    ) -> impl Fn() -> bool + Send + Sync + 'static {
        let active_stream = self.derived_active_stream.clone();
        let symbol = symbol.to_string();
        move || {
            active_stream
                .lock()
                .ok()
                .and_then(|m| m.get(&(exchange_id, symbol.clone(), account_type)).cloned())
                .is_some_and(|s| s == stream)
        }
    }

    /// Schedule the background lazy-deepen from `warm_n` (the clamped
    /// [`DERIVED_INITIAL_WARM_N`] first-paint depth) to `full_warm` (the
    /// station-wide [`derived_warm_n()`] default), for the 10 kinds
    /// `Station::rewarm_derived` supports.
    ///
    /// Mirrors `request_footprint`'s lazy-deepen task (bridge.rs, cold-open
    /// two-phase pattern) but reuses [`rewarm_and_emit_derived_prepend`]
    /// instead of hand-rolling the splice: the phase-1 subscribe spawned by
    /// the caller keeps running (live bars keep flowing) while this task
    /// pages the deeper trade window off to the side and delivers the older
    /// points as ONE batched `LiveUpdate::*HistoryPrepended` event. No-op
    /// (returns immediately) if `full_warm <= warm_n` — nothing to deepen.
    ///
    /// `still_active` is re-evaluated before the first attempt and on every
    /// retry — a chart-type/overlay switch cancels the wait. Callers for the
    /// 9 primary derived_subs kinds pass a `derived_active_stream` lookup
    /// (matches `record_active_stream`'s bookkeeping); `request_cvd_line`
    /// (the one overlay-slot kind among the 10) passes an `overlay_subs`
    /// membership check instead, since CVD never populates
    /// `derived_active_stream`.
    ///
    /// Single-flight per `(exchange, symbol, account, stream)` via
    /// `derived_deepen_in_flight` — a chart-type switch that re-enters the
    /// same kind before the first deepen finishes collapses to one pager.
    /// Records the resulting depth in `derived_warm_state` on success so the
    /// scroll-left doubling ladder ([`Self::request_deeper_derived_seed`])
    /// continues from full depth instead of restarting at `warm_n`.
    fn spawn_derived_lazy_deepen(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        stream: digdigdig3_station::Stream,
        extra_params: DerivedSeedExtra,
        warm_n: usize,
        full_warm: usize,
        still_active: impl Fn() -> bool + Send + Sync + 'static,
    ) {
        if full_warm <= warm_n {
            return;
        }
        let symbol_owned = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let station = self.station.clone();
        let tx = self.tx.clone();
        let warm_state = self.derived_warm_state.clone();
        let deepen_in_flight = self.derived_deepen_in_flight.clone();
        self.spawn_bg(async move {
            sleep_ms(FOOTPRINT_LAZY_DEEPEN_DELAY_MS).await;
            if !still_active() {
                crate::dlog!(
                    "[Bridge::derived lazy-deepen] {:?} {} at={:?} stream={:?} — no longer active, skipping",
                    exchange_id, symbol_owned, account_type, stream,
                );
                return;
            }
            let flight_key = (exchange_id, symbol_owned.clone(), account_type, stream.clone());
            {
                let Ok(mut inflight) = deepen_in_flight.lock() else { return; };
                if !inflight.insert(flight_key.clone()) {
                    crate::dlog!(
                        "[Bridge::derived lazy-deepen] {:?} {} at={:?} stream={:?} — deepen already in flight, skipping",
                        exchange_id, symbol_owned, account_type, stream,
                    );
                    return;
                }
            }
            crate::dlog!(
                "[Bridge::derived lazy-deepen] {:?} {} at={:?} stream={:?} — in-place rewarm {} -> {}",
                exchange_id, symbol_owned, account_type, stream, warm_n, full_warm,
            );
            // The delay races the phase-1 subscribe: its own cold seed
            // pages several REST calls, so at +4s the ring may not be
            // registered yet and rewarm no-ops. Retry on a short timer
            // until the ring exists (bounded; still_active re-checked so a
            // chart-type switch cancels the wait) — same pattern as
            // footprint's lazy-deepen retry loop.
            let key = derived_stream_series_key(exchange_id, account_type, &symbol_wire, &stream);
            let mut outcome = DerivedRewarmOutcome::Exhausted;
            for _attempt in 0u32..15 {
                if !still_active() {
                    break;
                }
                outcome = rewarm_and_emit_derived_prepend(
                    &station, &tx, exchange_id, account_type, &symbol_owned, &symbol_wire,
                    &stream, extra_params, full_warm,
                ).await;
                if !matches!(outcome, DerivedRewarmOutcome::Exhausted) {
                    break;
                }
                if station.series::<BarPoint>(&key).is_some()
                    || station_derived_ring_alive(&station, &key, &stream)
                {
                    // Ring exists but nothing older was produced — genuine
                    // no-op (venue history exhausted / already at depth),
                    // not the startup race.
                    break;
                }
                sleep_ms(2000).await;
            }
            match outcome {
                DerivedRewarmOutcome::Prepended => {
                    if let Ok(mut ws) = warm_state.lock() {
                        ws.insert((exchange_id, symbol_owned.clone(), account_type, stream.clone()), full_warm);
                    }
                }
                DerivedRewarmOutcome::Exhausted => {
                    crate::dlog!(
                        "[Bridge::derived lazy-deepen] {:?} {} at={:?} stream={:?} — no older points produced",
                        exchange_id, symbol_owned, account_type, stream,
                    );
                }
                DerivedRewarmOutcome::Fallback => {
                    // Cold-open lazy deepen has no live viewport riding on
                    // it yet — unlike scroll-left, there is nothing to
                    // recover into here. The next scroll-left request for
                    // this key will hit the same RewarmUnsupported/dead-ring
                    // condition and run the full teardown+respawn path.
                    crate::dlog!(
                        "[Bridge::derived lazy-deepen] {:?} {} at={:?} stream={:?} — rewarm unsupported/dead ring, leaving at initial depth",
                        exchange_id, symbol_owned, account_type, stream,
                    );
                }
            }
            if let Ok(mut inflight) = deepen_in_flight.lock() {
                inflight.remove(&flight_key);
            }
        });
    }

    /// Abort the active derived/footprint subscription for an instrument, if any.
    fn abort_derived_sub(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        if let Ok(mut subs) = self.derived_subs.lock() {
            if let Some(h) = subs.remove(&(exchange_id, symbol.to_string(), account_type)) {
                h.abort();
            }
        }
    }

    /// Abort the active overlay subscription for a given kind on an instrument.
    ///
    /// Cheap idempotent no-op when the slot is empty. Called before every
    /// `request_cvd_line` / `request_open_interest` / `request_funding_rate`
    /// so re-subscribes replace, not stack.
    fn abort_overlay_sub(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        kind: OverlayKind,
    ) {
        if let Ok(mut subs) = self.overlay_subs.lock() {
            if let Some(h) = subs.remove(&(exchange_id, symbol.to_string(), account_type, kind)) {
                h.abort();
            }
        }
    }

    /// Current warm-seed depth to request for `(exchange, symbol, account, stream)`.
    ///
    /// Reads `derived_warm_state`; falls back to the station-wide
    /// [`derived_warm_n()`] default when this key has never been reseeded
    /// via [`Self::request_deeper_derived_seed`]. Existing `request_*`
    /// methods call this so a fresh (first-time) subscribe is byte-for-byte
    /// unchanged (map miss → same default as before this feature existed).
    fn derived_warm_for_key(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        stream: &digdigdig3_station::Stream,
    ) -> usize {
        let key = (exchange_id, symbol.to_string(), account_type, stream.clone());
        self.derived_warm_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .copied()
            .unwrap_or_else(derived_warm_n)
    }

    /// Force a deeper history extension of a derived-chart-type subscription,
    /// for progressive scroll-left backfill.
    ///
    /// Two paths, split by [`stream_is_rewarm_capable`]
    /// (derived-prepend-fold-plan-2026-07-07, Step 1):
    ///
    /// - **Prepend-fold (10 kinds — renko/pnf/kagi/three_line_break/
    ///   range_bar/volume_bar/tick_bar/dollar_bar/cvd/tpo)**: in-place via
    ///   [`rewarm_and_emit_derived_prepend`] — the live station subscription
    ///   keeps running, no abort, no teardown. A non-empty result is
    ///   delivered as ONE batched `LiveUpdate::*HistoryPrepended` event and
    ///   `derived_warm_state` is updated to `new_warm_n`. An empty result
    ///   marks the key exhausted (venue history ends here). If the ring is
    ///   dead or `rewarm_derived` reports `RewarmUnsupported`, this falls
    ///   back to the teardown+respawn path below.
    /// - **Teardown+respawn (3 EMA kinds — tick_imbalance_bar/
    ///   volume_imbalance_bar/run_bar — plus the prepend-fold fallback
    ///   above)**: full station-side unsubscribe + resubscribe at doubled
    ///   depth, unchanged from the pre-Phase-1 behavior. Their fold state is
    ///   an EMA carried over the ENTIRE trade history, not reconstructible
    ///   from already-emitted output (see `Station::rewarm_derived`'s
    ///   `RewarmUnsupported` doc comment).
    ///
    /// Shared depth bookkeeping for both paths:
    /// 1. Look up the current warm depth (map default = [`derived_warm_n()`]).
    /// 2. No-op if this key is already marked exhausted (previous call hit
    ///    [`warm_cap()`] or found nothing older).
    /// 3. Compute `new_warm_n = (current * 2).min(warm_cap())`; if that does
    ///    not exceed `current` (already at/above cap), mark exhausted and
    ///    return without touching any subscription.
    ///
    /// `extra_params` provides whatever additional wire-format inputs the
    /// `stream`'s spawn variant needs beyond the plain instrument key (e.g.
    /// `box_size` for `Stream::PnfBar`). Most streams need none — callers
    /// pass `DerivedSeedExtra::None`.
    pub fn request_deeper_derived_seed(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        stream: digdigdig3_station::Stream,
        extra_params: DerivedSeedExtra,
    ) {
        let map_key = (exchange_id, symbol.to_string(), account_type, stream.clone());

        if self.derived_seed_exhausted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&map_key)
        {
            crate::dlog!(
                "[Bridge::request_deeper_derived_seed] {:?} {} at={:?} stream={:?} — already exhausted, no-op",
                exchange_id, symbol, account_type, stream,
            );
            return;
        }

        let current = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let cap = warm_cap();
        let new_warm_n = current.saturating_mul(2).min(cap);
        if new_warm_n <= current {
            self.derived_seed_exhausted
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(map_key);
            crate::dlog!(
                "[Bridge::request_deeper_derived_seed] {:?} {} at={:?} stream={:?} — at cap ({}), marking exhausted",
                exchange_id, symbol, account_type, stream, cap,
            );
            return;
        }

        crate::dlog!(
            "[Bridge::request_deeper_derived_seed] {:?} {} at={:?} stream={:?} {} -> {}",
            exchange_id, symbol, account_type, stream, current, new_warm_n,
        );

        if !stream_is_rewarm_capable(&stream) {
            self.spawn_deeper_derived_seed_teardown(
                exchange_id, symbol, account_type, stream, extra_params, new_warm_n,
            );
            return;
        }

        let flight_key = (exchange_id, symbol.to_string(), account_type, stream.clone());
        {
            let Ok(mut inflight) = self.derived_deepen_in_flight.lock() else { return; };
            if !inflight.insert(flight_key.clone()) {
                crate::dlog!(
                    "[Bridge::request_deeper_derived_seed] {:?} {} at={:?} stream={:?} — deepen already in flight, no-op",
                    exchange_id, symbol, account_type, stream,
                );
                return;
            }
        }

        // Order: abort the mlc-side forwarder FIRST — even on the
        // prepend-fold path, the rewarm below talks to the station directly
        // (not through our forwarder), so the live forwarder is left alone
        // here; it is only aborted inside the teardown fallback (see
        // `run_deeper_derived_seed_teardown`).
        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let warm_state = self.derived_warm_state.clone();
        let seed_exhausted = self.derived_seed_exhausted.clone();
        let deepen_in_flight = self.derived_deepen_in_flight.clone();
        let derived_subs = self.derived_subs.clone();

        self.spawn_bg(async move {
            let key = derived_stream_series_key(exchange_id, account_type, &symbol_wire, &stream);
            let ring_alive = station.series::<BarPoint>(&key).is_some()
                || station_derived_ring_alive(&station, &key, &stream);

            if ring_alive {
                let outcome = rewarm_and_emit_derived_prepend(
                    &station, &tx, exchange_id, account_type, &symbol_str, &symbol_wire,
                    &stream, extra_params, new_warm_n,
                ).await;
                match outcome {
                    DerivedRewarmOutcome::Prepended => {
                        crate::dlog!(
                            "[Bridge::request_deeper_derived_seed] {:?} {} at={:?} stream={:?} — in-place rewarm {} -> {}",
                            exchange_id, symbol_str, account_type, stream, current, new_warm_n,
                        );
                        if let Ok(mut ws) = warm_state.lock() {
                            ws.insert((exchange_id, symbol_str.clone(), account_type, stream.clone()), new_warm_n);
                        }
                        if let Ok(mut inflight) = deepen_in_flight.lock() { inflight.remove(&flight_key); }
                        return;
                    }
                    DerivedRewarmOutcome::Exhausted => {
                        crate::dlog!(
                            "[Bridge::request_deeper_derived_seed] {:?} {} at={:?} stream={:?} — rewarm produced nothing older, marking exhausted",
                            exchange_id, symbol_str, account_type, stream,
                        );
                        if let Ok(mut exhausted) = seed_exhausted.lock() {
                            exhausted.insert((exchange_id, symbol_str.clone(), account_type, stream.clone()));
                        }
                        if let Ok(mut inflight) = deepen_in_flight.lock() { inflight.remove(&flight_key); }
                        return;
                    }
                    DerivedRewarmOutcome::Fallback => {}
                }
            }

            if let Ok(mut inflight) = deepen_in_flight.lock() { inflight.remove(&flight_key); }
            // Fallback: RewarmUnsupported or a dead ring — cold teardown +
            // respawn at the doubled depth is the recovery path. The mlc-side
            // forwarder (if any) must be aborted here, on the fallback branch
            // only — the prepend-fold path above never touched it.
            crate::dlog!(
                "[Bridge::request_deeper_derived_seed] {:?} {} at={:?} stream={:?} — falling back to teardown+respawn",
                exchange_id, symbol_str, account_type, stream,
            );
            if let Ok(mut subs) = derived_subs.lock() {
                if let Some(h) = subs.remove(&sub_key) { h.abort(); }
            }
            run_deeper_derived_seed_teardown(
                station, tx, exchange_id, account_type, symbol_str, symbol_wire,
                sub_key, stream, extra_params, new_warm_n,
                warm_state, seed_exhausted, derived_subs,
            ).await;
        });
    }

    /// Non-rewarm-capable path: abort the mlc-side forwarder, awaited
    /// `force_unsubscribe_and_await`, then re-spawn via
    /// `spawn_deeper_derived_forwarder` at `new_warm_n`. Used directly for
    /// the 3 EMA kinds. The rewarm-fallback branch inside
    /// [`Self::request_deeper_derived_seed`] shares the same body via the
    /// free function [`run_deeper_derived_seed_teardown`] (it runs inside an
    /// already-detached task with no `&self` available).
    fn spawn_deeper_derived_seed_teardown(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        stream: digdigdig3_station::Stream,
        extra_params: DerivedSeedExtra,
        new_warm_n: usize,
    ) {
        // Order: abort the mlc-side forwarder FIRST (drops our own
        // consumer ref / SubscriptionHandle inside the station), THEN force
        // the station-side mux down inside the spawned task below. This
        // avoids racing our own `release_consumer` (triggered by the abort)
        // against the forced teardown for the same key.
        self.abort_derived_sub(exchange_id, symbol, account_type);

        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let warm_state = self.derived_warm_state.clone();
        let seed_exhausted = self.derived_seed_exhausted.clone();
        let derived_subs = self.derived_subs.clone();

        self.spawn_bg(async move {
            run_deeper_derived_seed_teardown(
                station, tx, exchange_id, account_type, symbol_str, symbol_wire,
                sub_key, stream, extra_params, new_warm_n,
                warm_state, seed_exhausted, derived_subs,
            ).await;
        });
    }

    /// Footprint counterpart of [`Self::request_deeper_derived_seed`].
    ///
    /// Kept as a separate method (rather than a `Stream::Footprint` arm in
    /// the generic dispatcher) because footprint needs `self.footprint_series`
    /// — the per-key shared `Vec<FootprintPoint>` — which only a `&self`
    /// method on `DataBridge` can reach; the generic path is deliberately a
    /// detached `'static` task with no `DataBridge` reference (see the
    /// module doc above `spawn_deeper_derived_forwarder`). Same
    /// depth-doubling / exhaustion bookkeeping as the generic path, keyed by
    /// `Stream::Footprint(interval)`.
    pub fn request_deeper_footprint_seed(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        interval: &str,
    ) {
        let kline_iv = KlineInterval::new(interval);
        let stream = digdigdig3_station::Stream::Footprint(kline_iv);
        let map_key = (exchange_id, symbol.to_string(), account_type, stream.clone());

        if self.derived_seed_exhausted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&map_key)
        {
            crate::dlog!(
                "[Bridge::request_deeper_footprint_seed] {:?} {} at={:?} interval={} — already exhausted, no-op",
                exchange_id, symbol, account_type, interval,
            );
            return;
        }

        let current = self.derived_warm_for_key(exchange_id, symbol, account_type, &stream);
        let cap = warm_cap();
        let new_warm_n = current.saturating_mul(2).min(cap);
        if new_warm_n <= current {
            self.derived_seed_exhausted
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(map_key);
            crate::dlog!(
                "[Bridge::request_deeper_footprint_seed] {:?} {} at={:?} interval={} — at cap ({}), marking exhausted",
                exchange_id, symbol, account_type, interval, cap,
            );
            return;
        }

        let station = self.station.clone();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let sub_key = (exchange_id, symbol.to_string(), account_type);
        let warm_state = self.derived_warm_state.clone();
        let seed_exhausted = self.derived_seed_exhausted.clone();
        let derived_subs = self.derived_subs.clone();
        let deepen_in_flight = self.footprint_deepen_in_flight.clone();
        let series: SharedFootprintSeries = self.footprint_series
            .entry((exchange_id, account_type, symbol.to_string()))
            .or_insert_with(|| Arc::new(tokio::sync::RwLock::new(Vec::new())))
            .clone();

        // In-place deepen (same path as the cold-open lazy deepen): the live
        // sub keeps running while `rewarm_footprint` pages the doubled trade
        // window off to the side; older bars land as ONE
        // FootprintHistoryPrepended batch. The old teardown+respawn left the
        // chart without live updates for the whole re-page and replayed the
        // deep ring as thousands of per-point events. Teardown survives only
        // as the fallback for a dead ring (sub broken → cold respawn IS the
        // recovery).
        self.spawn_bg(async move {
            let flight_key = (exchange_id, symbol_str.clone(), account_type);
            {
                let Ok(mut inflight) = deepen_in_flight.lock() else { return; };
                if !inflight.insert(flight_key.clone()) {
                    crate::dlog!(
                        "[Bridge::request_deeper_footprint_seed] {:?} {} at={:?} — deepen already in flight, no-op",
                        exchange_id, symbol_str, account_type,
                    );
                    return;
                }
            }
            let key = derived_stream_series_key(exchange_id, account_type, &symbol_wire, &stream);
            let ring_alive = station
                .series::<digdigdig3_station::data::FootprintPoint>(&key)
                .is_some();
            if ring_alive {
                let older = station.rewarm_footprint(&key, &symbol_wire, new_warm_n).await;
                crate::dlog!(
                    "[Bridge::request_deeper_footprint_seed] {:?} {} at={:?} — rewarm spliced {} older bars (warm {} → {})",
                    exchange_id, symbol_str, account_type, older.len(), current, new_warm_n,
                );
                if older.is_empty() {
                    // Ring alive but nothing older produced — venue history
                    // exhausted at this depth; stop the doubling ladder.
                    if let Ok(mut exhausted) = seed_exhausted.lock() {
                        exhausted.insert((exchange_id, symbol_str.clone(), account_type, stream.clone()));
                    }
                } else {
                    if let Ok(mut ws) = warm_state.lock() {
                        ws.insert(
                            (exchange_id, symbol_str.clone(), account_type, stream.clone()),
                            new_warm_n,
                        );
                    }
                    {
                        let mut w = series.write().await;
                        let old = std::mem::take(&mut *w);
                        let head = old.first().map(|p| p.open_time);
                        let boundary_replaced = head.is_some_and(|h| {
                            older.last().is_some_and(|p| p.open_time == h)
                        });
                        w.reserve(older.len() + old.len());
                        w.extend(older.iter().cloned());
                        w.extend(old.into_iter().skip(usize::from(boundary_replaced)));
                    }
                    let _ = tx.send(LiveUpdate::FootprintHistoryPrepended {
                        exchange_id,
                        account_type,
                        symbol: symbol_str.clone(),
                        points: older,
                    });
                }
                if let Ok(mut inflight) = deepen_in_flight.lock() {
                    inflight.remove(&flight_key);
                }
                return;
            }

            // Fallback: no live ring for the key — the sub is gone/broken;
            // cold respawn at the doubled depth is the recovery path.
            if let Ok(mut subs) = derived_subs.lock() {
                if let Some(h) = subs.remove(&sub_key) {
                    h.abort();
                }
            }
            if let Err(e) = station.force_unsubscribe_and_await(&key).await {
                crate::dlog!(
                    "[Bridge::request_deeper_footprint_seed] force_unsubscribe_and_await failed {:?} {} at={:?}: {:?}",
                    exchange_id, symbol_str, account_type, e,
                );
            }

            if let Ok(mut ws) = warm_state.lock() {
                ws.insert(
                    (exchange_id, symbol_str.clone(), account_type, stream.clone()),
                    new_warm_n,
                );
            }

            let (first_ok_tx, first_ok_rx) = tokio::sync::oneshot::channel::<usize>();
            let inner_handle = spawn_bg_join_nested(footprint_sub_body(
                station, tx, series, exchange_id, account_type,
                symbol_str.clone(), symbol_wire, stream.clone(), new_warm_n,
                Some(first_ok_tx),
            ));

            if let Ok(mut subs) = derived_subs.lock() {
                subs.insert(sub_key, inner_handle);
            }

            let ok_count = first_ok_rx.await.unwrap_or(0);
            if let Ok(mut inflight) = deepen_in_flight.lock() {
                inflight.remove(&flight_key);
            }
            if ok_count == 0 {
                if let Ok(mut exhausted) = seed_exhausted.lock() {
                    exhausted.insert((exchange_id, symbol_str, account_type, stream));
                }
            }
        });
    }

    /// Reseed whatever derived-chart-type subscription is currently active
    /// for `(exchange, symbol, account)`, without the caller needing to know
    /// which kind it is or reconstruct its parameters.
    ///
    /// This is the entry point the generic scroll-left trigger
    /// (`mlc-app/frame_lifecycle/tick.rs`) uses — it only knows the
    /// instrument, not the per-chart-type UI parameters (box_size, alpha,
    /// interval, ...) that were used to start the subscription. Looks up
    /// `footprint_active_interval` first (footprint routes through the
    /// dedicated [`Self::request_deeper_footprint_seed`] since it needs
    /// `self.footprint_series`), then falls back to `derived_active_stream`
    /// for every other derived kind via [`Self::request_deeper_derived_seed`].
    /// No-op (with a debug log) if neither map has an entry for this
    /// instrument — e.g. the chart is on a non-derived (kline) chart_type, or
    /// no derived subscription has ever been started for it.
    pub fn request_deeper_seed_for_active(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) {
        let key = (exchange_id, symbol.to_string(), account_type);

        if let Some(interval) = self.footprint_active_interval
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned()
        {
            self.request_deeper_footprint_seed(exchange_id, symbol, account_type, &interval);
            return;
        }

        if let Some(stream) = self.derived_active_stream
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned()
        {
            self.request_deeper_derived_seed(exchange_id, symbol, account_type, stream, DerivedSeedExtra::None);
            return;
        }

        crate::dlog!(
            "[Bridge::request_deeper_seed_for_active] {:?} {} at={:?} — no active derived subscription recorded, no-op",
            exchange_id, symbol, account_type,
        );
    }

    /// Spawn a derived-bar (Range/Tick/Volume/Dollar/Imbalance/Run) station
    /// subscriber.
    ///
    /// The station emits `Event::Bar` for all these derived-bar types; we
    /// re-emit them as `LiveUpdate::BarUpdate` (same as kline live bars).
    ///
    /// `warm_n` is the cold-start warm-seed depth — plain `request_*` callers
    /// pass [`derived_warm_n()`]; [`Self::request_deeper_derived_seed`]
    /// passes a doubled depth on reseed.
    fn spawn_derived_bar_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(derived_bar_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n, None,
        ))
    }

    /// Spawn a footprint station subscriber.
    ///
    /// Station emits `Event::Footprint { point: FootprintPoint }` (blob-backed
    /// OHLCV + per-price-level buy/sell). Bridge re-emits as
    /// `LiveUpdate::FootprintUpdate`. Same lifecycle as `spawn_derived_bar_sub`
    /// (one task per instrument, aborts on chart-type/TF switch).
    fn spawn_footprint_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        series: SharedFootprintSeries,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(footprint_sub_body(
            station, tx, series, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n, None,
        ))
    }

    /// Spawn a Renko station subscriber. Station emits one
    /// `Event::RenkoBar` per closed brick; we re-emit as
    /// `LiveUpdate::RenkoBarUpdate`.
    fn spawn_renko_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(renko_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n, None,
        ))
    }

    /// Spawn a Three Line Break station subscriber. Station emits one
    /// `Event::ThreeLineBreakUpdate` per closed line; we re-emit as
    /// `LiveUpdate::ThreeLineBreakUpdate`.
    fn spawn_three_line_break_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(three_line_break_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n, None,
        ))
    }

    /// Spawn a P&F station subscriber.
    fn spawn_pnf_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        box_size: f64,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(pnf_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, box_size, warm_n, None,
        ))
    }

    /// Spawn a Kagi station subscriber.
    fn spawn_kagi_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(kagi_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n, None,
        ))
    }

    /// Spawn a CVD station subscriber.
    fn spawn_cvd_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(cvd_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n, None,
        ))
    }

    /// Spawn a TPO station subscriber.
    fn spawn_tpo_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
        warm_n: usize,
    ) -> BgJoinHandle {
        self.spawn_bg_join(tpo_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n, None,
        ))
    }

    /// Spawn an Open Interest station subscriber (C15).
    ///
    /// Listens for `Event::OpenInterest { point }` and emits
    /// `LiveUpdate::OpenInterestUpdate` with a running Δ computed from the
    /// previous OI level. The first event carries delta = 0.0 — canonical seed.
    fn spawn_open_interest_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
    ) -> BgJoinHandle {
        self.spawn_bg_join(async move {
            // Same respawn-loop rationale as the derived bars forwarders:
            // station's auto-heal is kline-only, so non-kline forwarders end
            // with a clean `None` per grace cycle. Without the loop OI silently
            // freezes for the rest of the chart's lifetime.
            // `last_oi` is carried OUTSIDE the loop so the delta stays
            // continuous across respawn cycles — re-subscribing should not
            // make the next event reset the delta to 0.0.
            let stream_label = format!("{:?}", stream);
            let mut last_oi: Option<f64> = None;
            let mut respawn_n: u32 = 0;
            loop {
                let sub_set = digdigdig3_station::SubscriptionSet::new().add(
                    exchange_id, symbol_canonical.clone(), account_type, [stream.clone()],
                );
                let mut report = match station.subscribe(sub_set).await {
                    Ok(r) => r,
                    Err(e) => {
                        crate::dlog!("[Bridge::oi-fwd subscribe Err] respawn={} {:?} {} wire={}: {:?}",
                            respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                        return;
                    }
                };
                // Wire-absent venue (e.g. Binance OI is REST-poll only) → station
                // reports it in `failed[]`, `ok` is empty. Don't loop forever
                // re-subscribing to a non-existent WS channel — exit the task.
                if !report.failed.is_empty() && report.ok.is_empty() {
                    crate::dlog!("[Bridge::oi-fwd wire-absent] {:?} {} stream={} — exiting (failed={:?})",
                        exchange_id, symbol_canonical, stream_label, report.failed);
                    return;
                }
                while let Some(ev) = report.handle.recv().await {
                    if let digdigdig3_station::Event::OpenInterest { point, .. } = ev {
                        let oi = point.open_interest;
                        let delta = match last_oi {
                            Some(prev) => oi - prev,
                            None => 0.0,
                        };
                        last_oi = Some(oi);
                        let bar = mlc_core::types::OiDeltaBar { ts_ms: point.ts_ms, oi, delta };
                        let _ = tx.send(LiveUpdate::OpenInterestUpdate {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            point: bar,
                        });
                    }
                }
                crate::dlog!("[Bridge::oi-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
                    respawn_n, exchange_id, symbol_canonical, stream_label);
                respawn_n = respawn_n.wrapping_add(1);
                sleep_ms(1000).await;
            }
        })
    }

    /// Spawn a Funding Rate station subscriber (C15).
    ///
    /// Listens for `Event::FundingRate { point }` and emits
    /// `LiveUpdate::FundingRateUpdate`. The `interval_hours` field defaults to
    /// 8 hours — the standard perp funding cadence. If the station ever exposes
    /// the interval directly (via a richer FundingRatePoint), that value should
    /// be used instead.
    fn spawn_funding_rate_sub(
        &self,
        station: std::sync::Arc<digdigdig3_station::Station>,
        tx: tokio::sync::broadcast::Sender<LiveUpdate>,
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol_canonical: String,
        symbol_wire: String,
        stream: digdigdig3_station::Stream,
    ) -> BgJoinHandle {
        self.spawn_bg_join(async move {
            let stream_label = format!("{:?}", stream);
            let mut respawn_n: u32 = 0;
            loop {
                let sub_set = digdigdig3_station::SubscriptionSet::new().add(
                    exchange_id, symbol_canonical.clone(), account_type, [stream.clone()],
                );
                let mut report = match station.subscribe(sub_set).await {
                    Ok(r) => r,
                    Err(e) => {
                        crate::dlog!("[Bridge::fr-fwd subscribe FAILED] respawn={} {:?} {} wire={}: {:?}",
                            respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                        return;
                    }
                };
                if !report.failed.is_empty() && report.ok.is_empty() { return; }
                while let Some(ev) = report.handle.recv().await {
                    if let digdigdig3_station::Event::FundingRate { point, .. } = ev {
                        let cell = mlc_core::types::FundingRateCell {
                            ts_ms: point.ts_ms,
                            rate: point.rate,
                            // FundingRatePoint does not carry the interval; default to 8h
                            // (standard cadence for perpetual swaps on Binance, Bybit, OKX).
                            interval_hours: 8,
                        };
                        let _ = tx.send(LiveUpdate::FundingRateUpdate {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            point: cell,
                        });
                    }
                }
                crate::dlog!("[Bridge::fr-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
                    respawn_n, exchange_id, symbol_canonical, stream_label);
                respawn_n = respawn_n.wrapping_add(1);
                sleep_ms(1000).await;
            }
        })
    }

    /// Request the full list of trading symbols from an exchange.
    ///
    /// Non-blocking — spawns an async task. On completion, sends
    /// `SymbolsLoaded` through the update channel.
    ///
    /// Phase 2B: the symbol source is the station's `exchange_info` cache
    /// (Spot + FuturesCross, fetched once by `Station::warmup`). The bridge no
    /// longer issues its own `get_exchange_info` REST calls — the old path
    /// duplicated the warm-up fetch for every exchange at startup. If the
    /// cache is cold for this exchange (warm-up still in flight, or an
    /// exchange outside the warm-up set), a targeted `station.warmup` runs —
    /// it is idempotent: already-connected exchanges short-circuit and
    /// already-cached symbol lists re-broadcast without REST.
    pub fn request_symbols(&self, exchange_id: ExchangeId) {
        let station = self.station.clone();
        let tx = self.tx.clone();

        // Instant cache hit from the station's exchange_info cache.
        let cached = station.symbols(exchange_id);
        if !cached.is_empty() {
            crate::dlog!(
                "[STARTUP-WAVE bridge] request_symbols CACHE-HIT {:?} t={}ms n={}",
                exchange_id, crate::elapsed_ms(), cached.len()
            );
            let _ = tx.send(LiveUpdate::SymbolsLoaded {
                exchange_id,
                symbols: cached,
            });
            return;
        }

        // In-flight guard: one targeted warmup per venue at a time. A second
        // request while the first is still running (or after it failed) would
        // re-issue the exchange_info REST pair immediately — on a venue whose
        // exchange_info persistently fails that loops into a request storm.
        // NOTE: the id is inserted and NEVER removed on failure — a venue
        // that failed its targeted warmup stays parked until app restart
        // (manual retry would need an explicit user action path anyway).
        {
            let mut inflight = self
                .symbols_warmup_inflight
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !inflight.insert(exchange_id) {
                crate::dlog!(
                    "[STARTUP-WAVE bridge] request_symbols SKIP {:?} (warmup already in flight)",
                    exchange_id
                );
                return;
            }
        }

        crate::dlog!(
            "[STARTUP-WAVE bridge] request_symbols STATION-WARMUP {:?} t={}ms (exchange_info cache cold)",
            exchange_id, crate::elapsed_ms()
        );
        let inflight_set = self.symbols_warmup_inflight.clone();
        self.spawn_bg(async move {
            let report = station.warmup(&[exchange_id]).await;
            let symbols = station.symbols(exchange_id);
            if symbols.is_empty() {
                // Intentionally keep the in-flight marker set — see guard
                // comment above (prevents retry storms on broken venues).
                let failed_msg = report
                    .failed
                    .first()
                    .map(|(_, e)| e.clone())
                    .unwrap_or_else(|| "exchange_info empty after station warmup".to_string());
                let _ = tx.send(LiveUpdate::Error {
                    exchange_id,
                    account_type: digdigdig3::AccountType::Spot,
                    symbol: String::new(),
                    timeframe: String::new(),
                    message: failed_msg,
                });
                return;
            }
            // Success: clear the marker so later calls take the cache-hit
            // fast path (and a genuine refresh stays possible).
            inflight_set
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&exchange_id);
            let _ = tx.send(LiveUpdate::SymbolsLoaded {
                exchange_id,
                symbols,
            });
        });
    }

    /// Subscribe to live trade updates via WebSocket.
    ///
    /// Routes the symbol to the shared per-exchange trade actor, which
    /// multiplexes all symbols over a single WS connection.  If no actor
    /// exists yet for this exchange it is spawned automatically.
    ///
    /// Spawn (or re-use) a station Trade forwarder for this key and return the
    /// station's `Series<TradePoint>` handle so panels can read trades directly
    /// from the canonical store. The station's forwarder owns persistence,
    /// REST warm-seed, in-memory ring, and broadcast — the bridge only emits a
    /// flattened `LiveUpdate::TradeUpdate` for chart-side bar aggregation.
    ///
    /// The returned handle is `Some(...)` once the subscribe future completes
    /// inside the spawned task. The bridge does NOT block on subscribe — the
    /// caller stores `None` initially and the consumer can re-query
    /// `station.series::<TradePoint>(&key)` later if needed; in practice the
    /// subscribe runs in <100ms and the handle returned here is read-only fine
    /// when nothing has subscribed yet (returns `None`).
    pub fn subscribe_trades(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<SharedTradeSeries> {
        let gate_key = (exchange_id, account_type, symbol.to_string());

        // C3: spawn a station trade-subscriber once per (exchange_id, account_type, symbol).
        // Subsequent calls for the same key (multiple panels sharing the same stream) skip
        // the spawn — the existing forwarder is still alive (or in 30s grace window).
        {
            let mut subs = self.station_trade_subscribed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if subs.insert(gate_key.clone()) {
                // First subscriber for this key: spawn the station forwarder.
                let station = self.station.clone();
                let tx = self.tx.clone();
                let station_trade_subscribed = self.station_trade_subscribed.clone();
                // Canonical form — emitted back in TradeUpdate.symbol so app-side lookups match.
                let symbol_canonical = symbol.to_string();
                // Wire form — passed to dig3 station add_raw.
                let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
                let gate_key_task = gate_key.clone();
                self.spawn_bg(async move {
                    let sub_set = digdigdig3_station::SubscriptionSet::new().add(
                        exchange_id,
                        symbol_canonical.clone(),
                        account_type,
                        [digdigdig3_station::Stream::Trade],
                    );
                    let mut report = match station.subscribe(sub_set).await {
                        Ok(r) => r,
                        Err(e) => {
                            crate::dlog!(
                                "[Bridge] station trade subscribe failed {:?} sym={}: {:?}",
                                exchange_id, symbol_wire, e
                            );
                            // Remove gate key so a future subscribe_trades can retry.
                            let mut subs = station_trade_subscribed
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            subs.remove(&gate_key_task);
                            return;
                        }
                    };
                    if !report.failed.is_empty() {
                        crate::dlog!(
                            "[Bridge] station trade partial-fail {:?} sym={}: {} failed",
                            exchange_id, symbol_wire, report.failed.len()
                        );
                    }
                    // Drain station trade events and re-emit on the bridge broadcast.
                    // The station's Series<TradePoint> forwarder already wrote each
                    // point into its own ring + DiskStore; we only translate the
                    // event into the legacy LiveUpdate::TradeUpdate broadcast so
                    // tick.rs can drive time-TF bar aggregation from raw trades.
                    while let Some(ev) = report.handle.recv().await {
                        if let digdigdig3_station::Event::Trade { point, .. } = ev {
                            let _ = tx.send(LiveUpdate::TradeUpdate {
                                exchange_id,
                                account_type,
                                // Emit canonical form so app-side panel lookups match.
                                symbol: symbol_canonical.clone(),
                                price: point.price,
                                quantity: point.quantity,
                                timestamp: point.ts_ms,
                                // TradePoint.side 0 = buyer maker, 1 = seller maker.
                                // is_buyer_maker=true when side==0 (buyer made the trade).
                                is_buyer_maker: point.side == 0,
                            });
                        }
                    }
                    // Forwarder handle dropped (unsubscribe/grace expired).
                    // Remove from gate so a future subscribe_trades can re-spawn.
                    let mut subs = station_trade_subscribed
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    subs.remove(&gate_key_task);
                    crate::dlog!(
                        "[Bridge] station trade forwarder exited {:?} sym={}, gate removed",
                        exchange_id, symbol_wire
                    );
                });
            }
        }

        // Best-effort sync return: the station's Series is registered before
        // the first event broadcast; if subscribe hasn't completed yet the
        // caller will see None on the first frame and Some(...) shortly after
        // (panel pull_new_trades is idempotent and tolerates a missing handle).
        let wire = self.wire_symbol(exchange_id, account_type, symbol);
        let key = StationSeriesKey::new(exchange_id, account_type, wire, StationKind::Trade);
        self.station.series::<TradePoint>(&key)
    }

    /// Subscribe to a single symbol's mini ticker stream via WebSocket.
    ///
    /// Routes the symbol to the station ticker stream (C5).
    pub fn subscribe_mini_ticker(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        // C5: spawn station ticker side-channel once per (exchange_id, symbol, account_type).
        // The gate key uses the canonical symbol (what the app tracks in the watchlist).
        let ticker_key = (exchange_id, symbol.to_string(), account_type);
        {
            let mut subs = self.station_ticker_subscribed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !subs.insert(ticker_key.clone()) {
                return;
            }
        }
        let station = self.station.clone();
        let tx = self.tx.clone();
        let subs_set = self.station_ticker_subscribed.clone();
        // Canonical form — emitted back in MiniTickerUpdate.symbol AND passed
        // to dig3 station via `.add()` (the normalizer-translated path).
        // Pre-2026-06-24 we did `wire_symbol(...) → .add_raw(...)`, but
        // `wire_symbol` reads `station.symbols(exchange)` which is empty on
        // the very first subscribe after boot — wasm shell hit this on OKX
        // (raw `"BTCUSDT"` passed through, OKX requires `"BTC-USDT"`, sub
        // rejected with code 60018). dig3's own `SubscriptionSet::add` runs
        // the canonical → native translation per-venue via `SymbolNormalizer`
        // (see digdigdig3-station/src/subscription.rs:196), which does NOT
        // depend on the station's REST symbol catalogue, so it works from
        // the first call. Keep `symbol_wire` only for the legacy logging
        // path below; the real subscription uses canonical now.
        let symbol_canonical = symbol.to_string();
        let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
        let key_for_cleanup = ticker_key;
        let canonical_for_sub = symbol_canonical.clone();
        self.spawn_bg(async move {
            let sub_set = digdigdig3_station::SubscriptionSet::new().add(
                exchange_id,
                canonical_for_sub,
                account_type,
                [digdigdig3_station::Stream::Ticker],
            );
            let mut report = match station.subscribe(sub_set).await {
                Ok(r) => r,
                Err(e) => {
                    crate::dlog!("[Bridge] station ticker subscribe failed {:?} sym={}: {:?}",
                        exchange_id, symbol_wire, e);
                    if let Ok(mut s) = subs_set.lock() {
                        s.remove(&key_for_cleanup);
                    }
                    return;
                }
            };
            if !report.failed.is_empty() {
                crate::dlog!("[Bridge] station ticker partial-fail {:?} sym={}: {} failed",
                    exchange_id, symbol_wire, report.failed.len());
            }
            while let Some(ev) = report.handle.recv().await {
                if let digdigdig3_station::Event::Ticker { point, .. } = ev {
                    // NaN sentinel → None for optional 24h-stats fields.
                    let opt = |v: f64| if v.is_nan() { None } else { Some(v) };
                    let _ = tx.send(LiveUpdate::MiniTickerUpdate {
                        exchange_id,
                        account_type,
                        // Emit canonical form so watchlist ticker map keyed by canonical symbol matches.
                        symbol: symbol_canonical.clone(),
                        last_price: point.last,
                        price_change_percent: opt(point.change_pct_24h),
                        high_price: opt(point.high_24h),
                        low_price: opt(point.low_24h),
                        volume: opt(point.vol_24h),
                    });
                }
            }
            // Forwarder handle dropped. Remove gate so a future subscribe_mini_ticker can re-spawn.
            if let Ok(mut s) = subs_set.lock() {
                s.remove(&key_for_cleanup);
            }
            crate::dlog!("[Bridge] station ticker forwarder exited {:?} sym={}, gate removed",
                exchange_id, symbol_wire);
        });
    }

    /// Subscribe to L2 orderbook depth updates via WebSocket.
    ///
    /// Creates (or returns) the per-key `OrderBookTracker` in `ob_trackers` and
    /// triggers the station orderbook subscriber (C4), which handles REST seed and
    /// incremental WS deltas. The station forwarder writes directly into the tracker
    /// via `apply_snapshot` / `apply_delta` BEFORE emitting `LiveUpdate::Orderbook*`
    /// on the broadcast channel — panels that hold the tracker handle always see a
    /// fresh book by the time the drain loop runs.
    pub fn subscribe_depth(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        // Ensure the per-key tracker exists; subsequent subscribers share the same Arc.
        let gate_key = (exchange_id, account_type, symbol.to_string());
        let tracker: SharedOrderBookTracker = self.ob_trackers
            .entry(gate_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::RwLock::new(
                digdigdig3_station::orderbook::OrderBookTracker::new(symbol),
            )))
            .clone();

        // REST seed: always request a fresh full-book snapshot on every subscribe_depth
        // call, independent of the delta-forwarder gate. The tracker is STATE — every
        // (re)subscribing panel needs a valid starting book, and deltas alone leave it
        // blank until the next WS snapshot arrives (which may be minutes away on low-
        // volume symbols). Write into the tracker first, then emit the broadcast so
        // tick.rs L2-tape gets the event timestamp.
        {
            let hub = self.station.hub();
            let tx = self.tx.clone();
            let symbol_canonical = symbol.to_string();
            let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
            let tracker_clone = tracker.clone();
            self.spawn_bg(async move {
                let Some(rest) = hub.rest(exchange_id) else { return };
                let ob = match rest
                    .get_orderbook(
                        digdigdig3::core::types::SymbolInput::Raw(&symbol_wire),
                        Some(1000),
                        account_type,
                    )
                    .await
                {
                    Ok(ob) if !(ob.bids.is_empty() && ob.asks.is_empty()) => ob,
                    _ => return,
                };
                // Write into the per-key tracker so DOM/heatmap see live levels.
                let _ = tracker_clone.write().await.apply_snapshot(&ob);
                let bids: Vec<(f64, f64)> = ob.bids.iter().map(|l| (l.price, l.size)).collect();
                let asks: Vec<(f64, f64)> = ob.asks.iter().map(|l| (l.price, l.size)).collect();
                let _ = tx.send(LiveUpdate::OrderbookSnapshot {
                    exchange_id,
                    account_type,
                    symbol: symbol_canonical,
                    bids,
                    asks,
                    timestamp: ob.timestamp,
                    source: OrderbookSource::Rest,
                });
            });
        }

        // C4: station delta forwarder (idempotent — gate on station_orderbook_subscribed).
        // The gate governs only the steady-state delta recv-loop; the REST seed above is
        // independent.
        let mut subs = self.station_orderbook_subscribed
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let gate_was_open = subs.insert(gate_key.clone());
        drop(subs);
        if gate_was_open {
            let station = self.station.clone();
            let tx = self.tx.clone();
            let station_orderbook_subscribed = self.station_orderbook_subscribed.clone();
            // Canonical form — emitted back in orderbook events so app-side lookups match.
            let symbol_canonical = symbol.to_string();
            // Wire form — passed to dig3 station add_raw.
            let symbol_wire = self.wire_symbol(exchange_id, account_type, symbol);
            let key_for_cleanup = gate_key;
            let tracker_clone = tracker.clone();
            self.spawn_bg(async move {
                let sub_set = digdigdig3_station::SubscriptionSet::new().add(
                    exchange_id,
                    symbol_canonical.clone(),
                    account_type,
                    [digdigdig3_station::Stream::OrderbookDelta],
                );
                let mut report = match station.subscribe(sub_set).await {
                    Ok(r) => r,
                    Err(e) => {
                        crate::dlog!(
                            "[Bridge] station ob subscribe failed {:?} sym={}: {:?}",
                            exchange_id, symbol_wire, e
                        );
                        if let Ok(mut subs) = station_orderbook_subscribed.lock() {
                            subs.remove(&key_for_cleanup);
                        }
                        return;
                    }
                };
                if !report.failed.is_empty() {
                    crate::dlog!(
                        "[Bridge] station ob partial-fail {:?} sym={}: {} failed",
                        exchange_id, symbol_wire, report.failed.len()
                    );
                }
                while let Some(ev) = report.handle.recv().await {
                    match ev {
                        digdigdig3_station::Event::OrderbookSnapshot { point, .. } => {
                            // Write into the per-key tracker first (panels read this
                            // via blocking_read in their tick()).
                            let ob = ob_snapshot_point_to_orderbook(&point);
                            let _ = tracker_clone.write().await.apply_snapshot(&ob);
                            // Then emit on broadcast for L2-tape per-event-ts consumer.
                            let bids = strip_ob_padding(&point.bids);
                            let asks = strip_ob_padding(&point.asks);
                            let _ = tx.send(LiveUpdate::OrderbookSnapshot {
                                exchange_id,
                                account_type,
                                symbol: symbol_canonical.clone(),
                                bids,
                                asks,
                                timestamp: point.ts_ms,
                                source: OrderbookSource::Rest,
                            });
                        }
                        digdigdig3_station::Event::OrderbookDelta { point, .. } => {
                            // Write into the tracker before broadcasting.
                            let delta = ob_delta_point_to_orderbook_delta(&point);
                            let _ = tracker_clone.write().await.apply_delta(&delta);
                            let bids = strip_ob_padding(&point.bid_changes);
                            let asks = strip_ob_padding(&point.ask_changes);
                            let _ = tx.send(LiveUpdate::OrderbookDelta {
                                exchange_id,
                                account_type,
                                symbol: symbol_canonical.clone(),
                                bids,
                                asks,
                                timestamp: point.ts_ms,
                            });
                        }
                        _ => {}
                    }
                }
                if let Ok(mut subs) = station_orderbook_subscribed.lock() {
                    subs.remove(&key_for_cleanup);
                }
                crate::dlog!(
                    "[Bridge] station ob forwarder exited {:?} sym={}, gate removed",
                    exchange_id, symbol_wire
                );
            });
        }
    }

    /// Subscribe to the live orderbook for `(exchange, symbol, account_type)`.
    ///
    /// Starts the station depth subscriber (idempotent via C4 gate) and returns
    /// the `SharedOrderBookTracker` handle for the key so the caller (DOM, heatmap,
    /// L2-tape, paper-engine) can read the live reconstructed book directly via
    /// `blocking_read()`.
    ///
    /// Returns `None` only when `subscribe_depth` has not yet created the tracker entry
    /// (in practice the entry is created synchronously inside `subscribe_depth` before
    /// this returns, so `None` is unreachable).
    pub fn subscribe_orderbook(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<SharedOrderBookTracker> {
        // subscribe_depth creates the tracker in ob_trackers (idempotent).
        self.subscribe_depth(exchange_id, symbol, account_type);
        let key = (exchange_id, account_type, symbol.to_string());
        self.ob_trackers.get(&key).map(|r| r.clone())
    }

    /// Unsubscribe one consumer from the live orderbook.
    ///
    /// Station owns the orderbook lifetime via the 30-second unsubscribe grace window
    /// configured in the builder. The per-key tracker stays in `ob_trackers` as long
    /// as the station forwarder lives; a future subscribe call re-uses it without
    /// re-fetching the REST snapshot. No bookkeeping is needed here.

    /// Look up an existing tracker without starting a subscription.
    ///
    /// Returns `None` when no `subscribe_orderbook` call has been made for this
    /// key yet (i.e. the panel has not subscribed). Used by `PaperEngine` to
    /// read BBO without triggering a new WS stream.
    pub fn ob_tracker_for(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<SharedOrderBookTracker> {
        let key = (exchange_id, account_type, symbol.to_string());
        self.ob_trackers.get(&key).map(|r| r.clone())
    }

    pub fn unsubscribe_orderbook(
        &self,
        _exchange_id: ExchangeId,
        _symbol: &str,
        _account_type: AccountType,
    ) {}

    /// Connect a private (authenticated) WebSocket stream for the given exchange.
    ///
    /// Spawns a dedicated private-stream actor that authenticates with the
    /// exchange and subscribes to order updates, balance changes, and position
    /// changes.  Events are broadcast as [`LiveUpdate::OrderUpdate`],
    /// [`LiveUpdate::BalanceUpdate`], and [`LiveUpdate::PositionUpdate`].
    ///
    /// Calling this when a private actor is already running for the same
    /// `(exchange_id, account_type)` pair is a no-op — the existing actor is
    /// reused.
    pub fn connect_private(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        credentials: digdigdig3::Credentials,
    ) {
        // C5: spawn station private subscriber once per (exchange_id, account_type).
        // One authenticated WS covers all private streams (order/balance/position).
        // WASM: add_authenticated returns UnsupportedOperation → subscribe fails → silent skip.
        let private_key = (exchange_id, account_type);
        {
            let mut subs = self.station_private_subscribed
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !subs.insert(private_key) {
                return;
            }
        }
        let station = self.station.clone();
        let tx = self.tx.clone();
        let subs_set = self.station_private_subscribed.clone();
        self.spawn_bg(async move {
            let sub_set = digdigdig3_station::SubscriptionSet::new().add_authenticated(
                exchange_id,
                account_type,
                credentials,
                [
                    digdigdig3_station::Stream::OrderUpdate,
                    digdigdig3_station::Stream::BalanceUpdate,
                    digdigdig3_station::Stream::PositionUpdate,
                ],
            );
            let mut report = match station.subscribe(sub_set).await {
                Ok(r) => r,
                Err(e) => {
                    crate::dlog!("[Bridge] station private subscribe failed {:?} at={:?}: {:?}",
                        exchange_id, account_type, e);
                    if let Ok(mut s) = subs_set.lock() {
                        s.remove(&private_key);
                    }
                    return;
                }
            };
            if !report.failed.is_empty() {
                crate::dlog!("[Bridge] station private partial-fail {:?} at={:?}: {} failed",
                    exchange_id, account_type, report.failed.len());
            }
            while let Some(ev) = report.handle.recv().await {
                match ev {
                    digdigdig3_station::Event::OrderUpdate { symbol, point, .. } => {
                        use digdigdig3::{OrderSide, OrderStatus, OrderType};
                        let side = if point.side == 0 { OrderSide::Buy } else { OrderSide::Sell };
                        let order_type = match point.order_type {
                            1 => OrderType::Limit { price: point.price },
                            _ => OrderType::Market,
                        };
                        let status = match point.status {
                            1 => OrderStatus::PartiallyFilled,
                            2 => OrderStatus::Filled,
                            3 => OrderStatus::Canceled,
                            4 => OrderStatus::Rejected,
                            _ => OrderStatus::New,
                        };
                        // order_id string is lost in point (hashed); use hash as surrogate.
                        let event = digdigdig3::OrderUpdateEvent {
                            order_id: format!("{}", point.order_id_hash),
                            client_order_id: if point.client_id_hash != 0 {
                                Some(format!("{}", point.client_id_hash))
                            } else {
                                None
                            },
                            side,
                            order_type,
                            status,
                            price: if point.price != 0.0 { Some(point.price) } else { None },
                            quantity: point.qty,
                            filled_quantity: point.filled_qty,
                            average_price: if point.avg_price != 0.0 { Some(point.avg_price) } else { None },
                            last_fill_price: if point.last_fill_price != 0.0 { Some(point.last_fill_price) } else { None },
                            last_fill_quantity: if point.last_fill_qty != 0.0 { Some(point.last_fill_qty) } else { None },
                            last_fill_commission: if point.last_fill_commission != 0.0 { Some(point.last_fill_commission) } else { None },
                            commission_asset: None,
                            trade_id: None,
                            timestamp: point.ts_ms,
                        };
                        let _ = tx.send(LiveUpdate::OrderUpdate {
                            exchange_id,
                            account_type,
                            symbol,
                            event,
                        });
                    }
                    digdigdig3_station::Event::BalanceUpdate { symbol, point, .. } => {
                        // asset string is lost in point (hashed); use symbol from event or hash.
                        // Station BalanceUpdate symbol = asset ticker (e.g. "USDT") per Event docs.
                        let asset = if symbol.is_empty() {
                            format!("{}", point.asset_hash)
                        } else {
                            symbol
                        };
                        let event = digdigdig3::BalanceUpdateEvent {
                            asset,
                            free: point.free,
                            locked: point.locked,
                            total: point.total,
                            delta: if point.delta != 0.0 { Some(point.delta) } else { None },
                            reason: None,
                            timestamp: point.ts_ms,
                        };
                        let _ = tx.send(LiveUpdate::BalanceUpdate {
                            exchange_id,
                            account_type,
                            event,
                        });
                    }
                    digdigdig3_station::Event::PositionUpdate { symbol, point, .. } => {
                        use digdigdig3::PositionSide;
                        let side = match point.side {
                            1 => PositionSide::Short,
                            2 => PositionSide::Both,
                            _ => PositionSide::Long,
                        };
                        let event = digdigdig3::PositionUpdateEvent {
                            side,
                            quantity: point.qty,
                            entry_price: point.entry_price,
                            mark_price: if point.mark_price != 0.0 { Some(point.mark_price) } else { None },
                            unrealized_pnl: point.unrealized_pnl,
                            realized_pnl: if point.realized_pnl != 0.0 { Some(point.realized_pnl) } else { None },
                            liquidation_price: if point.liquidation_price != 0.0 { Some(point.liquidation_price) } else { None },
                            leverage: None,
                            margin_type: None,
                            reason: None,
                            timestamp: point.ts_ms,
                        };
                        let _ = tx.send(LiveUpdate::PositionUpdate {
                            exchange_id,
                            account_type,
                            symbol,
                            event,
                        });
                    }
                    _ => {}
                }
            }
            // Forwarder handle dropped. Remove gate so a future connect_private can re-spawn.
            if let Ok(mut s) = subs_set.lock() {
                s.remove(&private_key);
            }
            crate::dlog!("[Bridge] station private forwarder exited {:?} at={:?}, gate removed",
                exchange_id, account_type);
        });
    }

    /// Remove one consumer interest in depth stream for this symbol.
    ///
    /// Station manages unsubscribe lifecycle via the 30s grace window configured
    /// in the builder. The gate key is removed when the forwarder task exits.
    pub fn unsubscribe_depth(&self, _exchange_id: ExchangeId, _symbol: &str, _account_type: AccountType) {}

    /// No-op after Phase 2B step 2.
    ///
    /// The station owns trade-stream lifetime: each `subscribe_trades` returns
    /// a `SharedTradeSeries` handle bound to the station's forwarder; when the
    /// caller drops it (panel unmount / symbol switch), the station's 30 s
    /// `unsubscribe_grace` window kicks in and the WS forwarder shuts down
    /// once no live receivers remain. There is no mlc-side refcount to
    /// decrement anymore.
    pub fn unsubscribe_trades(&self, _exchange_id: ExchangeId, _symbol: &str, _account_type: AccountType) {
        // Intentionally empty — see doc comment above.
    }

    /// Abort trade WebSocket subscriptions (not mini ticker).
    ///
    /// Call this when switching symbols to stop old trade streams.
    /// Mini ticker subscriptions (watchlist) are preserved.
    pub fn unsubscribe_all(&self) {
        // Station manages trade subscription lifecycle via grace windows.
        // No legacy ws_actor trade streams remain.
    }

    /// Unsubscribe a single mini-ticker symbol (when symbol removed from watchlist).
    ///
    /// Sends a `RemoveSymbol` command to the ticker actor; the actor applies a
    /// 30-second grace period before actually unsubscribing from the exchange.
    pub fn unsubscribe_mini_ticker(&self, _exchange_id: ExchangeId, _symbol: &str, _account_type: AccountType) {
        // Station manages ticker subscription lifecycle via grace windows.
    }

    /// Stop all WebSocket subscriptions for a specific exchange.
    ///
    /// Station manages subscription lifecycle; legacy ws_actor maps are removed.
    pub fn unsubscribe_exchange(&self, _exchange_id: ExchangeId) {
        // No legacy ws_actor map remains. Station grace windows handle cleanup.
    }

    /// Disable a connector — remove from pool and stop all WS tasks.
    pub fn disable_connector(&self, exchange_id: ExchangeId) {
        self.unsubscribe_exchange(exchange_id);
        let was_connected = self.hub.is_connected(exchange_id);
        self.hub.shutdown(exchange_id);
        if was_connected {
            crate::dlog!("[Bridge] Removed connector: {:?}", exchange_id);
        }
    }

    /// Enable a connector — initialize it if not already in pool.
    pub fn enable_connector(&self, exchange_id: ExchangeId) {
        self.ensure_connector(exchange_id);
    }

    /// Count active WebSocket tasks for a specific exchange.
    ///
    /// Returns 0 — legacy ws_actor map removed. Station reports liveness via `ws_health`.
    pub fn ws_task_count(&self, _exchange_id: ExchangeId) -> usize {
        0
    }

    /// Count total active WebSocket tasks across all exchanges.
    ///
    /// Returns 0 — legacy ws_actor map removed. Station reports liveness via `ws_health`.
    pub fn ws_task_count_total(&self) -> usize {
        0
    }

    /// Get summary metrics for all active connectors.
    ///
    /// Returns one entry per exchange currently in the connector pool,
    /// containing the exchange ID, its `ConnectorStats`, and the number
    /// of active WebSocket tasks.
    ///
    /// Note: depends on `ConnectorStats` and `AnyConnector::metrics()` which
    /// are added by a parallel implementation task. This method will not
    /// compile until those additions are present.
    pub fn collect_metrics(&self) -> Vec<(ExchangeId, digdigdig3::core::types::ConnectorStats, usize)> {
        let mut results = Vec::new();
        let rtt_handles_snapshot = self.ws_rtt_handles.lock().ok()
            .map(|g| g.clone());
        for eid in self.hub.list_connected() {
            let mut stats = if let Some(connector) = self.hub.rest(eid) {
                connector.metrics()
            } else {
                digdigdig3::core::types::ConnectorStats::default()
            };
            // Overlay WS ping RTT if a live handle exists for this exchange.
            if let Some(ref handles) = rtt_handles_snapshot {
                if let Some(rtt_handle) = handles.get(&eid) {
                    if let Ok(rtt) = rtt_handle.try_lock() {
                        stats.ws_ping_rtt_ms = *rtt;
                    }
                }
            }
            let ws_count = self.ws_task_count(eid);
            results.push((eid, stats, ws_count));
        }
        results
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Agent API accessors
    // ─────────────────────────────────────────────────────────────────────────

    /// Get cached bars for a specific (exchange, account_type, symbol, timeframe) key.
    ///
    /// Returns `None` if the key is not in the cache.
    pub fn get_cached_bars(
        &self,
        exchange_id: &ExchangeId,
        account_type: AccountType,
        symbol: &str,
        timeframe: &str,
    ) -> Option<Vec<Bar>> {
        let key = station_bar_key(*exchange_id, account_type, symbol, timeframe);
        let bars = station_read_bars(&self.station, &key);
        if bars.is_empty() { None } else { Some(bars) }
    }

    /// Request background backfill of historical bars for a symbol/timeframe.
    ///
    /// Non-blocking — spawns an async task that pages backward through history
    /// until `target_bars` are accumulated in the cache. On completion, sends
    /// `BackfillComplete` through the update channel.
    ///
    /// This is Layer 2 of the data loading pipeline. It is designed to run
    /// silently after `request_bars` (Layer 1) has already populated the cache
    /// with recent bars, extending history further into the past.
    ///
    /// If the cache already holds `>= target_bars`, or a backfill for this key
    /// is already in-flight, the call returns immediately without spawning a task.
    pub fn request_background_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,
        target_bars: u32,
    ) {
        // Phase 2B: background backfill paginates through the STATION hub
        // (shared connector pool with live WS + Phase A/B fetch).
        let fetch_hub = self.station.hub();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        // Wire form for dig3 REST calls.
        let wire_sym = self.wire_symbol(exchange_id, account_type, symbol);
        let interval = timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let active_fetches = self.active_fetches.clone();
        let oldest_fetched_ts = self.oldest_fetched_ts.clone();
        let station_bg = self.station.clone();

        let station_bar_seriskey = station_bar_key(exchange_id, account_type, &symbol_str, &tf_name);

        // Backfill dedup key: encode the "backfill" role by prefixing the symbol.
        // This avoids colliding with the regular request_bars fetch key while
        // reusing the same HashSet type.
        let backfill_fetch_key: FetchKey = (
            exchange_id,
            account_type,
            format!("backfill:{}", symbol_str),
            tf_name.clone(),
        );

        // ── Deduplication guard ───────────────────────────────────────────────
        {
            let mut af = active_fetches.lock().unwrap_or_else(|e| e.into_inner());
            if af.contains(&backfill_fetch_key) {
                crate::dlog!(
                    "[Bridge] backfill already in flight for {:?} sym={} tf={}, skipping",
                    exchange_id, symbol_str, tf_name
                );
                return;
            }
            af.insert(backfill_fetch_key.clone());
        }

        // ── Early-exit if cache is already satisfied ─────────────────────────
        let cached_len = station_read_bars_len(&station_bg, &station_bar_seriskey);

        if cached_len >= target_bars as usize {
            crate::dlog!(
                "[Bridge] backfill {:?} sym={} tf={}: cache already has {} >= {} bars, skipping",
                exchange_id, symbol_str, tf_name, cached_len, target_bars
            );
            if let Ok(mut af) = active_fetches.lock() {
                af.remove(&backfill_fetch_key);
            }
            return;
        }

        self.spawn_bg(async move {
            macro_rules! finish_backfill {
                () => {
                    if let Ok(mut af) = active_fetches.lock() {
                        af.remove(&backfill_fetch_key);
                    }
                };
            }

            // Wait for the connector to be dialed (readiness barrier — dig3
            // fetch_history resolves hub.rest itself; same pattern as
            // request_bars / request_scroll_bars).
            {
                let mut attempts = 0;
                loop {
                    if fetch_hub.rest(exchange_id).is_some() {
                        break;
                    }
                    attempts += 1;
                    if attempts > 50 {
                        crate::dlog!(
                            "[Bridge] backfill: connector {:?} not initialized after 5s",
                            exchange_id
                        );
                        let _ = tx.send(LiveUpdate::Error {
                            exchange_id,
                            account_type,
                            symbol: symbol_str.clone(),
                            timeframe: tf_name.clone(),
                            message: format!(
                                "backfill: connector {:?} not initialized",
                                exchange_id
                            ),
                        });
                        finish_backfill!();
                        return;
                    }
                    sleep_ms(100).await;
                }
            }

            // Rule: the exchange is the source of truth. Each emit from
            // background_backfill must contain a SELF-SUFFICIENT dataset that
            // tick.rs can apply via `update_bars`/`set_bars` AS-IS, including
            // the fresh tail. We must NEVER ask the consumer to merge.
            //
            // Pre-Phase 2B that worked because the consumer-side ring held the
            // fresh tail by construction. After Phase 2B the station's ring
            // is a different buffer (warm-seed may be older than Phase A's
            // fresh 300), so we cannot use station_read_bars as the "fresh
            // anchor" any more. Instead: always start backfill paging from
            // `now_ms`. The FIRST page is the latest 500 from REST = the
            // fresh tail. Subsequent pages walk backwards. The accumulated
            // result is a complete contiguous history that already includes
            // the freshest closed bars, so the consumer's `update_bars`
            // replacement is correct without any merge.
            //
            // The "still_need" / "satisfied-by-cache" short-circuit is
            // intentionally dropped here — background_backfill is an
            // explicit "fetch target_bars history NOW from the exchange"
            // op (called by tick.rs after every BarsLoaded), not a
            // lazy-fill. The previous early-exit-on-cache branch was the
            // mechanism by which station's stale warm-seed could block the
            // refresh AND poison the cursor at the same time.
            let still_need = target_bars as usize;

            crate::dlog!(
                "[Bridge] backfill {:?} sym={} tf={}: target={} bars, paginating from now",
                exchange_id, symbol_str, tf_name, still_need
            );

            // Effective limit accounts for exchange-specific per-request caps
            // (e.g. OKX returns at most 300 bars regardless of what we request).
            let effective_limit = fetch_hub.max_kline_limit(exchange_id, 500);

            // Phase 2B: pages come from dig3 `backfill::fetch_history`
            // (exclusive `end_time_ms`, raw symbol, sorted+deduped). First
            // page = (now - effective_limit*tf) .. now → fresh tail. Cursor
            // walks backwards from there.
            let kline_iv = KlineInterval::new(interval.as_str());
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(i64::MAX);
            let mut end_time_cursor: i64 = now_ms;
            let mut accumulated: Vec<Bar> = Vec::with_capacity(still_need.min(20_000));
            let mut pages: usize = 0;

            'backfill: loop {
                let result = digdigdig3_station::backfill::fetch_history(
                    &fetch_hub,
                    exchange_id,
                    &wire_sym,
                    account_type,
                    &kline_iv,
                    end_time_cursor,
                    effective_limit,
                )
                .await;

                match result {
                    Ok(points) => {
                        if points.is_empty() {
                            // Exchange has no more data — record oldest_ts so scroll
                            // can stop asking for more.
                            crate::dlog!("[Bridge] backfill: empty page at page {}, stopping", pages);
                            if let Some(oldest_so_far) = accumulated.first().map(|b| b.timestamp) {
                                if let Ok(mut map) = oldest_fetched_ts.lock() {
                                    let bf_oldest_key: FetchKey = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());
                                    let entry = map.entry(bf_oldest_key).or_insert(i64::MAX);
                                    if oldest_so_far < *entry {
                                        *entry = oldest_so_far;
                                    }
                                }
                            }
                            break 'backfill;
                        }

                        let page_len = points.len();
                        pages += 1;
                        let batch: Vec<Bar> = points.iter().map(bar_point_to_bar).collect();

                        // Move cursor to before the oldest bar in this page
                        // (`open_time` ms; cursor exclusive ms).
                        if let Some(oldest_ms) = points.iter().map(|p| p.open_time).min() {
                            end_time_cursor = oldest_ms - 1;
                        }

                        let oldest_bar_ts = batch.first().map(|b| b.timestamp).unwrap_or(0);

                        // If the exchange returned fewer bars than its effective limit, it has
                        // run out of history — record oldest_ts so scroll-left stops requesting
                        // more.  A full page (page_len == effective_limit) means there is more
                        // data available further back; recording oldest_ts here would
                        // incorrectly block future fetches.
                        if page_len < effective_limit as usize && oldest_bar_ts > 0 {
                            if let Ok(mut map) = oldest_fetched_ts.lock() {
                                let bf_oldest_key: FetchKey = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());
                                let entry = map.entry(bf_oldest_key).or_insert(i64::MAX);
                                if oldest_bar_ts < *entry {
                                    *entry = oldest_bar_ts;
                                }
                            }
                        }

                        accumulated = merge_bars(batch, accumulated);

                        crate::dlog!(
                            "[Bridge] backfill: page {} -> {} accumulated bars (oldest_ts={}, page_len={})",
                            pages,
                            accumulated.len(),
                            oldest_bar_ts,
                            page_len,
                        );

                        if accumulated.len() >= still_need {
                            crate::dlog!(
                                "[Bridge] backfill: reached target ({} >= {}), more data may exist on exchange",
                                accumulated.len(),
                                still_need
                            );
                            // Do NOT record oldest_ts here — we stopped by choice, not
                            // because the exchange ran out of data.
                            break 'backfill;
                        }
                        if pages >= 40 {
                            crate::dlog!("[Bridge] backfill: capped at 40 pages (~20k bars)");
                            // Page cap also does not mean exchange ran out.
                            break 'backfill;
                        }
                    }
                    Err(e) => {
                        crate::dlog!("[Bridge] backfill: error at page {}: {}", pages, e);
                        break 'backfill;
                    }
                }
            }

            if accumulated.is_empty() {
                crate::dlog!("[Bridge] backfill: no bars fetched, done");
                finish_backfill!();
                return;
            }

            // accumulated is already self-sufficient: page 1 was anchored at
            // now_ms so the freshest bars are included; subsequent pages
            // walked further back. tick.rs's BackfillComplete handler does
            // `window.update_bars(bars)` (full replace) and that's correct
            // by the source-of-truth rule — REST overwrites RAM as-is.
            let acc_first = accumulated.first().map(|b| b.timestamp).unwrap_or(0);
            let acc_last = accumulated.last().map(|b| b.timestamp).unwrap_or(0);

            crate::dlog!(
                "[Bridge] backfill {:?} sym={} tf={}: fetched {} bars over {} pages, emitting",
                exchange_id, symbol_str, tf_name, accumulated.len(), pages
            );
            crate::dlog!(
                "[Bridge::backfill emit BackfillComplete] {:?} {} at={:?} tf={} bars={} first_ts={} last_ts={} pages={}",
                exchange_id, symbol_str, account_type, tf_name, accumulated.len(), acc_first, acc_last, pages,
            );

            let _ = tx.send(LiveUpdate::BackfillComplete {
                exchange_id,
                account_type,
                symbol: symbol_str,
                timeframe: tf_name,
                bars: accumulated,
            });

            finish_backfill!();
        });
    }

    /// Request a single page of bars older than `before_ts` for scroll-left (infinite scroll).
    ///
    /// Non-blocking — spawns an async task. On completion, sends
    /// `ScrollBarsLoaded` through the update channel.
    ///
    /// This is Layer 3 of the data loading pipeline. It is triggered when the
    /// user scrolls left past the oldest bar currently in the viewport. The
    /// caller should fire it again when the user scrolls further left; the
    /// dedup guard ensures only one in-flight scroll fetch exists per key at
    /// any time.
    ///
    /// # Parameters
    ///
    /// - `before_ts`: oldest timestamp currently in the window's bar series
    ///   (seconds). The fetch retrieves bars strictly older than this.
    /// - `batch_size`: number of bars to request per page (typically 500).
    ///
    /// # Behaviour
    ///
    /// 1. Dedup: if a scroll fetch for this key is already in-flight, returns immediately.
    /// 2. Cache hit: if the session bar cache already contains bars older than
    ///    `before_ts`, extracts them and sends `ScrollBarsLoaded` without a
    ///    network request.
    /// 3. Oldest-ts guard: if we already know the exchange has no data before
    ///    `before_ts` (recorded by a previous scroll fetch), returns immediately.
    /// 4. REST fetch: fetches one page ending just before `before_ts`, merges
    ///    the result into the cache, and sends `ScrollBarsLoaded`.
    pub fn request_scroll_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,
        before_ts: i64,
        batch_size: usize,
    ) {
        // Phase 2B: scroll-left pagination uses the STATION hub (shared pool
        // with live WS + Phase A/B + background backfill).
        let fetch_hub = self.station.hub();
        let tx = self.tx.clone();
        let symbol_str = symbol.to_string();
        // Wire form for dig3 REST calls.
        let wire_sym = self.wire_symbol(exchange_id, account_type, symbol);
        let interval = timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let active_fetches = self.active_fetches.clone();
        let oldest_fetched_ts = self.oldest_fetched_ts.clone();
        let station_sc = self.station.clone();

        let station_bar_seriskey = station_bar_key(exchange_id, account_type, &symbol_str, &tf_name);
        let scroll_oldest_ts_key: FetchKey = (exchange_id, account_type, symbol_str.clone(), tf_name.clone());

        // Scroll dedup key: encode "scroll:" role by prefixing the symbol,
        // matching the pattern used by "backfill:" in request_background_backfill.
        let scroll_fetch_key: FetchKey = (
            exchange_id,
            account_type,
            format!("scroll:{}", symbol_str),
            tf_name.clone(),
        );

        // ── Deduplication guard ───────────────────────────────────────────────
        {
            let mut af = active_fetches.lock().unwrap_or_else(|e| e.into_inner());
            if af.contains(&scroll_fetch_key) {
                crate::dlog!(
                    "[Bridge] scroll fetch already in flight for {:?} sym={} tf={}, skipping",
                    exchange_id, symbol_str, tf_name
                );
                return;
            }
            af.insert(scroll_fetch_key.clone());
        }

        // ── Cache check: do we already have bars older than before_ts? ────────
        let all_cached_bars = station_read_bars(&station_sc, &station_bar_seriskey);
        let cached_older: Option<Vec<Bar>> = if all_cached_bars.is_empty() {
            None
        } else {
            Some(
                all_cached_bars
                    .iter()
                    .filter(|b| b.timestamp < before_ts)
                    .copied()
                    .collect::<Vec<Bar>>(),
            )
        };

        if let Some(ref older_bars) = cached_older {
            if !older_bars.is_empty() {
                let prepend_count = older_bars.len().min(batch_size);
                // Full merged set is whatever is in cache (already sorted).
                let full_bars: Vec<Bar> = all_cached_bars;

                crate::dlog!(
                    "[Bridge] scroll {:?} sym={} tf={}: {} cached bars older than {}, serving {} from cache",
                    exchange_id, symbol_str, tf_name, older_bars.len(), before_ts, prepend_count
                );

                let _ = tx.send(LiveUpdate::ScrollBarsLoaded {
                    exchange_id,
                    account_type,
                    symbol: symbol_str.clone(),
                    timeframe: tf_name.clone(),
                    bars: full_bars,
                    prepend_count,
                });

                if let Ok(mut af) = active_fetches.lock() {
                    af.remove(&scroll_fetch_key);
                }
                return;
            }
        }

        // ── Oldest-ts guard: no point fetching if exchange has no more data ───
        let known_oldest = oldest_fetched_ts
            .lock()
            .ok()
            .and_then(|m| m.get(&scroll_oldest_ts_key).copied());

        if let Some(oldest) = known_oldest {
            if before_ts <= oldest {
                // This is correct behavior: the exchange returned an empty page or error
                // at this timestamp in a previous fetch, so there are no older bars.
                // All available bars are already loaded — scroll has reached the beginning
                // of the exchange's history for this symbol/timeframe.
                crate::dlog!(
                    "[Bridge] scroll {}: all available bars already loaded (oldest_fetched={}, before_ts={})",
                    symbol_str, oldest, before_ts
                );
                if let Ok(mut af) = active_fetches.lock() {
                    af.remove(&scroll_fetch_key);
                }
                return;
            }
        }

        // ── REST fetch ────────────────────────────────────────────────────────
        self.spawn_bg(async move {
            macro_rules! finish_scroll {
                () => {
                    if let Ok(mut af) = active_fetches.lock() {
                        af.remove(&scroll_fetch_key);
                    }
                };
            }

            // Wait for connector — ensures the exchange is connected before
            // fetch_history attempts its REST call (fetch_history returns Err
            // immediately if hub has no connector for this exchange).
            {
                let mut attempts = 0;
                loop {
                    if fetch_hub.rest(exchange_id).is_some() {
                        break;
                    }
                    attempts += 1;
                    if attempts > 50 {
                        crate::dlog!(
                            "[Bridge] scroll: connector {:?} not initialized after 5s",
                            exchange_id
                        );
                        finish_scroll!();
                        return;
                    }
                    sleep_ms(100).await;
                }
            }

            // end_time = just before the oldest timestamp the caller has (ms).
            let end_time_ms = before_ts * 1000 - 1;

            crate::dlog!(
                "[Bridge] scroll fetch (station): {:?} sym={} tf={} end_time_ms={} batch={}",
                exchange_id, symbol_str, tf_name, end_time_ms, batch_size
            );

            // Clamp to the exchange's actual per-request maximum so we request
            // exactly what the exchange will return, and can correctly distinguish
            // a full page from an exhausted-history short page.
            let limit = fetch_hub.max_kline_limit(exchange_id, (batch_size as u16).min(500));
            let kline_iv = KlineInterval::new(&interval);
            let result = digdigdig3_station::fetch_history(
                &fetch_hub,
                exchange_id,
                &wire_sym,
                account_type,
                &kline_iv,
                end_time_ms,
                limit,
            ).await;

            match result {
                Err(e) => {
                    crate::dlog!(
                        "[Bridge] scroll fetch error: {:?} sym={} tf={}: {}",
                        exchange_id, symbol_str, tf_name, e
                    );
                    // Do NOT record oldest_fetched_ts — this may be a temporary error
                    // (e.g. rate limit). The scroll trigger will retry on the next frame.
                    finish_scroll!();
                }
                Ok(points) if points.is_empty() => {
                    crate::dlog!(
                        "[Bridge] scroll fetch: {:?} sym={} tf={}: empty page, no more history",
                        exchange_id, symbol_str, tf_name
                    );
                    // Record before_ts as the oldest known — exchange has nothing older.
                    if let Ok(mut map) = oldest_fetched_ts.lock() {
                        let entry = map.entry(scroll_oldest_ts_key.clone()).or_insert(i64::MAX);
                        if before_ts < *entry {
                            *entry = before_ts;
                        }
                    }
                    finish_scroll!();
                }
                Ok(points) => {
                    let page_len = points.len();
                    let new_page: Vec<Bar> = points.iter().map(bar_point_to_bar).collect();
                    let prepend_count = new_page.len();

                    crate::dlog!(
                        "[Bridge] scroll fetch: {:?} sym={} tf={}: got {} bars",
                        exchange_id, symbol_str, tf_name, prepend_count
                    );

                    // Only record oldest_fetched_ts when the exchange returned fewer bars
                    // than requested — that means it ran out of history.  A full page
                    // (page_len == limit) means there is more data available further back;
                    // recording oldest_ts here would incorrectly block future scroll requests.
                    let oldest_bar_ts = new_page.first().map(|b| b.timestamp).unwrap_or(0);
                    if oldest_bar_ts > 0 && page_len < limit as usize {
                        if let Ok(mut map) = oldest_fetched_ts.lock() {
                            let entry = map.entry(scroll_oldest_ts_key.clone()).or_insert(i64::MAX);
                            if oldest_bar_ts < *entry {
                                *entry = oldest_bar_ts;
                            }
                        }
                    }

                    // Send only the new REST page; the consumer (BackendCore::
                    // tick_writers) merges this page into the existing bubble.bars
                    // by timestamp (last-write-wins) so live bars to the right of
                    // `before_ts` are preserved. `fetch_history` bypasses Station's
                    // persisted Series, so the page won't appear in the cache path
                    // above on its own — the bubble in BackendCore is the single
                    // source of truth for the chart's current bar set.
                    let _ = tx.send(LiveUpdate::ScrollBarsLoaded {
                        exchange_id,
                        account_type,
                        symbol: symbol_str,
                        timeframe: tf_name,
                        bars: new_page,
                        prepend_count,
                    });

                    finish_scroll!();
                }
            }
        });
    }

    /// Resolve a (possibly canonical) symbol key to the venue-native wire string.
    ///
    /// Canonical keys are of the form `"BTCUSDT"` (base+quote uppercased, no
    /// separator) as produced by `build_symbol_search_results`. Venue-native
    /// strings vary by exchange: `"BTC-USDT"` on OKX, `"KRW-BTC"` on Upbit,
    /// `"ETH/USDC"` on Lighter, etc.
    ///
    /// Resolution order:
    /// 1. Exact match on `SymbolInfo.symbol` → return `requested` unchanged
    ///    (native passthrough — covers old presets/watchlists and venues where
    ///    canonical == native).
    /// 2. Match on `format!("{}{}", base_asset, quote_asset).to_uppercase()` with
    ///    `account_type` filter → return the raw `SymbolInfo.symbol`.
    /// 3. Same as (2) but ignoring `account_type` when exactly one match exists.
    /// 4. Fallback → return `requested` unchanged.
    fn wire_symbol(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        requested: &str,
    ) -> String {
        // Memoised. The miss-path clones the full Vec<SymbolInfo> from
        // station + scans it 2-3 times — 130-170ms per call on Binance's
        // ~2500-symbol catalogue.
        let cache_key = (exchange_id, account_type, requested.to_string());
        if let Ok(cache) = self.wire_symbol_cache.lock() {
            if let Some(hit) = cache.get(&cache_key) {
                return hit.clone();
            }
        }

        let symbols = self.station.symbols(exchange_id);

        // (1) Exact native match — covers old watchlist/preset entries and venues
        // where the canonical form is identical to the native form (e.g. Binance).
        if symbols.iter().any(|s| s.symbol == requested) {
            let resolved = requested.to_string();
            if let Ok(mut cache) = self.wire_symbol_cache.lock() {
                cache.insert(cache_key, resolved.clone());
            }
            return resolved;
        }

        // (2) Canonical → native with account_type filter.
        let req_upper = requested.to_uppercase();
        let with_at: Vec<&SymbolInfo> = symbols
            .iter()
            .filter(|s| {
                s.account_type == account_type
                    && !s.base_asset.is_empty()
                    && !s.quote_asset.is_empty()
                    && format!("{}{}", s.base_asset, s.quote_asset).to_uppercase() == req_upper
            })
            .collect();

        if with_at.len() == 1 {
            let resolved = with_at[0].symbol.clone();
            if let Ok(mut cache) = self.wire_symbol_cache.lock() {
                cache.insert(cache_key, resolved.clone());
            }
            return resolved;
        }

        // (3) Relaxed: ignore account_type when exactly one match.
        if with_at.is_empty() {
            let any_at: Vec<&SymbolInfo> = symbols
                .iter()
                .filter(|s| {
                    !s.base_asset.is_empty()
                        && !s.quote_asset.is_empty()
                        && format!("{}{}", s.base_asset, s.quote_asset).to_uppercase() == req_upper
                })
                .collect();

            if any_at.len() == 1 {
                let resolved = any_at[0].symbol.clone();
                if let Ok(mut cache) = self.wire_symbol_cache.lock() {
                    cache.insert(cache_key, resolved.clone());
                }
                return resolved;
            }
        }

        // (4) Fallback — return the requested string unchanged.
        // Don't cache the fallback: the symbol catalogue may still be
        // warming up (REST `exchange_info` not in yet). A cache hit here
        // would freeze the resolution to passthrough until shutdown.
        requested.to_string()
    }

}

/// Attempt a confident wire-symbol resolution for `requested` against the
/// symbol catalogue held by `station`.
///
/// Returns `Some(wire_string)` when a confident match is found (exact native
/// or unique canonical→native match).  Returns `None` when the symbol list is
/// still empty (station warm-up in progress) or when no match was found
/// (caller should fall back to the passthrough form).
///
/// Unlike `wire_symbol` this never returns the passthrough fallback — `None`
/// explicitly means "not ready or not found", letting the caller retry.
fn try_resolve_wire_symbol(
    station: &digdigdig3_station::Station,
    exchange_id: ExchangeId,
    account_type: AccountType,
    requested: &str,
) -> Option<String> {
    let symbols = station.symbols(exchange_id);
    if symbols.is_empty() {
        return None;
    }

    // Exact native match — confident.
    if symbols.iter().any(|s| s.symbol == requested) {
        return Some(requested.to_string());
    }

    // Canonical → native with account_type filter.
    let req_upper = requested.to_uppercase();
    let with_at: Vec<&SymbolInfo> = symbols
        .iter()
        .filter(|s| {
            s.account_type == account_type
                && !s.base_asset.is_empty()
                && !s.quote_asset.is_empty()
                && format!("{}{}", s.base_asset, s.quote_asset).to_uppercase() == req_upper
        })
        .collect();

    if with_at.len() == 1 {
        return Some(with_at[0].symbol.clone());
    }

    // Relaxed: ignore account_type when exactly one match.
    if with_at.is_empty() {
        let any_at: Vec<&SymbolInfo> = symbols
            .iter()
            .filter(|s| {
                !s.base_asset.is_empty()
                    && !s.quote_asset.is_empty()
                    && format!("{}{}", s.base_asset, s.quote_asset).to_uppercase() == req_upper
            })
            .collect();

        if any_at.len() == 1 {
            return Some(any_at[0].symbol.clone());
        }
    }

    // Symbol list non-empty but no confident match.
    None
}


// ─────────────────────────────────────────────────────────────────────────────
// EXCHANGE KLINE LIMIT
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// ACCOUNT TYPE HELPERS
// ─────────────────────────────────────────────────────────────────────────────

/// Convert an account type short label string back to an [`AccountType`] enum.
///
/// Unknown labels fall back to [`AccountType::Spot`] — the safe default and
/// backward-compatible case (old disk files without an account_type prefix).
///
/// Delegates to `mlc_core::instrument::AccountType` (the single canonical
/// label parser — see `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1C)
/// and converts across the mlc-core/dig3 boundary via
/// [`crate::convert::account_type_to_dig3`].
pub fn account_type_from_short_label(s: &str) -> AccountType {
    let core_at: mlc_core::instrument::AccountType = s.parse().unwrap_or_default();
    crate::convert::account_type_to_dig3(core_at)
}

// ─────────────────────────────────────────────────────────────────────────────
// INTERVAL HELPERS
// ─────────────────────────────────────────────────────────────────────────────

/// Parse an interval string to seconds.
// ─────────────────────────────────────────────────────────────────────────────
// BAR PAGE MERGE
// ─────────────────────────────────────────────────────────────────────────────

/// Merge two sorted bar vectors by timestamp, keeping the *newer* version of
/// any bar that appears in both (same timestamp). The result is sorted
/// ascending by timestamp.
///
/// Used during Phase B heal accumulation and background-backfill page
/// accumulation — pure in-task math, no shared state.
fn merge_bars(old: Vec<Bar>, fresh: Vec<Bar>) -> Vec<Bar> {
    if fresh.is_empty() {
        return old;
    }
    if old.is_empty() {
        return fresh;
    }

    // Always merge via HashMap (fresh wins on conflicts) + sort.
    // This is self-healing: even if old is unsorted, the result is always sorted.
    let mut map: HashMap<i64, Bar> = HashMap::with_capacity(old.len() + fresh.len());
    for bar in old {
        map.insert(bar.timestamp, bar);
    }
    for bar in fresh {
        map.insert(bar.timestamp, bar);
    }
    let mut merged: Vec<Bar> = map.into_values().collect();
    merged.sort_by_key(|b| b.timestamp);
    merged.retain(|b| b.timestamp >= 1_000_000_000 && b.timestamp <= 9_999_999_999);
    merged
}

// ─────────────────────────────────────────────────────────────────────────────
// STATION BAR CONVERSION
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a `digdigdig3_station::BarPoint` to a chart `Bar`.
///
/// `BarPoint.open_time` is in milliseconds; `Bar.timestamp` is in seconds.
/// `Bar.derived_ts_ms` carries the ms value verbatim — trade-agg bar
/// families (range/volume/tick/dollar/imbalance/run) can close more than
/// once inside the same wall-clock second, so the seconds-truncated
/// `timestamp` alone is not a unique bar identity (2026-07-08 caterpillar
/// bug — see the field doc on `mlc_core::Bar`).
fn bar_point_to_bar(point: &digdigdig3_station::data::BarPoint) -> Bar {
    Bar {
        timestamp: point.open_time / 1000,
        open: point.open,
        high: point.high,
        low: point.low,
        close: point.close,
        volume: point.volume,
        derived_ts_ms: point.open_time,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DERIVED-STREAM FORWARDER BODIES
// ─────────────────────────────────────────────────────────────────────────────
//
// Each `spawn_*_sub` method above wraps one of these free async functions
// with `self.spawn_bg_join(...)` for the plain first-subscribe path. Being
// free functions (not `&self` methods) lets `request_deeper_derived_seed`
// re-enter them via `spawn_bg_join_nested` from inside an already-spawned
// task, where `self` is not available (the task must be `'static` and does
// not hold a `DataBridge` reference).
//
// `first_ok_tx`, when `Some`, is fired exactly once — right after the FIRST
// subscribe cycle (`respawn_n == 0`) resolves — with the count of
// successfully-subscribed streams (`report.ok.len()`, 0 on any failure path
// that returns early). This is the only signal available to a reseed caller
// for "did the deeper subscribe actually get more room" (a short REST page
// during warm-seed is absorbed silently by the ring and not observable
// here). Plain first-subscribe callers pass `None` — zero behavior change.

/// Bridge-crate-local mirror of `mlc_backend::bubble::HistoryDepth`.
///
/// `mlc-backend-data-bridge` sits BELOW `mlc-backend` in the crate graph
/// (engine layer vs kernel — see the contract table in
/// `mylittlechart/CLAUDE.md`), so this crate cannot name
/// `mlc_backend::bubble::HistoryDepth` directly. `LiveUpdate::
/// HistoryDepthReported` carries this plain wire shape instead;
/// `mlc-backend`'s `tick_writers` match arm does the final 1:1 conversion
/// into the real `HistoryDepth` enum (see
/// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDepthWire {
    Full,
    Capped { achieved: usize },
    LiveOnly,
}

/// Reduce a station `SeedOutcome` to the bubble-facing [`HistoryDepthWire`].
/// `achieved` is carried through for `Capped` so the UI badge can show a
/// number ("240 bars") instead of just "degraded".
fn history_depth_from_seed_outcome(outcome: &digdigdig3_station::SeedOutcome) -> HistoryDepthWire {
    use digdigdig3_station::TruncationReason as TR;
    match outcome.truncated_by {
        None => HistoryDepthWire::Full,
        Some(TR::VenueRecentOnly) => HistoryDepthWire::LiveOnly,
        Some(TR::VenueWindowCap) => HistoryDepthWire::Capped { achieved: outcome.achieved },
        // Exhausted-by-genuine-history-end or a transient error still means
        // "this is what we have" — not a hard venue ceiling, so it does NOT
        // block future deepen attempts the way RecentOnly/WindowCap do, but
        // it also isn't a confirmed-Full result. Treat as Capped so the UI
        // shows a soft-degraded state rather than silently claiming Full.
        Some(TR::VenueHistoryExhausted | TR::Error) => HistoryDepthWire::Capped { achieved: outcome.achieved },
    }
}

/// Map a derived `Stream` variant to the chart_type id it backs. 1:1 with
/// the `Stream` construction inside each `request_*` method — used only to
/// stamp `LiveUpdate::HistoryDepthReported.chart_type` for the generic
/// `*_sub_body` forwarders (which have no other way to know which
/// chart_type they were spawned for). `""` for any variant that never
/// reaches these forwarders (kline/render-family and the two heatmaps have
/// their own history story, or none at all).
fn stream_to_chart_type(stream: &digdigdig3_station::Stream) -> &'static str {
    use digdigdig3_station::Stream;
    match stream {
        Stream::RangeBar(_) => "range_bar",
        Stream::VolumeBar(_) => "volume_bar",
        Stream::TickBar(_) => "tick_bar",
        Stream::DollarBar { .. } => "dollar_bar",
        Stream::TickImbalanceBar { .. } => "tick_imbalance_bar",
        Stream::VolumeImbalanceBar { .. } => "volume_imbalance_bar",
        Stream::RunBar { .. } => "run_bar",
        Stream::Footprint(_) => "footprint",
        Stream::RenkoBar(_, _) => "renko",
        Stream::PnfBar(_, _) => "pnf",
        Stream::KagiBar(_) => "kagi",
        Stream::ThreeLineBreak { .. } => "three_line_break",
        Stream::TpoProfile(_, _) => "tpo",
        _ => "",
    }
}

/// Look up `key`'s seed outcome in `report.seed_outcomes` and, if present,
/// broadcast one `LiveUpdate::HistoryDepthReported` for `chart_type`.
///
/// Shared by every derived-stream `*_sub_body` cold-subscribe path (Step 1,
/// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md`) — called only
/// at `respawn_n == 0` (the actual cold/chart-open subscribe; later respawn
/// cycles are heal-driven, not user-visible depth changes). No-op (silently)
/// when `chart_type` is empty (stream has no chart_type mapping) or the key
/// has no seed outcome (WS-only stream, no REST seed ran).
fn emit_seed_history_depth(
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: &str,
    chart_type: &'static str,
    report: &digdigdig3_station::SubscribeReport,
    key: &StationSeriesKey,
) {
    if chart_type.is_empty() {
        return;
    }
    let Some((_, outcome)) = report.seed_outcomes.iter().find(|(k, _)| k == key) else {
        return;
    };
    let depth = history_depth_from_seed_outcome(outcome);
    let _ = tx.send(LiveUpdate::HistoryDepthReported {
        exchange_id,
        account_type,
        symbol: symbol_canonical.to_string(),
        chart_type,
        depth,
    });
}

/// Map a derived `Stream` variant to its `Kind` counterpart.
///
/// `Stream::to_kind()` on the station side is `pub(crate)` (not part of
/// digdigdig3-station's public API), so this mirrors just the derived subset
/// [`DataBridge::request_deeper_derived_seed`] /
/// [`DataBridge::request_deeper_footprint_seed`] can ever be called with.
/// Non-derived variants map to `Kind::Trade` as an inert default — they are
/// never passed to `derived_stream_series_key` in practice (both callers
/// only construct derived `Stream` variants).
fn derived_stream_to_kind(stream: &digdigdig3_station::Stream) -> StationKind {
    use digdigdig3_station::Stream;
    match stream {
        Stream::RangeBar(r) => StationKind::RangeBar(*r),
        Stream::TickBar(n) => StationKind::TickBar(*n),
        Stream::VolumeBar(v) => StationKind::VolumeBar(*v),
        Stream::DollarBar { dollar_threshold } => StationKind::DollarBar { dollar_threshold: *dollar_threshold },
        Stream::TickImbalanceBar { alpha_x100, min_ticks } => StationKind::TickImbalanceBar { alpha_x100: *alpha_x100, min_ticks: *min_ticks },
        Stream::VolumeImbalanceBar { alpha_x100, min_ticks } => StationKind::VolumeImbalanceBar { alpha_x100: *alpha_x100, min_ticks: *min_ticks },
        Stream::RunBar { alpha_x100, min_ticks } => StationKind::RunBar { alpha_x100: *alpha_x100, min_ticks: *min_ticks },
        Stream::Footprint(iv) => StationKind::Footprint(iv.clone()),
        Stream::RenkoBar(b, r) => StationKind::RenkoBar(*b, *r),
        Stream::PnfBar(b, r) => StationKind::PnfBar(*b, *r),
        Stream::KagiBar(r) => StationKind::KagiBar(*r),
        Stream::CvdLine => StationKind::CvdLine,
        Stream::ThreeLineBreak { lines_back } => StationKind::ThreeLineBreak { lines_back: *lines_back },
        Stream::TpoProfile(freq, src) => StationKind::TpoProfile(*freq, *src),
        _ => StationKind::Trade,
    }
}

/// Build the station `SeriesKey` for a derived `Stream` variant, given the
/// wire-form symbol. Used by `Station::force_unsubscribe_and_await`, which
/// takes a `SeriesKey` directly (unlike `subscribe`, which takes a `Stream`
/// inside a `SubscriptionSet`).
fn derived_stream_series_key(
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_wire: &str,
    stream: &digdigdig3_station::Stream,
) -> StationSeriesKey {
    StationSeriesKey::new(exchange_id, account_type, symbol_wire, derived_stream_to_kind(stream))
}

/// Result of [`rewarm_and_emit_derived_prepend`] — tells the caller how to
/// update its own bookkeeping (warm depth / exhaustion) without re-deriving
/// the outcome from the emitted event.
enum DerivedRewarmOutcome {
    /// Non-empty batch spliced + emitted — `warm_state` should record
    /// `new_warm_n` for this key.
    Prepended,
    /// Ring alive, rewarm ran, but produced nothing older — venue history
    /// exhausted at this depth (or already fully covered).
    Exhausted,
    /// `Station::rewarm_derived` returned `Err(RewarmUnsupported)` (imbalance/
    /// run/footprint/non-derived kind) or no live ring exists for this key —
    /// caller must fall back to teardown+respawn.
    Fallback,
}

/// Shared prepend-fold driver for the 10 rewarm-capable derived kinds
/// (renko/pnf/kagi/three_line_break/range_bar/volume_bar/tick_bar/
/// dollar_bar/cvd/tpo — NOT footprint, which has its own dedicated
/// `rewarm_footprint`/`request_deeper_footprint_seed` path, and NOT
/// tick_imbalance_bar/volume_imbalance_bar/run_bar, which cannot rewarm at
/// all).
///
/// Calls `Station::rewarm_derived` IN PLACE — the live station subscription
/// keeps running, no abort, no teardown. On a non-empty result the older
/// points are converted to the matching `LiveUpdate::*HistoryPrepended`
/// batch (one event, oldest→newest) and sent on `tx`. Used by both the
/// scroll-left deepen ([`DataBridge::request_deeper_derived_seed`]) and the
/// staged cold-open background deepen (the `request_*` methods' lazy-deepen
/// task, mirroring footprint's cold-open pattern).
async fn rewarm_and_emit_derived_prepend(
    station: &digdigdig3_station::Station,
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: &str,
    symbol_wire: &str,
    stream: &digdigdig3_station::Stream,
    extra_params: DerivedSeedExtra,
    new_warm_n: usize,
) -> DerivedRewarmOutcome {
    let key = derived_stream_series_key(exchange_id, account_type, symbol_wire, stream);
    let outcome = match station.rewarm_derived(&key, symbol_wire, new_warm_n).await {
        Ok(r) => r,
        Err(e) => {
            crate::dlog!(
                "[Bridge::rewarm_and_emit_derived_prepend] {:?} {} at={:?} stream={:?} — rewarm_derived Err, fallback to teardown: {:?}",
                exchange_id, symbol_canonical, account_type, stream, e,
            );
            return DerivedRewarmOutcome::Fallback;
        }
    };
    // dig3 0.3.26: rewarm reports the seed outcome alongside the points.
    // A capability ceiling (RecentOnly venue / RestWindow wall) means deeper
    // retries can never produce more — stop the ladder immediately instead
    // of re-fetching the same shallow window on every doubling.
    let capability_exhausted = outcome.is_capability_exhausted();
    // Report the depth this rewarm actually found — a mid-session deepen
    // that discovers the venue ceiling (Full → Capped/LiveOnly) must update
    // the bubble the same way the cold-open seed does, not just flip the
    // bridge-internal exhaustion bookkeeping silently.
    let chart_type = stream_to_chart_type(stream);
    if !chart_type.is_empty() {
        let _ = tx.send(LiveUpdate::HistoryDepthReported {
            exchange_id,
            account_type,
            symbol: symbol_canonical.to_string(),
            chart_type,
            depth: history_depth_from_seed_outcome(&outcome.seed),
        });
    }
    let rewarmed = outcome.points;
    if rewarmed.is_empty() {
        return DerivedRewarmOutcome::Exhausted;
    }
    let symbol = symbol_canonical.to_string();
    let update = match rewarmed {
        digdigdig3_station::RewarmedPoints::Renko(points) => LiveUpdate::RenkoHistoryPrepended {
            exchange_id, account_type, symbol, points,
        },
        digdigdig3_station::RewarmedPoints::Pnf(points) => {
            let box_size = match extra_params {
                DerivedSeedExtra::PnfBoxSize(b) => b,
                DerivedSeedExtra::None => {
                    if let digdigdig3_station::Stream::PnfBar(box_size_units, _) = stream {
                        *box_size_units as f64 / 1e8
                    } else {
                        0.0
                    }
                }
            };
            LiveUpdate::PnfHistoryPrepended { exchange_id, account_type, symbol, points, box_size }
        }
        digdigdig3_station::RewarmedPoints::Kagi(points) => LiveUpdate::KagiHistoryPrepended {
            exchange_id, account_type, symbol, points,
        },
        digdigdig3_station::RewarmedPoints::ThreeLineBreak(points) => LiveUpdate::TlbHistoryPrepended {
            exchange_id, account_type, symbol, points,
        },
        digdigdig3_station::RewarmedPoints::Bar(points) => {
            let bars: Vec<Bar> = points.iter().map(bar_point_to_bar).collect();
            LiveUpdate::DerivedBarsPrepended { exchange_id, account_type, symbol, bars }
        }
        digdigdig3_station::RewarmedPoints::Cvd(points) => LiveUpdate::CvdHistoryPrepended {
            exchange_id, account_type, symbol, points,
        },
        digdigdig3_station::RewarmedPoints::Tpo(points) => LiveUpdate::TpoHistoryPrepended {
            exchange_id, account_type, symbol, points,
        },
    };
    let _ = tx.send(update);
    if capability_exhausted {
        // Points delivered, but the venue ceiling is reached — report
        // Exhausted so the deepen ladder marks this key done instead of
        // scheduling ever-deeper retries that can never return more.
        return DerivedRewarmOutcome::Exhausted;
    }
    DerivedRewarmOutcome::Prepended
}

/// Whether `stream` is one of the 10 kinds `Station::rewarm_derived`/
/// `rewarm_footprint` support (in-place prepend-fold). `false` for
/// `TickImbalanceBar`/`VolumeImbalanceBar`/`RunBar` (EMA state over the
/// entire history, not reconstructible from emitted output) and for any
/// non-derived `Stream` variant.
fn stream_is_rewarm_capable(stream: &digdigdig3_station::Stream) -> bool {
    use digdigdig3_station::Stream;
    matches!(
        stream,
        Stream::RenkoBar(_, _)
            | Stream::PnfBar(_, _)
            | Stream::KagiBar(_)
            | Stream::ThreeLineBreak { .. }
            | Stream::RangeBar(_)
            | Stream::VolumeBar(_)
            | Stream::TickBar(_)
            | Stream::DollarBar { .. }
            | Stream::CvdLine
            | Stream::TpoProfile(_, _)
    )
}

/// Liveness probe for `rewarm_derived`'s ring, across every concrete output
/// type the 10 rewarm-capable kinds can produce. `station.series::<BarPoint>`
/// alone (checked by the caller before this) only covers the counted-bar
/// family — this covers the rest so the scroll-left/lazy-deepen paths can
/// tell "ring exists, rewarm should work" from "no forwarder registered,
/// fall back to teardown" for renko/pnf/kagi/tlb/cvd/tpo too.
fn station_derived_ring_alive(
    station: &digdigdig3_station::Station,
    key: &StationSeriesKey,
    stream: &digdigdig3_station::Stream,
) -> bool {
    use digdigdig3_station::Stream;
    match stream {
        Stream::RenkoBar(_, _) => station.series::<digdigdig3_station::data::RenkoBrickPoint>(key).is_some(),
        Stream::PnfBar(_, _) => station.series::<digdigdig3_station::data::PnfColumnPoint>(key).is_some(),
        Stream::KagiBar(_) => station.series::<digdigdig3_station::data::KagiSegmentPoint>(key).is_some(),
        Stream::ThreeLineBreak { .. } => {
            station.series::<digdigdig3_station::data::ThreeLineBreakLinePoint>(key).is_some()
        }
        Stream::CvdLine => station.series::<digdigdig3_station::data::ScalarBarPoint>(key).is_some(),
        Stream::TpoProfile(_, _) => station.series::<digdigdig3_station::data::TpoSessionPoint>(key).is_some(),
        // RangeBar/VolumeBar/TickBar/DollarBar already checked via BarPoint
        // by the caller; any other variant is not rewarm-capable.
        _ => false,
    }
}

/// Shared teardown+respawn body for [`DataBridge::spawn_deeper_derived_seed_teardown`]
/// (direct call, 3 EMA kinds) and the rewarm-fallback branch inside
/// [`DataBridge::request_deeper_derived_seed`] (dead ring / `RewarmUnsupported`).
/// A free function (not a `&self` method) because the fallback caller runs
/// inside an already-detached `'static` task with no `DataBridge` reference
/// — same rationale as `spawn_deeper_derived_forwarder`.
///
/// Callers are responsible for aborting the previous mlc-side forwarder
/// (`derived_subs.remove(&sub_key)`) BEFORE calling this — this function only
/// does the awaited `force_unsubscribe_and_await` + respawn + bookkeeping.
async fn run_deeper_derived_seed_teardown(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_str: String,
    symbol_wire: String,
    sub_key: (ExchangeId, String, AccountType),
    stream: digdigdig3_station::Stream,
    extra_params: DerivedSeedExtra,
    new_warm_n: usize,
    warm_state: Arc<Mutex<HashMap<(ExchangeId, String, AccountType, digdigdig3_station::Stream), usize>>>,
    seed_exhausted: Arc<Mutex<HashSet<(ExchangeId, String, AccountType, digdigdig3_station::Stream)>>>,
    derived_subs: Arc<Mutex<HashMap<(ExchangeId, String, AccountType), BgJoinHandle>>>,
) {
    let key = derived_stream_series_key(exchange_id, account_type, &symbol_wire, &stream);
    if let Err(e) = station.force_unsubscribe_and_await(&key).await {
        crate::dlog!(
            "[Bridge::run_deeper_derived_seed_teardown] force_unsubscribe_and_await failed {:?} {} at={:?} stream={:?}: {:?}",
            exchange_id, symbol_str, account_type, stream, e,
        );
        // Fall through and attempt the respawn anyway — the mux may
        // simply not have existed (idempotent Ok(()) case is the
        // common outcome; an Err here is unexpected but not fatal to
        // retrying the subscribe).
    }

    if let Ok(mut ws) = warm_state.lock() {
        ws.insert(
            (exchange_id, symbol_str.clone(), account_type, stream.clone()),
            new_warm_n,
        );
    }

    let (inner_handle, ok_count) = spawn_deeper_derived_forwarder(
        station,
        tx,
        exchange_id,
        account_type,
        symbol_str.clone(),
        symbol_wire,
        stream.clone(),
        extra_params,
        new_warm_n,
    ).await;

    if let Ok(mut subs) = derived_subs.lock() {
        subs.insert(sub_key, inner_handle);
    }

    if ok_count == 0 {
        if let Ok(mut exhausted) = seed_exhausted.lock() {
            exhausted.insert((exchange_id, symbol_str, account_type, stream));
        }
    }
}

/// Dispatch table: spawn the correct `*_sub_body` for `stream`, at depth
/// `warm_n`, wired to report its first-subscribe `ok` count back to the
/// caller. Used only by [`DataBridge::request_deeper_derived_seed`] — the
/// plain `request_*` methods call their specific `spawn_*_sub` directly.
///
/// Returns `(handle, ok_count)`. `ok_count == 0` means the deeper subscribe
/// did not produce a live forwarder (venue gap / hard failure) — the caller
/// marks the key exhausted so it stops trying to grow it.
async fn spawn_deeper_derived_forwarder(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    extra_params: DerivedSeedExtra,
    warm_n: usize,
) -> (BgJoinHandle, usize) {
    let (first_ok_tx, first_ok_rx) = tokio::sync::oneshot::channel::<usize>();

    let handle = match &stream {
        digdigdig3_station::Stream::RangeBar(_)
        | digdigdig3_station::Stream::TickBar(_)
        | digdigdig3_station::Stream::VolumeBar(_)
        | digdigdig3_station::Stream::DollarBar { .. }
        | digdigdig3_station::Stream::TickImbalanceBar { .. }
        | digdigdig3_station::Stream::VolumeImbalanceBar { .. }
        | digdigdig3_station::Stream::RunBar { .. } => spawn_bg_join_nested(derived_bar_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n,
            Some(first_ok_tx),
        )),
        digdigdig3_station::Stream::Footprint(_) => {
            // Footprint needs the per-key shared Vec — the caller
            // (`request_deeper_derived_seed`) does not have access to
            // `footprint_series` (a `DataBridge` field), so footprint
            // reseed is intentionally routed through
            // `DataBridge::request_footprint` instead of this dispatch —
            // see the doc comment on `request_deeper_derived_seed`'s mlc-app
            // caller contract. Reaching this arm means the caller passed
            // `Stream::Footprint` directly, which is a caller bug; fail soft
            // by reporting zero and no live handle rather than panicking.
            let _ = first_ok_tx.send(0);
            spawn_bg_join_nested(async {})
        }
        digdigdig3_station::Stream::RenkoBar(_, _) => spawn_bg_join_nested(renko_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n,
            Some(first_ok_tx),
        )),
        digdigdig3_station::Stream::ThreeLineBreak { .. } => spawn_bg_join_nested(three_line_break_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n,
            Some(first_ok_tx),
        )),
        digdigdig3_station::Stream::PnfBar(_, _) => {
            let box_size = match extra_params {
                DerivedSeedExtra::PnfBoxSize(b) => b,
                DerivedSeedExtra::None => {
                    if let digdigdig3_station::Stream::PnfBar(box_size_units, _) = &stream {
                        *box_size_units as f64 / 1e8
                    } else {
                        0.0
                    }
                }
            };
            spawn_bg_join_nested(pnf_sub_body(
                station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, box_size, warm_n,
                Some(first_ok_tx),
            ))
        }
        digdigdig3_station::Stream::KagiBar(_) => spawn_bg_join_nested(kagi_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n,
            Some(first_ok_tx),
        )),
        digdigdig3_station::Stream::CvdLine => spawn_bg_join_nested(cvd_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n,
            Some(first_ok_tx),
        )),
        digdigdig3_station::Stream::TpoProfile(_, _) => spawn_bg_join_nested(tpo_sub_body(
            station, tx, exchange_id, account_type, symbol_canonical, symbol_wire, stream, warm_n,
            Some(first_ok_tx),
        )),
        other => {
            crate::dlog!(
                "[Bridge::spawn_deeper_derived_forwarder] unsupported stream for reseed: {:?}",
                other,
            );
            let _ = first_ok_tx.send(0);
            spawn_bg_join_nested(async {})
        }
    };

    let ok_count = first_ok_rx.await.unwrap_or(0);
    (handle, ok_count)
}

/// Body of [`DataBridge::spawn_derived_bar_sub`] — see its doc comment for
/// behavior. `warm_n` replaces the previous hardcoded `derived_warm_n()`
/// call; `first_ok_tx` reports the first cycle's `report.ok.len()` (see the
/// module doc above this section).
async fn derived_bar_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    let stream_label = format!("{:?}", stream);
    // dig3-station 0.3.7+ explicitly disables auto-heal for non-kline
    // streams (their forwarders exit cleanly on disconnect). Our derived
    // bars (RangeBar/VolumeBar/TickBar/...) ARE non-kline by station's
    // taxonomy, so the upstream forwarder ends after a few seconds and
    // `report.handle.recv()` starts returning None. Mount a respawn
    // loop here on top so the consumer sees a continuous bar stream:
    // after every clean exit we wait 1s and re-subscribe.
    //
    // The wrapping JoinHandle is the same as before — abort cancels
    // the loop entirely, which is what abort_derived_sub wants.
    let mut total_bars: u64 = 0;
    let mut respawn_n: u32 = 0;
    loop {
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds). Only matters
        // on respawn_n == 0 (the actual chart-open subscribe); later
        // respawn cycles re-request the same depth harmlessly since
        // the station has already seeded/registered the ring.
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id,
            symbol_canonical.clone(),
            account_type,
            [stream.clone()],
            warm_n,
        );
        crate::dlog!(
            "[Bridge::derived-bar-fwd subscribing] respawn={} {:?} {} wire={} stream={} warm_n={}",
            respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label, warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!(
                    "[Bridge::derived-bar-fwd subscribe FAILED] respawn={} {:?} {} wire={} stream={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label, e,
                );
                // Hard failure (likely StreamNotSupported / venue gap).
                // Don't busy-loop — give up.
                if respawn_n == 0 {
                    if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); }
                }
                return;
            }
        };
        if !report.failed.is_empty() {
            crate::dlog!(
                "[Bridge::derived-bar-fwd subscribe partial-fail] respawn={} {:?} {} wire={} stream={}: {} failed (ok={})",
                respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label,
                report.failed.len(), report.ok.len(),
            );
            if report.ok.is_empty() {
                // Nothing succeeded — give up.
                if respawn_n == 0 {
                    if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); }
                }
                return;
            }
        }
        crate::dlog!(
            "[Bridge::derived-bar-fwd subscribed OK] respawn={} {:?} {} wire={} stream={} ok_streams={}",
            respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label, report.ok.len(),
        );
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live: on the FIRST subscribe of this task (a fresh
        // chart-open / re-open — including a post-teardown respawn from
        // `run_deeper_derived_seed_teardown`, which always starts a NEW task
        // at `respawn_n == 0`), peek the station's own `Series<BarPoint>`
        // ring for this key and replay it before entering the recv loop.
        // The ring already holds whatever the warm-seed / prior-session
        // subscribers put there (register-in-series_handles fix,
        // dig3 commit 620cda0) — this makes re-open show history
        // instantly instead of waiting for the first live tick.
        // Skipped on respawn cycles (respawn_n > 0): those are internal
        // reconnects of the SAME open, not a new consumer view.
        //
        // Batched as ONE `LiveUpdate::BarsLoaded` (Task 1, prepend-fold plan
        // Phase 2b) instead of one `BarUpdate` per point — this snapshot can
        // hold up to `DERIVED_INITIAL_WARM_N` (10k) points at cold-open or
        // the full doubled depth after a teardown+respawn deepen; emitting
        // per-point flooded `tick_writers`' unbounded drain with N events
        // in a single frame (the exact stall class `FootprintHistoryPrepended`
        // was introduced to kill for the lazy-deepen path — this closes the
        // matching hole on the COLD/replay path). `timeframe` is left empty:
        // the bridge has no window/bubble access to know this bubble's own
        // TF label (range/volume/tick/dollar size, or the fixed EMA params
        // for imbalance/run) — `BackendCore::tick_writers`'s `BarsLoaded` arm
        // resolves the real per-bubble timeframe when it sees an empty
        // string. `bars-ownership` gate (FootprintOhlc) is also enforced
        // there, same as every other `BarsLoaded` writer. Live per-point
        // `BarUpdate`s continue unchanged below — this only touches the
        // snapshot replay, which is not "live" in nature.
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(series) = station.series::<BarPoint>(key) {
                    let points = series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::derived-bar-fwd ring-peek] {:?} {} stream={} n={}",
                        exchange_id, symbol_canonical, stream_label, points.len(),
                    );
                    if !points.is_empty() {
                        let bars: Vec<Bar> = points.iter().map(bar_point_to_bar).collect();
                        let _ = tx.send(LiveUpdate::BarsLoaded {
                            exchange_id,
                            account_type,
                            symbol: symbol_canonical.clone(),
                            timeframe: String::new(),
                            bars,
                        });
                    }
                }
            }
        }
        let mut bar_n_this_cycle: u64 = 0;
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::Bar { point, .. } = ev {
                let bar = bar_point_to_bar(&point);
                bar_n_this_cycle = bar_n_this_cycle.wrapping_add(1);
                total_bars = total_bars.wrapping_add(1);
                if total_bars <= 3 || total_bars % 50 == 0 {
                    crate::dlog!(
                        "[Bridge::derived-bar-fwd emit BarUpdate] total={} cycle={} respawn={} {:?} {} stream={} bar_ts={}",
                        total_bars, bar_n_this_cycle, respawn_n,
                        exchange_id, symbol_canonical, stream_label, bar.timestamp,
                    );
                }
                let _ = tx.send(LiveUpdate::BarUpdate {
                    exchange_id,
                    account_type,
                    symbol: symbol_canonical.clone(),
                    bar,
                });
            }
        }
        crate::dlog!(
            "[Bridge::derived-bar-fwd cycle ended] respawn={} {:?} {} stream={} bars_this_cycle={} total={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label, bar_n_this_cycle, total_bars,
        );
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Body of [`DataBridge::spawn_footprint_sub`] — see its doc comment.
/// `warm_n`/`first_ok_tx`: see [`derived_bar_sub_body`]'s doc comment.
async fn footprint_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    series: SharedFootprintSeries,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    let stream_label = format!("{:?}", stream);
    // Footprint is a non-kline station stream. As of dig3-station
    // 0.3.7+, station's own auto-heal is kline-only, so the
    // SubscriptionHandle ends with a clean None after ~grace seconds
    // (or after the upstream forwarder cycles). The previous
    // single-subscribe version lost the entire stream for the rest
    // of the chart's lifetime the moment that happened — visible as
    // `subscribed OK → exited events_emitted=0` in dlog. Mount a
    // respawn loop here the same way `derived_bar_sub_body` does
    // for Range/Volume/Tick so consumers see continuous footprint
    // updates. Abort cancels the loop entirely.
    let mut total_fp: u64 = 0;
    let mut respawn_n: u32 = 0;
    loop {
        crate::dlog!(
            "[Bridge::footprint-fwd subscribing] respawn={} {:?} {} wire={} stream={} warm_n={}",
            respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label, warm_n,
        );
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds). Only matters
        // on respawn_n == 0 (the actual chart-open subscribe); later
        // respawn cycles re-request the same depth harmlessly since
        // the station has already seeded/registered the ring.
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id,
            symbol_canonical.clone(),
            account_type,
            [stream.clone()],
            warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!(
                    "[Bridge::footprint-fwd subscribe FAILED] respawn={} {:?} {} wire={} stream={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label, e,
                );
                // Hard failure (e.g. StreamNotSupported / venue gap)
                // — don't busy-loop; give up.
                if respawn_n == 0 {
                    if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); }
                }
                return;
            }
        };
        if !report.failed.is_empty() {
            crate::dlog!(
                "[Bridge::footprint-fwd subscribe partial-fail] respawn={} {:?} {} wire={} stream={}: {} failed (ok={})",
                respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label,
                report.failed.len(), report.ok.len(),
            );
            if report.ok.is_empty() {
                if respawn_n == 0 {
                    if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); }
                }
                return;
            }
        }
        crate::dlog!(
            "[Bridge::footprint-fwd subscribed OK] respawn={} {:?} {} wire={} stream={} ok_streams={}",
            respawn_n, exchange_id, symbol_canonical, symbol_wire, stream_label, report.ok.len(),
        );
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live (see derived_bar_sub_body for rationale):
        // replay the station's own Series<FootprintPoint> ring on the
        // first subscribe of this task, before entering the recv loop.
        // Also upserts into the bridge's own `series` Vec (same as the
        // live path) so readers relying on that Vec see the same data.
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(station_series) = station.series::<digdigdig3_station::data::FootprintPoint>(key) {
                    let points = station_series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::footprint-fwd ring-peek] {:?} {} stream={} n={}",
                        exchange_id, symbol_canonical, stream_label, points.len(),
                    );
                    if !points.is_empty() {
                        let mut w = series.write().await;
                        for point in &points {
                            let ts = point.open_time;
                            if let Some(last) = w.last_mut() {
                                if last.open_time == ts {
                                    *last = point.clone();
                                } else {
                                    w.push(point.clone());
                                }
                            } else {
                                w.push(point.clone());
                            }
                        }
                    }
                    for point in points {
                        let _ = tx.send(LiveUpdate::FootprintUpdate {
                            exchange_id,
                            account_type,
                            symbol: symbol_canonical.clone(),
                            point,
                        });
                    }
                }
            }
        }
        let mut fp_n_this_cycle: u64 = 0;
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::Footprint { point, .. } = ev {
                fp_n_this_cycle = fp_n_this_cycle.wrapping_add(1);
                total_fp = total_fp.wrapping_add(1);
                if total_fp <= 5 || total_fp % 50 == 0 {
                    crate::dlog!(
                        "[Bridge::footprint-fwd emit FootprintUpdate] total={} cycle={} respawn={} {:?} {} open_time={} levels.len={} vol={:.4}",
                        total_fp, fp_n_this_cycle, respawn_n,
                        exchange_id, symbol_canonical, point.open_time, point.levels.len(), point.volume,
                    );
                }
                // Upsert-by-timestamp into the shared Vec BEFORE the
                // broadcast — readers triggered by the FootprintUpdate
                // event always see the bar already in series.
                {
                    let mut w = series.write().await;
                    let ts = point.open_time;
                    if let Some(last) = w.last_mut() {
                        if last.open_time == ts {
                            *last = point.clone();
                        } else {
                            w.push(point.clone());
                        }
                    } else {
                        w.push(point.clone());
                    }
                }
                let _ = tx.send(LiveUpdate::FootprintUpdate {
                    exchange_id,
                    account_type,
                    symbol: symbol_canonical.clone(),
                    point,
                });
            }
        }
        crate::dlog!(
            "[Bridge::footprint-fwd cycle ended] respawn={} {:?} {} stream={} fp_this_cycle={} total={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label, fp_n_this_cycle, total_fp,
        );
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Body of [`DataBridge::spawn_renko_sub`] — see its doc comment.
/// `warm_n`/`first_ok_tx`: see [`derived_bar_sub_body`]'s doc comment.
async fn renko_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    // Same respawn-loop rationale as footprint_sub_body: station's
    // auto-heal is kline-only (0.3.7+), so non-kline forwarders end
    // with a clean `None` on every grace cycle. Without the loop, the
    // chart silently loses derived bars after the first cycle.
    let stream_label = format!("{:?}", stream);
    let mut respawn_n: u32 = 0;
    loop {
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds).
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id, symbol_canonical.clone(), account_type, [stream.clone()], warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!("[Bridge::renko-fwd subscribe FAILED] respawn={} {:?} {} wire={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
                return;
            }
        };
        if !report.failed.is_empty() && report.ok.is_empty() {
            if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
            return;
        }
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live: replay the station's Series<RenkoBrickPoint>
        // ring on the first subscribe of this task (see
        // derived_bar_sub_body for full rationale). Batched as ONE
        // `RenkoHistoryPrepended` (Task 1) — the mlc-backend handler for
        // that variant already handles an empty target slot correctly
        // (`head.is_none_or(...)` inserts everything when the store is
        // empty), so reusing it for the cold seed is a no-op change on the
        // receive side. Live per-brick `RenkoBarUpdate` continues below.
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(station_series) = station.series::<digdigdig3_station::data::RenkoBrickPoint>(key) {
                    let points = station_series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::renko-fwd ring-peek] {:?} {} n={}",
                        exchange_id, symbol_canonical, points.len(),
                    );
                    if !points.is_empty() {
                        let _ = tx.send(LiveUpdate::RenkoHistoryPrepended {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            points,
                        });
                    }
                }
            }
        }
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::RenkoBar { point, .. } = ev {
                let _ = tx.send(LiveUpdate::RenkoBarUpdate {
                    exchange_id, account_type,
                    symbol: symbol_canonical.clone(),
                    point,
                });
            }
        }
        crate::dlog!("[Bridge::renko-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label);
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Body of [`DataBridge::spawn_three_line_break_sub`] — see its doc comment.
/// `warm_n`/`first_ok_tx`: see [`derived_bar_sub_body`]'s doc comment.
async fn three_line_break_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    let stream_label = format!("{:?}", stream);
    let mut respawn_n: u32 = 0;
    loop {
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds).
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id, symbol_canonical.clone(), account_type, [stream.clone()], warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!("[Bridge::tlb-fwd subscribe FAILED] respawn={} {:?} {} wire={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
                return;
            }
        };
        if !report.failed.is_empty() && report.ok.is_empty() {
            if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
            return;
        }
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live: replay the station's
        // Series<ThreeLineBreakLinePoint> ring on the first subscribe
        // of this task (see derived_bar_sub_body for full rationale).
        // Batched as ONE `TlbHistoryPrepended` (Task 1) — see the
        // `RenkoHistoryPrepended` seed batch above for the empty-slot
        // rationale (identical `head.is_none_or(...)` handling).
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(station_series) = station.series::<digdigdig3_station::data::ThreeLineBreakLinePoint>(key) {
                    let points = station_series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::tlb-fwd ring-peek] {:?} {} n={}",
                        exchange_id, symbol_canonical, points.len(),
                    );
                    if !points.is_empty() {
                        let _ = tx.send(LiveUpdate::TlbHistoryPrepended {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            points,
                        });
                    }
                }
            }
        }
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::ThreeLineBreakUpdate { point, .. } = ev {
                let _ = tx.send(LiveUpdate::ThreeLineBreakUpdate {
                    exchange_id, account_type,
                    symbol: symbol_canonical.clone(),
                    point,
                });
            }
        }
        crate::dlog!("[Bridge::tlb-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label);
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Body of [`DataBridge::spawn_pnf_sub`] — see its doc comment.
/// `warm_n`/`first_ok_tx`: see [`derived_bar_sub_body`]'s doc comment.
async fn pnf_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    box_size: f64,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    let stream_label = format!("{:?}", stream);
    let mut respawn_n: u32 = 0;
    loop {
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds).
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id, symbol_canonical.clone(), account_type, [stream.clone()], warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!("[Bridge::pnf-fwd subscribe FAILED] respawn={} {:?} {} wire={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
                return;
            }
        };
        if !report.failed.is_empty() && report.ok.is_empty() {
            if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
            return;
        }
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live: replay the station's Series<PnfColumnPoint>
        // ring on the first subscribe of this task (see
        // derived_bar_sub_body for full rationale). Batched as ONE
        // `PnfHistoryPrepended` (Task 1) — see the `RenkoHistoryPrepended`
        // seed batch above for the empty-slot rationale.
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(station_series) = station.series::<digdigdig3_station::data::PnfColumnPoint>(key) {
                    let points = station_series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::pnf-fwd ring-peek] {:?} {} n={}",
                        exchange_id, symbol_canonical, points.len(),
                    );
                    if !points.is_empty() {
                        let _ = tx.send(LiveUpdate::PnfHistoryPrepended {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            points,
                            box_size,
                        });
                    }
                }
            }
        }
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::PnfBar { point, .. } = ev {
                let _ = tx.send(LiveUpdate::PnfBarUpdate {
                    exchange_id, account_type,
                    symbol: symbol_canonical.clone(),
                    point,
                    box_size,
                });
            }
        }
        crate::dlog!("[Bridge::pnf-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label);
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Body of [`DataBridge::spawn_kagi_sub`] — see its doc comment.
/// `warm_n`/`first_ok_tx`: see [`derived_bar_sub_body`]'s doc comment.
async fn kagi_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    let stream_label = format!("{:?}", stream);
    let mut respawn_n: u32 = 0;
    loop {
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds).
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id, symbol_canonical.clone(), account_type, [stream.clone()], warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!("[Bridge::kagi-fwd subscribe FAILED] respawn={} {:?} {} wire={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
                return;
            }
        };
        if !report.failed.is_empty() && report.ok.is_empty() {
            if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
            return;
        }
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live: replay the station's Series<KagiSegmentPoint>
        // ring on the first subscribe of this task (see
        // derived_bar_sub_body for full rationale). Batched as ONE
        // `KagiHistoryPrepended` (Task 1) — see the `RenkoHistoryPrepended`
        // seed batch above for the empty-slot rationale.
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(station_series) = station.series::<digdigdig3_station::data::KagiSegmentPoint>(key) {
                    let points = station_series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::kagi-fwd ring-peek] {:?} {} n={}",
                        exchange_id, symbol_canonical, points.len(),
                    );
                    if !points.is_empty() {
                        let _ = tx.send(LiveUpdate::KagiHistoryPrepended {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            points,
                        });
                    }
                }
            }
        }
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::KagiBar { point, .. } = ev {
                let _ = tx.send(LiveUpdate::KagiBarUpdate {
                    exchange_id, account_type,
                    symbol: symbol_canonical.clone(),
                    point,
                });
            }
        }
        crate::dlog!("[Bridge::kagi-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label);
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Body of [`DataBridge::spawn_cvd_sub`] — see its doc comment.
/// `warm_n`/`first_ok_tx`: see [`derived_bar_sub_body`]'s doc comment.
async fn cvd_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    let stream_label = format!("{:?}", stream);
    let mut respawn_n: u32 = 0;
    loop {
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds).
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id, symbol_canonical.clone(), account_type, [stream.clone()], warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!("[Bridge::cvd-fwd subscribe FAILED] respawn={} {:?} {} wire={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
                return;
            }
        };
        if !report.failed.is_empty() && report.ok.is_empty() {
            if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
            return;
        }
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live: replay the station's Series<ScalarBarPoint>
        // ring on the first subscribe of this task (see
        // derived_bar_sub_body for full rationale). Batched as ONE
        // `CvdHistoryPrepended` (Task 1) — its mlc-backend handler pushes
        // into `bubble.cvd_line` and emits ONE `CvdSampleAppended` (same
        // downstream fan-out `CvdLineUpdate`'s live path drives: the CVD
        // indicator pane recalc), so batching the seed is behavior-identical
        // to the per-point replay it replaces, just one event instead of N.
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(station_series) = station.series::<digdigdig3_station::data::ScalarBarPoint>(key) {
                    let points = station_series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::cvd-fwd ring-peek] {:?} {} n={}",
                        exchange_id, symbol_canonical, points.len(),
                    );
                    if !points.is_empty() {
                        let _ = tx.send(LiveUpdate::CvdHistoryPrepended {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            points,
                        });
                    }
                }
            }
        }
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::CvdLine { point, .. } = ev {
                let _ = tx.send(LiveUpdate::CvdLineUpdate {
                    exchange_id, account_type,
                    symbol: symbol_canonical.clone(),
                    point,
                });
            }
        }
        crate::dlog!("[Bridge::cvd-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label);
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Body of [`DataBridge::spawn_tpo_sub`] — see its doc comment.
/// `warm_n`/`first_ok_tx`: see [`derived_bar_sub_body`]'s doc comment.
async fn tpo_sub_body(
    station: std::sync::Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_canonical: String,
    symbol_wire: String,
    stream: digdigdig3_station::Stream,
    warm_n: usize,
    mut first_ok_tx: Option<tokio::sync::oneshot::Sender<usize>>,
) {
    let stream_label = format!("{:?}", stream);
    let mut respawn_n: u32 = 0;
    loop {
        // Deep warm seed on chart-open (design doc: dig3-station-0325-
        // derived-seeding-2026-07-04.md, risk register — 50k native /
        // 10k wasm for browser REST latency/CORS bounds).
        let sub_set = digdigdig3_station::SubscriptionSet::new().add_with_warm(
            exchange_id, symbol_canonical.clone(), account_type, [stream.clone()], warm_n,
        );
        let mut report = match station.subscribe(sub_set).await {
            Ok(r) => r,
            Err(e) => {
                crate::dlog!("[Bridge::tpo-fwd subscribe FAILED] respawn={} {:?} {} wire={}: {:?}",
                    respawn_n, exchange_id, symbol_canonical, symbol_wire, e);
                if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
                return;
            }
        };
        if !report.failed.is_empty() && report.ok.is_empty() {
            if respawn_n == 0 { if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(0); } }
            return;
        }
        if respawn_n == 0 {
            if let Some(tx0) = first_ok_tx.take() { let _ = tx0.send(report.ok.len()); }
        }
        // Pull-then-live: replay the station's Series<TpoSessionPoint>
        // ring on the first subscribe of this task (see
        // derived_bar_sub_body for full rationale). Batched as ONE
        // `TpoHistoryPrepended` (Task 1) — its mlc-backend handler applies
        // the same `MAX_TPO_SESSIONS` (11) recency cap post-splice that the
        // live `TpoProfileUpdate` push enforces, so batching the seed keeps
        // the same steady-state ring size.
        if respawn_n == 0 {
            if let Some(key) = report.ok.first() {
                emit_seed_history_depth(
                    &tx, exchange_id, account_type, &symbol_canonical,
                    stream_to_chart_type(&stream), &report, key,
                );
                if let Some(station_series) = station.series::<digdigdig3_station::data::TpoSessionPoint>(key) {
                    let points = station_series.read().await.snapshot();
                    crate::dlog!(
                        "[Bridge::tpo-fwd ring-peek] {:?} {} n={}",
                        exchange_id, symbol_canonical, points.len(),
                    );
                    if !points.is_empty() {
                        let _ = tx.send(LiveUpdate::TpoHistoryPrepended {
                            exchange_id, account_type,
                            symbol: symbol_canonical.clone(),
                            points,
                        });
                    }
                }
            }
        }
        while let Some(ev) = report.handle.recv().await {
            if let digdigdig3_station::Event::TpoProfile { point, .. } = ev {
                let _ = tx.send(LiveUpdate::TpoProfileUpdate {
                    exchange_id, account_type,
                    symbol: symbol_canonical.clone(),
                    point,
                });
            }
        }
        crate::dlog!("[Bridge::tpo-fwd cycle ended] respawn={} {:?} {} stream={}, respawning in 1s",
            respawn_n, exchange_id, symbol_canonical, stream_label);
        respawn_n = respawn_n.wrapping_add(1);
        sleep_ms(1000).await;
    }
}

/// Compound key for in-flight fetch dedup + oldest-ts tracking.
///
/// Replaced the legacy `BarSeriesKey` after the bars-on-station migration
/// (Phase 2B). Role prefix (`"backfill:"` / `"scroll:"`) is encoded into the
/// symbol field, matching the prior convention.
type FetchKey = (ExchangeId, AccountType, String, String);

/// Build a station `SeriesKey` for the `Kind::Kline(interval)` series
/// addressed by (exchange, account, raw_symbol, timeframe_name).
///
/// The kline-interval label comes from [`crate::convert::interval_label`]
/// (month stays "1M" — the one case-sensitive interval; a blanket
/// lowercase here silently rekeyed the month series as 1-minute).
fn station_bar_key(exchange: ExchangeId, account: AccountType, symbol: &str, timeframe: &str) -> StationSeriesKey {
    let iv = KlineInterval::new(&crate::convert::interval_label(timeframe));
    StationSeriesKey::new(exchange, account, symbol, StationKind::Kline(iv))
}

/// Read all bars from the station's `Series<BarPoint>` ring.
///
/// Works from BOTH the app thread (sync bridge API surface) and from inside
/// a tokio task — uses `try_read` which never blocks. If the writer holds
/// the lock right now, returns an empty Vec; the caller is expected to
/// treat empty as "not ready yet" and retry. The window is microseconds —
/// any miss is acceptable.
///
/// Previously this used `blocking_read()`, which panicked with
/// "Cannot block the current thread from within a runtime" whenever an
/// agent-server HTTP handler (running on tokio) called `get_cached_bars`.
fn station_read_bars(station: &digdigdig3_station::Station, key: &StationSeriesKey) -> Vec<Bar> {
    let Some(arc) = station.series::<BarPoint>(key) else { return Vec::new(); };
    let result = match arc.try_read() {
        Ok(guard) => guard.snapshot().iter().map(bar_point_to_bar).collect(),
        Err(_) => Vec::new(),
    };
    drop(arc);
    result
}

/// Count of bars in the station's `Series<BarPoint>` ring. Non-blocking;
/// returns 0 when the writer holds the lock (treat as "unknown / retry").
fn station_read_bars_len(station: &digdigdig3_station::Station, key: &StationSeriesKey) -> usize {
    let Some(arc) = station.series::<BarPoint>(key) else { return 0; };
    let result = match arc.try_read() {
        Ok(guard) => guard.len(),
        Err(_) => 0,
    };
    drop(arc);
    result
}


// ── Trading / Account / Position bridge methods ─────────────────────────────

impl DataBridge {
    // ── Trading ──────────────────────────────────────────────────────────────

    /// Place an order on the given exchange.
    pub async fn place_order(
        &self,
        exchange_id: ExchangeId,
        req: OrderRequest,
    ) -> digdigdig3::ExchangeResult<PlaceOrderResponse> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.place_order(req).await
    }

    /// Cancel an order on the given exchange.
    pub async fn cancel_order(
        &self,
        exchange_id: ExchangeId,
        req: CancelRequest,
    ) -> digdigdig3::ExchangeResult<Order> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.cancel_order(req).await
    }

    /// Fetch a single order by symbol and order ID.
    pub async fn get_order(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        order_id: &str,
        account_type: AccountType,
    ) -> digdigdig3::ExchangeResult<Order> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        let wire = self.wire_symbol(exchange_id, account_type, symbol);
        connector.get_order(&wire, order_id, account_type).await
    }

    /// Fetch all open orders, optionally filtered by symbol.
    pub async fn get_open_orders(
        &self,
        exchange_id: ExchangeId,
        symbol: Option<&str>,
        account_type: AccountType,
    ) -> digdigdig3::ExchangeResult<Vec<Order>> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        let wire_opt: Option<String> = symbol.map(|s| self.wire_symbol(exchange_id, account_type, s));
        connector.get_open_orders(wire_opt.as_deref(), account_type).await
    }

    /// Fetch historical orders matching the given filter.
    pub async fn get_order_history(
        &self,
        exchange_id: ExchangeId,
        filter: OrderHistoryFilter,
        account_type: AccountType,
    ) -> digdigdig3::ExchangeResult<Vec<Order>> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.get_order_history(filter, account_type).await
    }

    // ── Account ──────────────────────────────────────────────────────────────

    /// Fetch account balances.
    pub async fn get_balance(
        &self,
        exchange_id: ExchangeId,
        query: BalanceQuery,
    ) -> digdigdig3::ExchangeResult<Vec<Balance>> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.get_balance(query).await
    }

    /// Fetch account metadata (fees, permissions, …).
    pub async fn get_account_info(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
    ) -> digdigdig3::ExchangeResult<AccountInfo> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.get_account_info(account_type).await
    }

    // ── Positions ────────────────────────────────────────────────────────────

    /// Fetch open positions matching the given query.
    pub async fn get_positions(
        &self,
        exchange_id: ExchangeId,
        query: PositionQuery,
    ) -> digdigdig3::ExchangeResult<Vec<Position>> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.get_positions(query).await
    }

    /// Modify an open position (e.g. change TP/SL).
    pub async fn modify_position(
        &self,
        exchange_id: ExchangeId,
        req: PositionModification,
    ) -> digdigdig3::ExchangeResult<()> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.modify_position(req).await
    }

    // ── Optional operations ──────────────────────────────────────────────────

    /// Cancel all orders matching the given scope on the given exchange.
    pub async fn cancel_all_orders(
        &self,
        exchange_id: ExchangeId,
        scope: CancelScope,
        account_type: AccountType,
    ) -> digdigdig3::ExchangeResult<CancelAllResponse> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.cancel_all_orders(scope, account_type).await
    }

    /// Amend an existing order (price, quantity, or other fields).
    pub async fn amend_order(
        &self,
        exchange_id: ExchangeId,
        req: AmendRequest,
    ) -> digdigdig3::ExchangeResult<Order> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.amend_order(req).await
    }

    /// Place multiple orders in a single batch request.
    pub async fn place_orders_batch(
        &self,
        exchange_id: ExchangeId,
        orders: Vec<OrderRequest>,
    ) -> digdigdig3::ExchangeResult<Vec<OrderResult>> {
        let connector = self.hub.rest(exchange_id).ok_or_else(|| {
            ExchangeError::NotFound(format!("connector not ready: {:?}", exchange_id))
        })?;
        connector.place_orders_batch(orders).await
    }

    // ── Capability queries (synchronous) ─────────────────────────────────────

    /// Returns trading capabilities for the given exchange and account type,
    /// or `None` if the connector is not yet ready.
    pub fn trading_capabilities(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
    ) -> Option<TradingCapabilities> {
        self.hub.rest(exchange_id).map(|c| c.trading_capabilities(account_type))
    }

    /// Returns account capabilities for the given exchange and account type,
    /// or `None` if the connector is not yet ready.
    pub fn account_capabilities(
        &self,
        exchange_id: ExchangeId,
        account_type: AccountType,
    ) -> Option<AccountCapabilities> {
        self.hub.rest(exchange_id).map(|c| c.account_capabilities(account_type))
    }

    /// Returns the list of account types supported by the connector,
    /// or `None` if the connector is not yet ready.
    pub fn supported_account_types(&self, exchange_id: ExchangeId) -> Option<Vec<AccountType>> {
        self.hub.rest(exchange_id).map(|c| c.supported_account_types())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ORDERBOOK HELPERS
// ─────────────────────────────────────────────────────────────────────────────

/// Strip zero-padded tail from an orderbook side returned by station.
///
/// Per station 0.3.9 convention:
/// - `(0.0, 0.0)` = padding sentinel (skip).
/// - `(price, 0.0)` where `price != 0.0` = explicit removal (keep — consumer needs it).
/// - `(price, qty)` where both nonzero = add/update (keep).
fn strip_ob_padding(arr: &[(f64, f64)]) -> Vec<(f64, f64)> {
    arr.iter()
        .filter(|(p, q)| !(*p == 0.0 && *q == 0.0))
        .copied()
        .collect()
}

/// Convert a station `ObSnapshotPoint` to the dig3-core `OrderBook` type expected by
/// `OrderBookTracker::apply_snapshot`.
fn ob_snapshot_point_to_orderbook(
    p: &digdigdig3_station::data::ObSnapshotPoint,
) -> digdigdig3::core::types::OrderBook {
    use digdigdig3::core::types::OrderBookLevel;
    digdigdig3::core::types::OrderBook {
        bids: p.bids.iter().map(|(price, size)| OrderBookLevel::new(*price, *size)).collect(),
        asks: p.asks.iter().map(|(price, size)| OrderBookLevel::new(*price, *size)).collect(),
        timestamp: p.ts_ms,
        last_update_id: None,
        sequence: None,
        first_update_id: None,
        prev_update_id: None,
        event_time: None,
        transaction_time: None,
        checksum: None,
        cts: None,
        prev_change_id: None,
    }
}

/// Convert a station `ObDeltaPoint` to the dig3-core `OrderbookDelta` type expected by
/// `OrderBookTracker::apply_delta`.
fn ob_delta_point_to_orderbook_delta(
    p: &digdigdig3_station::data::ObDeltaPoint,
) -> digdigdig3::core::types::OrderbookDelta {
    use digdigdig3::core::types::OrderBookLevel;
    digdigdig3::core::types::OrderbookDelta {
        bids: p.bid_changes.iter().map(|(price, size)| OrderBookLevel::new(*price, *size)).collect(),
        asks: p.ask_changes.iter().map(|(price, size)| OrderBookLevel::new(*price, *size)).collect(),
        timestamp: p.ts_ms,
        prev_update_id: None,
        last_update_id: None,
        first_update_id: None,
        event_time: None,
        checksum: None,
    }
}

