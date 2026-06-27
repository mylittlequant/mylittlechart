//! `BackendCore` — shared data plane for the chart system.
//!
//! Lives one-per-process. Multiple `ChartApp` instances (i.e. registered
//! AppSession-like consumers) hold `Arc<BackendCore>` and **share** all the
//! data + compute fields below. Connectors don't duplicate, bars don't
//! duplicate, indicator catalogs don't duplicate.
//!
//! This module is staged in `mlc-app` for the cutover so the surgery can
//! ship as one PR without needing a new crate first. Once every shared
//! field is migrated and call sites are updated, the struct moves wholesale
//! into a `mlc-backend-core` crate above `mlc-indicators`.
//!
//! ## Migration plan
//!
//! Fields migrate one session at a time, each with a fully passing build:
//!
//! - **Session 1 (this commit)** — `bridge`, `mini_ticker`, `exchange_symbols`,
//!   `trading`. All four are either already `Arc` or `DashMap` (lock-free) or
//!   `Option<Arc<…>>`. No lock guard discipline required.
//! - **Session 2** — `managers: RwLock<Managers>` (contains `BubbleStore`).
//!   This is the real surgery: 654 call sites, requires lock-guard scoping in
//!   `prepare.rs` (long read frame) and `tick.rs` (17 short write arms).
//! - **Session 3** — `indicators: RwLock<IndicatorManager>`. 169 call sites.
//!   Smaller scope, similar lock discipline.
//! - **Final** — lift module into `mlc-backend-core` crate.

use std::sync::Arc;
use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Serialize, Serializer};

use digdigdig3::ExchangeId;
use mlc_alerts::AlertManager;
use mlc_core::preset::preset::ChartPreset;
use mlc_core::templates::manager::TemplateManager;
use mlc_core::tag_manager::{BubbleId, BubbleStore};
use mlc_core::Bar;
use mlc_core::drawing::DrawingManager;
use mlc_core::state::chart_container_leaf::ChartPanelId;
use mlc_indicators::IndicatorManager;
use mlc_live_data::{DataBridge, LiveUpdate, LiveUpdateReceiver, OrderbookSource, SymbolInfo};
use mlc_sidebar_content::watchlist::WatchlistManager;
use mlc_trading_manager::TradingManager;

// =============================================================================
// DrawingRegistry
// =============================================================================

/// Registry of all `DrawingManager` instances — one per chart canvas leaf
/// (`ChartPanelId`).
///
/// Ownership was previously on `ChartPanelState` (in `TradingPanelsStore`
/// inside mlc-app).  Moving it here mirrors the same pattern used for
/// `IndicatorManager`: the data plane lives in `BackendCore`; renderers and
/// input handlers resolve the guard and pass a borrow down.
///
/// Key = `ChartPanelId` (the per-canvas-leaf id already used as the
/// `TradingPanelsStore.chart_panel` key).  One entry per `ChartContainer`.
/// Sub-pane indicator pane addressing uses `pane_id` fields on individual
/// `PrimitiveData` values *inside* the single per-window `DrawingManager` —
/// no separate per-sub-pane registry entries are needed.
pub struct DrawingRegistry {
    by_panel: HashMap<ChartPanelId, DrawingManager>,
}

impl Default for DrawingRegistry {
    fn default() -> Self {
        Self { by_panel: HashMap::new() }
    }
}

impl DrawingRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Immutable borrow of the manager for `panel_id`.
    /// Returns `None` when the panel has not been registered yet.
    pub fn get(&self, panel_id: ChartPanelId) -> Option<&DrawingManager> {
        self.by_panel.get(&panel_id)
    }

    /// Mutable borrow of the manager for `panel_id`.
    /// Returns `None` when the panel has not been registered yet.
    pub fn get_mut(&mut self, panel_id: ChartPanelId) -> Option<&mut DrawingManager> {
        self.by_panel.get_mut(&panel_id)
    }

    /// Mutable borrow, inserting a default `DrawingManager` on miss.
    ///
    /// Mirrors the `TradingPanelsStore::chart_panel.entry(…).or_insert_with`
    /// pattern so callers do not need to pre-seed the registry.
    pub fn entry_or_default(&mut self, panel_id: ChartPanelId) -> &mut DrawingManager {
        self.by_panel.entry(panel_id).or_insert_with(DrawingManager::new)
    }

    /// Insert (or replace) a `DrawingManager` for `panel_id`.
    pub fn insert(&mut self, panel_id: ChartPanelId, dm: DrawingManager) {
        self.by_panel.insert(panel_id, dm);
    }

    /// Remove the `DrawingManager` for `panel_id`. Returns the removed
    /// instance, or `None` when it was not present.
    pub fn remove(&mut self, panel_id: ChartPanelId) -> Option<DrawingManager> {
        self.by_panel.remove(&panel_id)
    }

    /// Remove all `DrawingManager` entries whose `ChartPanelId` satisfies
    /// `predicate`.  Used when a whole chart window is closed to purge every
    /// panel that belonged to it.
    pub fn drain_matching<F>(&mut self, predicate: F)
    where
        F: Fn(ChartPanelId) -> bool,
    {
        self.by_panel.retain(|&pid, _| !predicate(pid));
    }

    /// Iterate over all `(ChartPanelId, &DrawingManager)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (ChartPanelId, &DrawingManager)> {
        self.by_panel.iter().map(|(&k, v)| (k, v))
    }

    /// Iterate over all `(ChartPanelId, &mut DrawingManager)` pairs.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (ChartPanelId, &mut DrawingManager)> {
        self.by_panel.iter_mut().map(|(&k, v)| (k, v))
    }
}

/// Cached 24-hour ticker statistics from the WebSocket mini-ticker stream.
///
/// One entry per `(symbol, exchange, account_type)`, updated every second from
/// `LiveUpdate::MiniTickerUpdate`. Used as a fallback price source in the
/// watchlist sidebar for symbols that do not have a chart window open.
#[derive(Serialize)]
pub struct MiniTickerData {
    pub last_price: f64,
    pub price_change_percent: f64,
    pub high_price: f64,
    pub low_price: f64,
    pub volume: f64,
}

/// Events emitted by [`BackendCore::tick_writers`] for each logical update
/// received from the live-data broadcast channel.
///
/// **Backend-write events** (BarsLoaded through LiquidationBucketPushed) signal
/// that a write into the shared bubble store / caches has already been
/// completed inside `tick_writers`. Per-app reactors use the event payload to
/// run viewport snap, `pending_symbol_load` clear, indicator recalc, etc.
///
/// **Pass-through events** (arms whose write target depends on per-app window
/// flags such as `oi_show`, `funding_show`, `chart_type`) carry the raw data
/// and are handled entirely by the per-app reactor.
///
/// Mode-2 daemon calls [`BackendCore::tick_bridge_only`] which drops all events.
#[derive(Debug, Clone)]
pub enum BubbleEvent {
    /// Historical bars written into one bubble (BarsLoaded arm).
    ///
    /// `prev_bars` is the snapshot of `bubble.bars` taken in `tick_writers`
    /// *before* the new `bars` payload replaced it. The reactor needs this to
    /// compute a view-anchor (timestamp of the bar that was previously at
    /// `view_start`) — re-reading the bubble after the replacement gives back
    /// the new bars, which would collapse the anchor onto itself and leave the
    /// viewport pointing at older candles every time bars grow underneath it.
    BarsLoaded {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        bars: Vec<Bar>,
        prev_bars: Vec<Bar>,
    },
    /// Background backfill bars written into one bubble.
    BackfillCompleted {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        bars: Vec<Bar>,
        prev_bars: Vec<Bar>,
    },
    /// Scroll-fetch bars (left-extend) written into one bubble.
    ScrollBarsAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        bars: Vec<Bar>,
        prepend_count: usize,
    },
    /// BarUpdate pass-through — the per-app reactor owns this write because
    /// the target bubbles depend on per-window `is_derived` / `chart_type` flags
    /// that are not visible from BackendCore.
    BarUpdatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        bar: mlc_core::Bar,
    },
    /// TradeUpdate pass-through — the per-app reactor owns the OHLCV aggregation
    /// because the aggregation period and derived-window exclusion depend on
    /// per-window TF metadata and `is_derived` flags not available in BackendCore.
    TradeUpdatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        price: f64,
        quantity: f64,
        timestamp: i64,
        is_buyer_maker: bool,
    },
    /// Mini-ticker cache entry written/updated.
    MiniTickerUpdated {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        last_price: f64,
    },
    /// Connector became ready (connection established).
    ConnectorReady {
        exchange_id: ExchangeId,
    },
    /// Exchange symbol list written into the shared cache.
    SymbolsLoaded {
        exchange_id: ExchangeId,
        count: usize,
    },
    /// A data-fetch error was reported for an instrument.
    ErrorReported {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        message: String,
    },
    /// REST orderbook snapshot arrived (DOM needs full rebuild).
    OrderbookSnapshotRest {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// WS orderbook delta arrived (L2 tape + DOM incremental update).
    OrderbookDelta {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
        ts_ms: i64,
    },
    /// Multi-DOM fan-out: delta was routed into a multi-dom panel state.
    /// Carried per-event so the reactor doesn't need to re-route.
    OrderbookDeltaForMultiDom {
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },
    /// Private stream updates (order/balance/position) — passed through for
    /// the per-app trading manager tick.
    TradingUpdate { update: LiveUpdate },
    /// Footprint bar upserted into one bubble.
    FootprintBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Renko brick appended to one bubble.
    RenkoBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// PnF column upserted in one bubble.
    PnfBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Kagi segment appended to one bubble.
    KagiBarAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// CVD scalar sample appended to one bubble.
    CvdSampleAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// TPO session upserted in one bubble.
    TpoSessionUpdated {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Three-line-break line appended to one bubble.
    TlbLineAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
    },
    /// Open interest update — write target depends on per-app `oi_show` flag.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    OiUpdatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        point: mlc_core::types::OiDeltaBar,
    },
    /// Funding rate update — write target depends on per-app `funding_show` flag.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    FundingRatePassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        point: mlc_core::types::FundingRateCell,
    },
    /// DOM heatmap snapshot — write target depends on per-app `chart_type=="dom_heatmap"`.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    DomHeatmapSnapshotPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },
    /// DOM trade dot — write target depends on per-app `chart_type=="dom_heatmap"`.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    DomTradeDotPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        dot: mlc_core::chart::dom_heatmap::DomTradeDot,
    },
    /// Liquidation per-event — write target depends on `chart_type=="liquidation_heatmap"`.
    /// Pass-through.
    LiquidationEventPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        price: f64,
        value_usd: f64,
        side: u8,
    },
    /// Liquidation bucket update — write target depends on `chart_type=="liquidation_heatmap"`.
    /// Pass-through.
    LiquidationBucketPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        long_liq_usd: f64,
        short_liq_usd: f64,
    },
}

// ---------------------------------------------------------------------------
// Custom Serialize for BubbleEvent
//
// Manual impl because:
//  - `ExchangeId` does not derive Serialize (use `.as_str()`)
//  - `LiveUpdate` does not derive Serialize (TradingUpdate emits minimal placeholder)
//  - `DomTradeDot` does not derive Serialize (inline fields manually)
// ---------------------------------------------------------------------------
impl Serialize for BubbleEvent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        macro_rules! ex {
            ($e:expr) => { $e.as_str() };
        }
        match self {
            Self::BarsLoaded { bid, exchange_id, symbol, account_type, timeframe, bars, prev_bars: _ } => {
                let mut m = s.serialize_map(Some(7))?;
                m.serialize_entry("type", "BarsLoaded")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("timeframe", timeframe)?;
                m.serialize_entry("bars_count", &bars.len())?;
                m.end()
            }
            Self::BackfillCompleted { bid, exchange_id, symbol, account_type, timeframe, bars, prev_bars: _ } => {
                let mut m = s.serialize_map(Some(7))?;
                m.serialize_entry("type", "BackfillCompleted")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("timeframe", timeframe)?;
                m.serialize_entry("bars_count", &bars.len())?;
                m.end()
            }
            Self::ScrollBarsAppended { bid, exchange_id, symbol, account_type, timeframe, bars, prepend_count } => {
                let mut m = s.serialize_map(Some(8))?;
                m.serialize_entry("type", "ScrollBarsAppended")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("timeframe", timeframe)?;
                m.serialize_entry("bars_count", &bars.len())?;
                m.serialize_entry("prepend_count", prepend_count)?;
                m.end()
            }
            Self::BarUpdatePassthrough { exchange_id, account_type, symbol, bar } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "BarUpdatePassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("bar", bar)?;
                m.end()
            }
            Self::TradeUpdatePassthrough { exchange_id, account_type, symbol, price, quantity, timestamp, is_buyer_maker } => {
                let mut m = s.serialize_map(Some(8))?;
                m.serialize_entry("type", "TradeUpdatePassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("price", price)?;
                m.serialize_entry("quantity", quantity)?;
                m.serialize_entry("timestamp", timestamp)?;
                m.serialize_entry("is_buyer_maker", is_buyer_maker)?;
                m.end()
            }
            Self::MiniTickerUpdated { exchange_id, symbol, account_type, last_price } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "MiniTickerUpdated")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("last_price", last_price)?;
                m.end()
            }
            Self::ConnectorReady { exchange_id } => {
                let mut m = s.serialize_map(Some(2))?;
                m.serialize_entry("type", "ConnectorReady")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.end()
            }
            Self::SymbolsLoaded { exchange_id, count } => {
                let mut m = s.serialize_map(Some(3))?;
                m.serialize_entry("type", "SymbolsLoaded")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("count", count)?;
                m.end()
            }
            Self::ErrorReported { exchange_id, symbol, account_type, timeframe, message } => {
                let mut m = s.serialize_map(Some(6))?;
                m.serialize_entry("type", "ErrorReported")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("timeframe", timeframe)?;
                m.serialize_entry("message", message)?;
                m.end()
            }
            Self::OrderbookSnapshotRest { exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(4))?;
                m.serialize_entry("type", "OrderbookSnapshotRest")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::OrderbookDelta { exchange_id, symbol, account_type, bids, asks, ts_ms } => {
                let mut m = s.serialize_map(Some(7))?;
                m.serialize_entry("type", "OrderbookDelta")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("bids", bids)?;
                m.serialize_entry("asks", asks)?;
                m.serialize_entry("ts_ms", ts_ms)?;
                m.end()
            }
            Self::OrderbookDeltaForMultiDom { exchange_id, symbol, account_type, bids, asks } => {
                let mut m = s.serialize_map(Some(6))?;
                m.serialize_entry("type", "OrderbookDeltaForMultiDom")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("bids", bids)?;
                m.serialize_entry("asks", asks)?;
                m.end()
            }
            Self::TradingUpdate { .. } => {
                // LiveUpdate is not Serialize; emit minimal placeholder.
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("type", "TradingUpdate")?;
                m.end()
            }
            Self::FootprintBarAppended { bid, exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "FootprintBarAppended")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::RenkoBarAppended { bid, exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "RenkoBarAppended")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::PnfBarAppended { bid, exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "PnfBarAppended")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::KagiBarAppended { bid, exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "KagiBarAppended")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::CvdSampleAppended { bid, exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "CvdSampleAppended")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::TpoSessionUpdated { bid, exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "TpoSessionUpdated")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::TlbLineAppended { bid, exchange_id, symbol, account_type } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "TlbLineAppended")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.end()
            }
            Self::OiUpdatePassthrough { exchange_id, account_type, symbol, point } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "OiUpdatePassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("point", point)?;
                m.end()
            }
            Self::FundingRatePassthrough { exchange_id, account_type, symbol, point } => {
                let mut m = s.serialize_map(Some(5))?;
                m.serialize_entry("type", "FundingRatePassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("point", point)?;
                m.end()
            }
            Self::DomHeatmapSnapshotPassthrough { exchange_id, account_type, symbol, ts_ms, bids, asks } => {
                let mut m = s.serialize_map(Some(7))?;
                m.serialize_entry("type", "DomHeatmapSnapshotPassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("ts_ms", ts_ms)?;
                m.serialize_entry("bids", bids)?;
                m.serialize_entry("asks", asks)?;
                m.end()
            }
            Self::DomTradeDotPassthrough { exchange_id, account_type, symbol, dot } => {
                let mut m = s.serialize_map(Some(8))?;
                m.serialize_entry("type", "DomTradeDotPassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                // DomTradeDot does not impl Serialize — inline fields.
                m.serialize_entry("ts_ms", &dot.ts_ms)?;
                m.serialize_entry("price", &dot.price)?;
                m.serialize_entry("quantity", &dot.quantity)?;
                m.serialize_entry("side", &dot.side)?;
                m.end()
            }
            Self::LiquidationEventPassthrough { exchange_id, account_type, symbol, ts_ms, price, value_usd, side } => {
                let mut m = s.serialize_map(Some(8))?;
                m.serialize_entry("type", "LiquidationEventPassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("ts_ms", ts_ms)?;
                m.serialize_entry("price", price)?;
                m.serialize_entry("value_usd", value_usd)?;
                m.serialize_entry("side", side)?;
                m.end()
            }
            Self::LiquidationBucketPassthrough { exchange_id, account_type, symbol, ts_ms, long_liq_usd, short_liq_usd } => {
                let mut m = s.serialize_map(Some(7))?;
                m.serialize_entry("type", "LiquidationBucketPassthrough")?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("ts_ms", ts_ms)?;
                m.serialize_entry("long_liq_usd", long_liq_usd)?;
                m.serialize_entry("short_liq_usd", short_liq_usd)?;
                m.end()
            }
        }
    }
}

/// Shared data plane owned by every chart-app process. Mode-1 hosts (vello,
/// future tessera-shell, wasm) own one `Arc<BackendCore>` and pass `Arc::clone`
/// to every `ChartApp` they spawn. Mode-2 daemon hosts own one `Arc<BackendCore>`
/// and serve it through the HTTP/WS API.
pub struct BackendCore {
    /// Bubble registry + sync-group routing.  Holds bars, derived series,
    /// primitives, indicator_configs, connection status, etc.
    ///
    /// In single-threaded embedded mode (one main loop = one writer + one
    /// renderer running sequentially) lock contention is zero — guards are
    /// taken and dropped each tick frame, no waits.
    pub bubble: std::sync::RwLock<BubbleStore>,

    /// Global alert service.  Owns alert items, the ID counter, and
    /// per-symbol crossing state.  Separate lock from `bubble` so alert reads
    /// do not block bubble writes and vice versa.
    pub alert: std::sync::RwLock<AlertManager>,

    /// Indicator manager — owns all indicator instances + recalc state +
    /// per-window assignment + global definitions catalog. Cannot live inside
    /// `Managers` (mlc-core) because mlc-indicators is above mlc-core in the
    /// dep graph; that's the architectural reason BackendCore exists.
    pub indicators: std::sync::RwLock<IndicatorManager>,

    /// Live data bridge — owns the tokio runtime, station, connector pool,
    /// broadcast channel. Already internally synchronised; `Arc::clone` is
    /// the only thing consumers need.
    pub bridge: Arc<DataBridge>,

    /// Backend's own broadcast receiver. Drained by `tick_writers` each tick
    /// to write incoming data into the shared bubble store / caches and emit
    /// `BubbleEvent`s for per-app reactors.
    ///
    /// `Mutex` because `try_recv` requires `&mut Receiver` and `tick_writers`
    /// takes `&self` (so multiple callers can coexist without `&mut BackendCore`).
    broadcast_rx: std::sync::Mutex<LiveUpdateReceiver>,

    /// Lock-free per-symbol mini-ticker cache. Read every frame by the
    /// watchlist + by the `LiquidationBucketUpdate` writer arm for mark price.
    /// Writes only from the `MiniTickerUpdate` arm. Using `DashMap` so all
    /// access is lock-free at the per-key level.
    pub mini_ticker: DashMap<String, MiniTickerData>,

    /// Symbol catalogue per exchange — populated asynchronously from
    /// `LiveUpdate::SymbolsLoaded`. Same for every consumer of this backend.
    /// `RwLock` because writes are rare (once per exchange) and reads are
    /// frequent (symbol search overlay, watchlist).
    pub exchange_symbols: std::sync::RwLock<HashMap<ExchangeId, Vec<SymbolInfo>>>,

    /// Order routing + paper engine. `None` when trading is disabled.
    /// Wrapped in `Mutex` because `TradingManager` has `&mut self` methods
    /// (`on_connector_ready`, `tick`, `refresh_balances`, etc) and is mutated
    /// from the tick loop. Single-threaded contention in embedded mode = 0.
    pub trading: Option<std::sync::Mutex<TradingManager>>,

    /// Chart presets — in-memory store keyed by preset id.
    ///
    /// Previously lived on `ChartPanelApp.presets`. Moved here so that the
    /// persist layer, input handlers, and render path all share one authoritative
    /// copy without per-window duplication (single-`BackendCore` hosts).
    pub presets: std::sync::RwLock<HashMap<String, ChartPreset>>,

    /// In-memory template manager — drawing primitive / indicator / compare /
    /// chart templates and indicator sets.
    ///
    /// Previously on `ChartPanelApp.template_manager`. Moved here for the same
    /// reason as `presets`.
    pub template_manager: std::sync::RwLock<TemplateManager>,

    /// Watchlist manager — all named watchlists with their symbol rows.
    ///
    /// Previously on `SidebarState.watchlist_manager`. Moved here so that the
    /// sidebar render, input handlers, and agent snapshot builder all read from
    /// one place.
    pub watchlist_manager: std::sync::RwLock<WatchlistManager>,

    /// Registry of attached `AppSession`s. One per consumer of this backend.
    ///
    /// In Mode 1 (embedded `ChartApp`) the host calls `register_session` once
    /// at construction and `deregister_session` on shutdown. In Mode 2 the
    /// daemon registers a session per external HTTP client.
    ///
    /// Sessions are pure metadata for now — `(app_id, label, created_at_secs,
    /// last_heartbeat_secs)`. The reactor logic that runs per session lives
    /// on the per-app side (`ChartApp::react_to_bubble_event`); session id
    /// is the handle the agent server uses to scope queries.
    pub sessions: std::sync::RwLock<SessionRegistry>,

    /// Registry of all drawing managers — one per chart canvas leaf
    /// (`ChartPanelId`).
    ///
    /// Previously owned by `ChartPanelState` in `TradingPanelsStore` (mlc-app).
    /// Moved here so every consumer (renderer, input handler, preset persist,
    /// headless API) reads from a single authoritative location, same pattern
    /// as `indicators` and `managers`.
    ///
    /// In single-threaded embedded mode lock contention is zero — guards are
    /// taken and released within each logical frame / input handler arm.
    pub drawings: std::sync::RwLock<DrawingRegistry>,

    /// Optional tokio `Runtime` for native entry points that need to call
    /// `block_on` from sync input handlers (gate4agent PTY ops).
    ///
    /// `None` when the backend is constructed without a pre-built runtime
    /// (e.g. headless, daemon — those callers never invoke gate4agent).
    /// `Some` when constructed via [`BackendCore::new_with_runtime`].
    ///
    /// Wrapped in `RwLock` so the `Arc<BackendCore>` can be shared while
    /// a one-time write sets this field at construction time.
    #[cfg(not(target_arch = "wasm32"))]
    native_rt: std::sync::RwLock<Option<Arc<tokio::runtime::Runtime>>>,
}

/// Registry of attached `AppSession`s. Shared across the agent server's
/// `/v1/apps` endpoints and the daemon's `/v1/apps` endpoints — both end up
/// pointing at this one registry on the per-process `BackendCore`.
#[derive(Default)]
pub struct SessionRegistry {
    pub by_id: HashMap<String, AppSession>,
}

/// One attached consumer of the backend. Pure metadata. The render thread,
/// reactor, and persist layer all run regardless of how many sessions are
/// registered; sessions exist so external observers can correlate their own
/// client state with backend events.
#[derive(Clone, Debug)]
pub struct AppSession {
    pub app_id: String,
    pub label: Option<String>,
    pub created_at_secs: u64,
    pub last_heartbeat_secs: u64,
}

impl BackendCore {
    /// Construct a fresh backend wrapping an already-spawned `DataBridge`.
    pub fn new(bridge: Arc<DataBridge>, trading: Option<TradingManager>) -> Arc<Self> {
        let broadcast_rx = bridge.add_listener();
        Arc::new(Self {
            bubble: std::sync::RwLock::new(BubbleStore::new()),
            alert: std::sync::RwLock::new(AlertManager::new()),
            indicators: std::sync::RwLock::new(IndicatorManager::new()),
            broadcast_rx: std::sync::Mutex::new(broadcast_rx),
            bridge,
            mini_ticker: DashMap::new(),
            exchange_symbols: std::sync::RwLock::new(HashMap::new()),
            trading: trading.map(std::sync::Mutex::new),
            presets: std::sync::RwLock::new(HashMap::new()),
            template_manager: std::sync::RwLock::new(TemplateManager::new()),
            watchlist_manager: std::sync::RwLock::new(WatchlistManager::default()),
            sessions: std::sync::RwLock::new(SessionRegistry::default()),
            drawings: std::sync::RwLock::new(DrawingRegistry::new()),
            #[cfg(not(target_arch = "wasm32"))]
            native_rt: std::sync::RwLock::new(None),
        })
    }

    /// Construct a backend and immediately attach a native tokio `Runtime`.
    ///
    /// Use this from entry points that own a runtime and need gate4agent's
    /// sync `block_on` calls to work (i.e. embedded `ChartApp` on native).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_with_runtime(
        bridge: Arc<DataBridge>,
        trading: Option<TradingManager>,
        rt: Arc<tokio::runtime::Runtime>,
    ) -> Arc<Self> {
        let core = Self::new(bridge, trading);
        *core.native_rt.write().expect("native_rt poisoned") = Some(rt);
        core
    }

    /// Return a reference to the native tokio `Runtime`, if one was attached.
    ///
    /// Gate4agent input handlers call
    /// `self.core.native_rt().unwrap().block_on(...)` on native.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn native_rt(&self) -> Option<Arc<tokio::runtime::Runtime>> {
        self.native_rt.read().expect("native_rt poisoned").clone()
    }

    /// Register a new AppSession. Returns the freshly minted `app_id`.
    pub fn register_session(&self, label: Option<String>, now_secs: u64) -> String {
        let app_id = format!("app-{:016x}", rand_u64());
        let session = AppSession {
            app_id: app_id.clone(),
            label,
            created_at_secs: now_secs,
            last_heartbeat_secs: now_secs,
        };
        self.sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)")
            .by_id
            .insert(app_id.clone(), session);
        app_id
    }

    /// Drop a session by id. Returns true if it existed.
    pub fn deregister_session(&self, app_id: &str) -> bool {
        self.sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)")
            .by_id
            .remove(app_id)
            .is_some()
    }

    /// Bump the heartbeat on a session. Returns true if the session existed.
    pub fn heartbeat_session(&self, app_id: &str, now_secs: u64) -> bool {
        let mut reg = self
            .sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)");
        if let Some(s) = reg.by_id.get_mut(app_id) {
            s.last_heartbeat_secs = now_secs;
            true
        } else {
            false
        }
    }

    /// Drop sessions whose heartbeat is older than `idle_timeout_secs`.
    /// Returns the number reaped. Callers (Mode-2 daemon, embedded host idle
    /// timer) drive this periodically.
    pub fn reap_idle_sessions(&self, now_secs: u64, idle_timeout_secs: u64) -> usize {
        let mut reg = self
            .sessions
            .write()
            .expect("BackendCore.sessions poisoned (write)");
        let before = reg.by_id.len();
        reg.by_id
            .retain(|_, s| now_secs.saturating_sub(s.last_heartbeat_secs) < idle_timeout_secs);
        before - reg.by_id.len()
    }

    /// Snapshot of all currently-registered sessions.
    pub fn sessions_snapshot(&self) -> Vec<AppSession> {
        self.sessions
            .read()
            .expect("BackendCore.sessions poisoned (read)")
            .by_id
            .values()
            .cloned()
            .collect()
    }

    /// Read guard for the indicator manager.
    #[inline]
    pub fn indicators(&self) -> std::sync::RwLockReadGuard<'_, IndicatorManager> {
        self.indicators.read().expect("BackendCore.indicators poisoned (read)")
    }

    /// Write guard for the indicator manager.
    #[inline]
    pub fn indicators_mut(&self) -> std::sync::RwLockWriteGuard<'_, IndicatorManager> {
        self.indicators.write().expect("BackendCore.indicators poisoned (write)")
    }

    /// Read guard for the bubble store.  Hold for the duration of one logical
    /// read operation.  The guard drops automatically.
    #[inline]
    pub fn bubble(&self) -> std::sync::RwLockReadGuard<'_, BubbleStore> {
        self.bubble.read().expect("BackendCore.bubble poisoned (read)")
    }

    /// Write guard for the bubble store.  Take one per writer arm in tick.rs;
    /// release at end of arm.
    #[inline]
    pub fn bubble_mut(&self) -> std::sync::RwLockWriteGuard<'_, BubbleStore> {
        self.bubble.write().expect("BackendCore.bubble poisoned (write)")
    }

    /// Read guard for the alert manager.
    #[inline]
    pub fn alert(&self) -> std::sync::RwLockReadGuard<'_, AlertManager> {
        self.alert.read().expect("BackendCore.alert poisoned (read)")
    }

    /// Write guard for the alert manager.
    #[inline]
    pub fn alert_mut(&self) -> std::sync::RwLockWriteGuard<'_, AlertManager> {
        self.alert.write().expect("BackendCore.alert poisoned (write)")
    }

    /// Lock the trading manager for mutation. Returns `None` if trading is
    /// disabled, `Some(guard)` otherwise.
    #[inline]
    pub fn trading_lock(&self) -> Option<std::sync::MutexGuard<'_, TradingManager>> {
        self.trading.as_ref().map(|m| m.lock().expect("BackendCore.trading poisoned"))
    }

    /// Short alias for the most common read pattern on exchange symbols.
    #[inline]
    pub fn exchange_symbols_read(&self) -> std::sync::RwLockReadGuard<'_, HashMap<ExchangeId, Vec<SymbolInfo>>> {
        self.exchange_symbols.read().expect("BackendCore.exchange_symbols poisoned (read)")
    }

    #[inline]
    pub fn exchange_symbols_mut(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<ExchangeId, Vec<SymbolInfo>>> {
        self.exchange_symbols.write().expect("BackendCore.exchange_symbols poisoned (write)")
    }

    /// Read guard for the presets map.
    #[inline]
    pub fn presets(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, ChartPreset>> {
        self.presets.read().expect("BackendCore.presets poisoned (read)")
    }

    /// Write guard for the presets map.
    #[inline]
    pub fn presets_mut(&self) -> std::sync::RwLockWriteGuard<'_, HashMap<String, ChartPreset>> {
        self.presets.write().expect("BackendCore.presets poisoned (write)")
    }

    /// Read guard for the template manager.
    #[inline]
    pub fn template_manager(&self) -> std::sync::RwLockReadGuard<'_, TemplateManager> {
        self.template_manager.read().expect("BackendCore.template_manager poisoned (read)")
    }

    /// Write guard for the template manager.
    #[inline]
    pub fn template_manager_mut(&self) -> std::sync::RwLockWriteGuard<'_, TemplateManager> {
        self.template_manager.write().expect("BackendCore.template_manager poisoned (write)")
    }

    /// Read guard for the watchlist manager.
    #[inline]
    pub fn watchlist(&self) -> std::sync::RwLockReadGuard<'_, WatchlistManager> {
        self.watchlist_manager.read().expect("BackendCore.watchlist_manager poisoned (read)")
    }

    /// Write guard for the watchlist manager.
    #[inline]
    pub fn watchlist_mut(&self) -> std::sync::RwLockWriteGuard<'_, WatchlistManager> {
        self.watchlist_manager.write().expect("BackendCore.watchlist_manager poisoned (write)")
    }

    /// Read guard for the drawing registry.
    #[inline]
    pub fn drawings(&self) -> std::sync::RwLockReadGuard<'_, DrawingRegistry> {
        self.drawings.read().expect("BackendCore.drawings poisoned (read)")
    }

    /// Write guard for the drawing registry.
    #[inline]
    pub fn drawings_mut(&self) -> std::sync::RwLockWriteGuard<'_, DrawingRegistry> {
        self.drawings.write().expect("BackendCore.drawings poisoned (write)")
    }

    /// Drain the live-data broadcast channel, write incoming updates into
    /// shared state (bubble store, `mini_ticker`, `exchange_symbols`), and
    /// emit one [`BubbleEvent`] per logical write for per-app reactors.
    ///
    /// Uses `bubbles_for_instrument` to locate all registered bubbles for an
    /// incoming instrument key — correct for N≥1 apps sharing one BackendCore.
    ///
    /// Returns events in the order they were processed. Mode-1 embedded apps
    /// pass them to [`ChartApp::react_to_bubble_event`]; Mode-2 daemon drops
    /// or forwards them over HTTP/WS.
    ///
    /// # Lagged messages
    ///
    /// When the broadcast channel signals `Lagged`, this method silently
    /// skips the gap (no backfill trigger — that is per-app logic). The caller
    /// can detect a lag by comparing event counts across ticks if needed.
    pub fn tick_writers(&self, _now_ms: u64) -> Vec<BubbleEvent> {
        let mut events: Vec<BubbleEvent> = Vec::new();
        let mut rx = self.broadcast_rx.lock().expect("BackendCore.broadcast_rx poisoned");

        loop {
            let update = match rx.try_recv() {
                Ok(u) => u,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                Err(_) => break,
            };

            match update {
                // ── BarsLoaded ─────────────────────────────────────────────────
                LiveUpdate::BarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    let bids_for_instrument = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), account_type.short_label(), &symbol);
                    if bids_for_instrument.is_empty() {
                        // No registered bubble for this instrument yet; skip bubble write.
                        // Per-app reactors may still want to hear about this (e.g. to create
                        // a bubble and then write), but the write itself is a no-op.
                        events.push(BubbleEvent::BarsLoaded {
                            bid: BubbleId(0),
                            exchange_id,
                            symbol,
                            account_type: account_type.short_label().to_string(),
                            timeframe: tf_name,
                            bars,
                            prev_bars: Vec::new(),
                        });
                    } else {
                        for bid in bids_for_instrument {
                            // Snapshot bubble.bars BEFORE replacing so the
                            // reactor can compute a real view-anchor over the
                            // OLD bars (otherwise `prev` == `new` and the
                            // anchor collapses, leaving the viewport pinned to
                            // the historic index whenever bars grow underneath).
                            let prev_bars = {
                                let mgr = self.bubble();
                                mgr.bubble(bid)
                                    .map(|b| b.bars.clone())
                                    .unwrap_or_default()
                            };
                            {
                                let mut mgr = self.bubble_mut();
                                if let Some(bubble) = mgr.bubble_mut(bid) {
                                    bubble.bars = bars.clone();
                                }
                            }
                            events.push(BubbleEvent::BarsLoaded {
                                bid,
                                exchange_id,
                                symbol: symbol.clone(),
                                account_type: account_type.short_label().to_string(),
                                timeframe: tf_name.clone(),
                                bars: bars.clone(),
                                prev_bars,
                            });
                        }
                    }
                }

                // ── BackfillComplete ────────────────────────────────────────────
                LiveUpdate::BackfillComplete { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), account_type.short_label(), &symbol);
                    for bid in bids {
                        // Same rationale as BarsLoaded above: snapshot the OLD
                        // bubble.bars before overwrite so the reactor's
                        // capture_view_anchor sees real prior state.
                        let prev_bars = {
                            let mgr = self.bubble();
                            mgr.bubble(bid)
                                .map(|b| b.bars.clone())
                                .unwrap_or_default()
                        };
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                bubble.bars = bars.clone();
                            }
                        }
                        events.push(BubbleEvent::BackfillCompleted {
                            bid,
                            exchange_id,
                            symbol: symbol.clone(),
                            account_type: account_type.short_label().to_string(),
                            timeframe: tf_name.clone(),
                            bars: bars.clone(),
                            prev_bars,
                        });
                    }
                }

                // ── ScrollBarsLoaded ────────────────────────────────────────────
                // The bridge's `request_scroll_bars` REST path returns ONLY the
                // older page (bars older than `before_ts`). Replacing bubble.bars
                // with that page wholesale would drop every live / right-edge
                // bar the chart has accumulated since seed. Merge by timestamp
                // (last-write-wins so the live ring's version of any overlapping
                // bar takes priority), then sort oldest→newest. Pre-cutover native
                // worked because the legacy path concatenated server-side; this
                // branch is the merge that path used to do for us.
                LiveUpdate::ScrollBarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars, prepend_count } => {
                    let at_label = account_type.short_label().to_string();
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, &symbol);
                    for bid in bids {
                        let merged: Vec<mlc_core::Bar> = {
                            let mgr = self.bubble();
                            let existing = mgr.bubble(bid)
                                .map(|b| b.bars.clone())
                                .unwrap_or_default();
                            // Build a ts→bar map: insert the older REST page first,
                            // then overwrite with existing bars so live/right-edge
                            // versions win on any timestamp collision.
                            let mut by_ts: std::collections::BTreeMap<i64, mlc_core::Bar> =
                                std::collections::BTreeMap::new();
                            for b in &bars { by_ts.insert(b.timestamp, *b); }
                            for b in &existing { by_ts.insert(b.timestamp, *b); }
                            by_ts.into_values().collect()
                        };
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                bubble.bars = merged.clone();
                            }
                        }
                        events.push(BubbleEvent::ScrollBarsAppended {
                            bid,
                            exchange_id,
                            symbol: symbol.clone(),
                            account_type: at_label.clone(),
                            timeframe: tf_name.clone(),
                            bars: merged,
                            prepend_count,
                        });
                    }
                }

                // ── BarUpdate — pass-through ────────────────────────────────────
                // Bubble write depends on per-window `is_derived` / `chart_type` flags;
                // reactor owns the full arm.
                LiveUpdate::BarUpdate { exchange_id, account_type: bar_at, symbol: bar_sym, bar } => {
                    events.push(BubbleEvent::BarUpdatePassthrough {
                        exchange_id,
                        account_type: bar_at.short_label().to_string(),
                        symbol: bar_sym,
                        bar,
                    });
                }

                // ── TradeUpdate — pass-through ──────────────────────────────────
                // Aggregation period and derived-window exclusion depend on per-window
                // TF metadata and `is_derived` flags; reactor owns the full arm.
                LiveUpdate::TradeUpdate { exchange_id, account_type, symbol, price, quantity, timestamp, is_buyer_maker } => {
                    events.push(BubbleEvent::TradeUpdatePassthrough {
                        exchange_id,
                        account_type: account_type.short_label().to_string(),
                        symbol,
                        price,
                        quantity,
                        timestamp,
                        is_buyer_maker,
                    });
                }

                // ── MiniTickerUpdate ────────────────────────────────────────────
                LiveUpdate::MiniTickerUpdate { exchange_id, account_type, symbol, last_price,
                    price_change_percent, high_price, low_price, volume } =>
                {
                    let cache_key = format!("{}:{}:{}", symbol, exchange_id.as_str(), account_type.short_label());
                    let mut entry = self.mini_ticker
                        .entry(cache_key)
                        .or_insert(MiniTickerData {
                            last_price,
                            price_change_percent: 0.0,
                            high_price: 0.0,
                            low_price: 0.0,
                            volume: 0.0,
                        });
                    entry.last_price = last_price;
                    if let Some(v) = price_change_percent { entry.price_change_percent = v; }
                    if let Some(v) = high_price           { entry.high_price = v; }
                    if let Some(v) = low_price            { entry.low_price  = v; }
                    if let Some(v) = volume               { entry.volume     = v; }
                    events.push(BubbleEvent::MiniTickerUpdated {
                        exchange_id,
                        symbol,
                        account_type: account_type.short_label().to_string(),
                        last_price,
                    });
                }

                // ── ConnectorReady ──────────────────────────────────────────────
                LiveUpdate::ConnectorReady { exchange_id } => {
                    events.push(BubbleEvent::ConnectorReady { exchange_id });
                }

                // ── SymbolsLoaded ───────────────────────────────────────────────
                LiveUpdate::SymbolsLoaded { exchange_id, symbols } => {
                    let count = symbols.len();
                    self.exchange_symbols_mut().insert(exchange_id, symbols);
                    self.bridge.prewarm_wire_symbol_cache(exchange_id);
                    events.push(BubbleEvent::SymbolsLoaded { exchange_id, count });
                }

                // ── Error ───────────────────────────────────────────────────────
                LiveUpdate::Error { exchange_id, account_type, symbol, timeframe, message } => {
                    events.push(BubbleEvent::ErrorReported {
                        exchange_id,
                        symbol,
                        account_type: account_type.short_label().to_string(),
                        timeframe,
                        message,
                    });
                }

                // ── OrderbookSnapshot ───────────────────────────────────────────
                LiveUpdate::OrderbookSnapshot { exchange_id, account_type, symbol, source, .. } => {
                    if matches!(source, OrderbookSource::Rest) {
                        events.push(BubbleEvent::OrderbookSnapshotRest {
                            exchange_id,
                            symbol,
                            account_type: account_type.short_label().to_string(),
                        });
                    }
                    // WS snapshot: tracker already updated in bridge; skip.
                }

                // ── OrderbookDelta ──────────────────────────────────────────────
                LiveUpdate::OrderbookDelta { exchange_id, account_type, symbol, bids, asks, timestamp } => {
                    let at_str = account_type.short_label().to_string();
                    // Emit one event for the L2-tape / DOM coalescer.
                    events.push(BubbleEvent::OrderbookDelta {
                        exchange_id,
                        symbol: symbol.clone(),
                        account_type: at_str.clone(),
                        bids: bids.clone(),
                        asks: asks.clone(),
                        ts_ms: timestamp,
                    });
                    // Separate event tag for multi-dom fan-out so reactor can
                    // distinguish without re-cloning the slice.
                    events.push(BubbleEvent::OrderbookDeltaForMultiDom {
                        exchange_id,
                        symbol,
                        account_type: at_str,
                        bids,
                        asks,
                    });
                }

                // ── ConnectorMetrics ────────────────────────────────────────────
                LiveUpdate::ConnectorMetrics { .. } => {
                    // Collected on-demand; no action.
                }

                // ── Trading (Order / Balance / Position) ─────────────────────────
                LiveUpdate::OrderUpdate { .. }
                | LiveUpdate::BalanceUpdate { .. }
                | LiveUpdate::PositionUpdate { .. } => {
                    events.push(BubbleEvent::TradingUpdate { update });
                }

                // ── FootprintUpdate ─────────────────────────────────────────────
                LiveUpdate::FootprintUpdate { exchange_id, account_type: fp_at, symbol: fp_sym, point } => {
                    let fp_at_label = fp_at.short_label().to_string();
                    let new_bar = mlc_core::FootprintBar {
                        open_time: point.open_time,
                        open: point.open,
                        high: point.high,
                        low: point.low,
                        close: point.close,
                        volume: point.volume,
                        levels: point.levels.iter()
                            .map(|(price, buy_vol, sell_vol)| mlc_core::FootprintLevel {
                                price: *price,
                                buy_vol: *buy_vol,
                                sell_vol: *sell_vol,
                                is_single_print: false,
                                is_unfinished: false,
                            })
                            .collect(),
                        cumulative_delta: 0.0,
                        stacked_ask_start: None,
                        stacked_ask_len: None,
                        stacked_bid_start: None,
                        stacked_bid_len: None,
                        is_absorption: false,
                    };
                    let tick_size = {
                        let mut step = f64::INFINITY;
                        for w in new_bar.levels.windows(2) {
                            let d = (w[1].price - w[0].price).abs();
                            if d > 0.0 && d < step { step = d; }
                        }
                        if step.is_finite() { step } else { 1.0 }
                    };
                    let mut new_bar_with_static = new_bar;
                    new_bar_with_static.compute_static_extras(3.0, 3, 0.15, 0.05, tick_size);
                    let ohlc_ts = new_bar_with_static.open_time / 1000;
                    let ohlc_bar = mlc_core::Bar::with_volume(
                        ohlc_ts,
                        new_bar_with_static.open,
                        new_bar_with_static.high,
                        new_bar_with_static.low,
                        new_bar_with_static.close,
                        new_bar_with_static.volume,
                    );
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &fp_at_label, fp_sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                let prev_cvd = {
                                    let store = bubble.footprint_bars.get_or_insert_with(Vec::new);
                                    match store.last() {
                                        Some(last) if last.open_time != new_bar_with_static.open_time => last.cumulative_delta,
                                        Some(_) if store.len() >= 2 => store[store.len() - 2].cumulative_delta,
                                        _ => 0.0,
                                    }
                                };
                                let store = bubble.footprint_bars.get_or_insert_with(Vec::new);
                                let mut bar = new_bar_with_static.clone();
                                bar.set_cumulative_delta(prev_cvd);
                                match store.last_mut() {
                                    Some(last) if last.open_time == bar.open_time => { *last = bar; }
                                    _ => store.push(bar),
                                }
                                let last_ohlc_ts = bubble.bars.last().map(|b| b.timestamp);
                                if last_ohlc_ts == Some(ohlc_ts) {
                                    if let Some(last) = bubble.last_bar_mut() {
                                        last.open   = ohlc_bar.open;
                                        last.high   = ohlc_bar.high;
                                        last.low    = ohlc_bar.low;
                                        last.close  = ohlc_bar.close;
                                        last.volume = ohlc_bar.volume;
                                    }
                                } else {
                                    bubble.append_bar(ohlc_bar.clone());
                                }
                            }
                        }
                        events.push(BubbleEvent::FootprintBarAppended {
                            bid,
                            exchange_id,
                            symbol: fp_sym.clone(),
                            account_type: fp_at_label.clone(),
                        });
                    }
                }

                // ── RenkoBarUpdate ──────────────────────────────────────────────
                LiveUpdate::RenkoBarUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let brick = mlc_core::types::RenkoBrick {
                        open_time: point.open_time,
                        bottom: point.bottom,
                        top: point.top,
                        up: point.up,
                        volume: point.volume,
                        trades_count: point.trades_count,
                    };
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                bubble.append_renko(brick.clone());
                            }
                        }
                        events.push(BubbleEvent::RenkoBarAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── PnfBarUpdate ────────────────────────────────────────────────
                LiveUpdate::PnfBarUpdate { exchange_id, account_type: at, symbol: sym, point, box_size: bs } => {
                    let at_label = at.short_label().to_string();
                    let col = mlc_core::types::PnfColumn {
                        open_time: point.open_time,
                        column_id: point.column_id,
                        is_x: point.is_x,
                        bottom: point.bottom,
                        top: point.top,
                        volume: point.volume,
                        trades_count: point.trades_count,
                        box_size: bs,
                    };
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                let store = bubble.pnf_columns.get_or_insert_with(Vec::new);
                                match store.last_mut() {
                                    Some(last) if last.column_id == col.column_id => *last = col.clone(),
                                    _ => store.push(col.clone()),
                                }
                            }
                        }
                        events.push(BubbleEvent::PnfBarAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── KagiBarUpdate ───────────────────────────────────────────────
                LiveUpdate::KagiBarUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let seg = mlc_core::types::KagiSegment {
                        open_time: point.open_time,
                        start_price: point.start_price,
                        end_price: point.end_price,
                        up: point.up,
                        yang: point.yang,
                        is_connector: point.is_connector,
                    };
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                bubble.append_kagi(seg.clone());
                            }
                        }
                        events.push(BubbleEvent::KagiBarAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── CvdLineUpdate ───────────────────────────────────────────────
                LiveUpdate::CvdLineUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let sample = mlc_core::types::CvdSample {
                        ts_ms: point.ts_ms,
                        value: point.value,
                    };
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                bubble.append_cvd(sample);
                            }
                        }
                        events.push(BubbleEvent::CvdSampleAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── TpoProfileUpdate ────────────────────────────────────────────
                LiveUpdate::TpoProfileUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let session = mlc_core::types::TpoSession {
                        open_time: point.open_time,
                        tick_size: point.tick_size,
                        session_high: point.session_high,
                        session_low: point.session_low,
                        poc_price: point.poc_price,
                        vah_price: point.vah_price,
                        val_price: point.val_price,
                        rows: point.rows.clone(),
                    };
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                let store = bubble.tpo_sessions.get_or_insert_with(Vec::new);
                                match store.last_mut() {
                                    Some(last) if last.open_time == session.open_time => *last = session.clone(),
                                    _ => {
                                        store.push(session.clone());
                                        const MAX_TPO_SESSIONS: usize = 11;
                                        while store.len() > MAX_TPO_SESSIONS {
                                            store.remove(0);
                                        }
                                    }
                                }
                            }
                        }
                        events.push(BubbleEvent::TpoSessionUpdated {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── ThreeLineBreakUpdate ────────────────────────────────────────
                LiveUpdate::ThreeLineBreakUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    let at_label = at.short_label().to_string();
                    let line = mlc_core::types::ThreeLineBreakLine {
                        ts_open: point.ts_open,
                        ts_close: point.ts_close,
                        price_open: point.open,
                        price_close: point.close,
                        volume: point.volume,
                        direction: point.direction,
                    };
                    let bids = self.bubble()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        {
                            let mut mgr = self.bubble_mut();
                            if let Some(bubble) = mgr.bubble_mut(bid) {
                                bubble.append_three_line_break(line.clone());
                            }
                        }
                        events.push(BubbleEvent::TlbLineAppended {
                            bid,
                            exchange_id,
                            symbol: sym.clone(),
                            account_type: at_label.clone(),
                        });
                    }
                }

                // ── OpenInterestUpdate ──────────────────────────────────────────
                // Write target depends on per-app `oi_show` window flag — pass through.
                LiveUpdate::OpenInterestUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    events.push(BubbleEvent::OiUpdatePassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        point,
                    });
                }

                // ── FundingRateUpdate ───────────────────────────────────────────
                // Write target depends on per-app `funding_show` window flag — pass through.
                LiveUpdate::FundingRateUpdate { exchange_id, account_type: at, symbol: sym, point } => {
                    events.push(BubbleEvent::FundingRatePassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        point,
                    });
                }

                // ── DomHeatmapSnapshot — pass-through ───────────────────────────
                // Write target depends on per-app `chart_type=="dom_heatmap"` filter.
                LiveUpdate::DomHeatmapSnapshot { exchange_id, account_type: at, symbol: sym, ts_ms, bids, asks } => {
                    events.push(BubbleEvent::DomHeatmapSnapshotPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        ts_ms,
                        bids,
                        asks,
                    });
                }

                // ── DomTradeDot ─────────────────────────────────────────────────
                // Write target depends on per-app `chart_type=="dom_heatmap"` — pass through.
                LiveUpdate::DomTradeDot { exchange_id, account_type: at, symbol: sym, dot } => {
                    events.push(BubbleEvent::DomTradeDotPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        dot,
                    });
                }

                // ── LiquidationEvent — pass-through ─────────────────────────────
                // Write target depends on per-app `chart_type=="liquidation_heatmap"`.
                LiveUpdate::LiquidationEvent { exchange_id, account_type: at, symbol: sym, ts_ms, price, value_usd, side } => {
                    events.push(BubbleEvent::LiquidationEventPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        ts_ms,
                        price,
                        value_usd,
                        side,
                    });
                }

                // ── LiquidationBucketUpdate — pass-through ───────────────────────
                // Write target depends on per-app `chart_type=="liquidation_heatmap"`.
                LiveUpdate::LiquidationBucketUpdate { exchange_id, account_type: at, symbol: sym, ts_ms, long_liq_usd, short_liq_usd, .. } => {
                    events.push(BubbleEvent::LiquidationBucketPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        ts_ms,
                        long_liq_usd,
                        short_liq_usd,
                    });
                }
            }
        }

        events
    }

    /// Mode-2 daemon entry point: drive the writers, discard per-app events.
    ///
    /// Equivalent to `let _ = self.tick_writers(now_ms);` but more explicit
    /// about intent — the daemon has no `ChartApp` to consume events.
    pub fn tick_bridge_only(&self, now_ms: u64) {
        let _ = self.tick_writers(now_ms);
    }
}

/// xorshift64 over a thread-local seed; good enough for app-id uniqueness
/// in-process. Not a security-grade RNG; never use for crypto.
fn rand_u64() -> u64 {
    use std::cell::Cell;
    use web_time::SystemTime;
    use web_time::UNIX_EPOCH;
    thread_local! {
        static STATE: Cell<u64> = Cell::new(0);
    }
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            // Lazy seed from wall clock + thread id hash.
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdead_beef);
            x = now ^ 0x9E37_79B9_7F4A_7C15;
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        x
    })
}
