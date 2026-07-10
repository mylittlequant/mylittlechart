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
//! into a `mlc-backend` crate above `mlc-backend-indicator-engine`.
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
//! - **Final** — lift module into `mlc-backend` crate.

pub mod bubble;
pub mod alert;
pub mod chart_type;
pub mod drawing;
pub mod indicator;
pub mod watchlist;
pub mod tag;
pub mod preset;
pub mod template;
pub mod user;
pub mod trading;
pub mod exchange_symbols;

pub use alert::{Alerts, AlertsMut};
pub use drawing::{Drawings, DrawingsMut, SwapSymbolResult};
pub use bubble::{Bubbles, BubblesMut};
pub use indicator::{IndicatorFullConfig, Indicators, IndicatorsMut};
pub use watchlist::{Watchlists, WatchlistsMut};
pub use tag::{Tags, TagsMut};
pub use preset::{Presets, PresetsMut};
pub use template::{Templates, TemplatesMut};
pub use user::{User, UserMut};
pub use trading::{Trading, TradingMut};
pub use exchange_symbols::{ExchangeSymbols, ExchangeSymbolsMut};

use std::sync::Arc;
use std::collections::HashMap;

use dashmap::DashMap;
use serde::{Serialize, Serializer};

use digdigdig3::{AccountType, ExchangeId};
use mlc_core::ids::BubbleId;
use mlc_core::state::Timeframe;
use crate::alert::AlertManager;
use crate::bubble::BubbleStore;
use crate::drawing::DrawingManager;
use mlc_core::Bar;
use crate::indicator::IndicatorManager;
use mlc_backend_data_bridge::{
    DataBridge, LiveUpdate, LiveUpdateReceiver, OrderbookSource,
    SharedTradeSeries, SharedOrderBookTracker,
};
use crate::trading::TradingManager;
use crate::watchlist::WatchlistManager;
use crate::tag::TagManager;
use crate::preset::PresetManager;
use crate::template::TemplateManager;
use crate::user::UserProfileManager;
use crate::exchange_symbols::ExchangeSymbolsManager;

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
    ///
    /// `prev_bars` is the snapshot of `bubble.bars` taken in `tick_writers`
    /// *before* the raw kline layer's prepend (`kline_layer_prepend_batch`)
    /// and the display rebuild from that layer (P0 fix 2026-07-09, see the
    /// `ScrollBarsLoaded` arm) landed. Same rationale as
    /// `BarsLoaded`/`BackfillCompleted` (see doc comment above `BarsLoaded`):
    /// re-reading the bubble after the merge gives back the already-front-
    /// extended array, and `capture_view_anchor` would then index `view_start`
    /// into the NEW array — collapsing the anchor onto a bar `prepend_count`
    /// positions further back in time than what the user was actually
    /// looking at pre-merge. Every scroll-left/zoom-out backfill round-trip
    /// re-locked the viewport to a progressively older date window. Field
    /// added 2026-07-06.
    ScrollBarsAppended {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        timeframe: String,
        bars: Vec<Bar>,
        prev_bars: Vec<Bar>,
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
    /// DOM heatmap snapshot — write target depends on per-app `dom_heatmap_show`
    /// flag (chart-type → overlay migration; mirrors `oi_show`/`funding_show`).
    /// Pass-through: reactor finds matching window bubbles and does the write.
    DomHeatmapSnapshotPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        bids: Vec<(f64, f64)>,
        asks: Vec<(f64, f64)>,
    },
    /// DOM trade dot — write target depends on per-app `dom_heatmap_show` flag.
    /// Pass-through: reactor finds matching window bubbles and does the write.
    DomTradeDotPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        dot: mlc_core::chart::dom_heatmap::DomTradeDot,
    },
    /// Liquidation per-event — write target depends on per-app `liq_heatmap_show`
    /// flag (chart-type → overlay migration; mirrors `oi_show`/`funding_show`).
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
    /// Liquidation bucket update — write target depends on per-app `liq_heatmap_show` flag.
    /// Pass-through.
    LiquidationBucketPassthrough {
        exchange_id: ExchangeId,
        account_type: String,
        symbol: String,
        ts_ms: i64,
        long_liq_usd: f64,
        short_liq_usd: f64,
    },
    /// History depth report landed for `bid`'s currently-active chart_type
    /// (hybrid-data-modes-plan-2026-07-08, Step 1). The bubble write
    /// (`DataBubble::set_history_depth`) already happened in `tick_writers`
    /// — this event exists so the reactor can clear a stuck
    /// `pending_symbol_load` for a `LiveOnly`/tiny-`Capped` batch-cut
    /// chart_type whose cold seed produced zero complete bars (Step 4: the
    /// eternal-skeleton fix — a tick_bar/volume_bar seed of 60 trades can
    /// complete zero buckets, so `LiveUpdate::BarsLoaded` never fires and
    /// the spinner would otherwise hang until enough live trades land).
    HistoryDepthReported {
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: String,
        account_type: String,
        chart_type: &'static str,
        depth: crate::bubble::HistoryDepth,
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
            Self::ScrollBarsAppended { bid, exchange_id, symbol, account_type, timeframe, bars, prev_bars: _, prepend_count } => {
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
            Self::HistoryDepthReported { bid, exchange_id, symbol, account_type, chart_type, depth } => {
                let mut m = s.serialize_map(Some(6))?;
                m.serialize_entry("type", "HistoryDepthReported")?;
                m.serialize_entry("bid", &bid.0)?;
                m.serialize_entry("exchange_id", ex!(exchange_id))?;
                m.serialize_entry("symbol", symbol)?;
                m.serialize_entry("account_type", account_type)?;
                m.serialize_entry("chart_type", chart_type)?;
                // `HistoryDepth` does not impl Serialize — inline as a tagged shape.
                match depth {
                    crate::bubble::HistoryDepth::Unknown => {
                        m.serialize_entry("depth", "unknown")?;
                    }
                    crate::bubble::HistoryDepth::Full => {
                        m.serialize_entry("depth", "full")?;
                    }
                    crate::bubble::HistoryDepth::Capped { achieved } => {
                        m.serialize_entry("depth", "capped")?;
                        m.serialize_entry("achieved", achieved)?;
                    }
                    crate::bubble::HistoryDepth::LiveOnly => {
                        m.serialize_entry("depth", "live_only")?;
                    }
                }
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
    /// taken and dropped each tick frame, no waits. Accessed exclusively
    /// through the `Bubbles` / `BubblesMut` facade (`BackendCore::bubbles` /
    /// `bubbles_mut`) — Step 7 doctrine cutover.
    bubble: std::sync::RwLock<BubbleStore>,

    /// Alert manager — owns the per-bubble last-tick-price cache used for
    /// crossing detection (`AlertManager::on_tick`). Alert *data*
    /// (`alerts`, `alert_next_id`) lives on `DataBubble`, mutated through
    /// this manager via the `Alerts` / `AlertsMut` facade
    /// (`BackendCore::alerts` / `alerts_mut`).
    ///
    /// LOCK ORDER: every facade/accessor that takes both `bubble` and
    /// `alert` MUST acquire `bubble` first, then `alert` — see
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.
    alert: std::sync::RwLock<AlertManager>,

    /// Indicator manager — owns all indicator instances + recalc state +
    /// per-window assignment + global definitions catalog. Compute
    /// primitives it calls into (`IndicatorCalculator`, catalog bridge,
    /// `mli`) live in `mlc-backend-indicator-engine`; the manager itself
    /// lives in `mlc_backend::indicator::manager` (doctrine 3.7a — engine =
    /// compute, manager = state, managers live in mlc-backend).
    indicators: std::sync::RwLock<IndicatorManager>,

    /// Live data bridge — owns the tokio runtime, station, connector pool,
    /// broadcast channel. Already internally synchronised; `Arc::clone` is
    /// the only thing consumers need.
    ///
    /// Private (Step 12 of the chart-type registry arc closed the `pub`
    /// hole) — external callers (mlc-app*, shells) go through the facade
    /// methods below (`request_kline`, `subscribe_depth`, ...) or, for the
    /// narrow set of call sites with no facade wrapper (chart-type registry
    /// dispatch, mini-ticker subscribe, ws metrics), through
    /// [`BackendCore::bridge_handle`] / [`BackendCore::bridge_arc`]. Do not
    /// widen this back to `pub` — that reopens the exact hole Step 12 closed.
    bridge: Arc<DataBridge>,

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
    /// frequent (symbol search overlay, watchlist). Accessed exclusively
    /// through the `ExchangeSymbols` / `ExchangeSymbolsMut` facade
    /// (`BackendCore::exchange_symbols` / `exchange_symbols_mut`) — Step 7
    /// doctrine cutover.
    exchange_symbols: std::sync::RwLock<ExchangeSymbolsManager>,

    /// Order routing + paper engine. `None` when trading is disabled.
    /// Wrapped in `Mutex` because `TradingManager` has `&mut self` methods
    /// (`on_connector_ready`, `tick`, `refresh_balances`, etc) and is mutated
    /// from the tick loop. Single-threaded contention in embedded mode = 0.
    /// Accessed exclusively through the `Trading` / `TradingMut` facade
    /// (`BackendCore::trading` / `trading_mut`) — doctrine 3.7c.
    trading: Option<std::sync::Mutex<TradingManager>>,

    /// Chart preset registry.
    ///
    /// Previously lived on `ChartPanelApp.presets` as a bare
    /// `HashMap<String, ChartPreset>`. Moved here so that the persist layer,
    /// input handlers, and render path all share one authoritative copy
    /// without per-window duplication (single-`BackendCore` hosts). Accessed
    /// exclusively through the `Presets` / `PresetsMut` facade
    /// (`BackendCore::presets` / `presets_mut`).
    ///
    /// LOCK ORDER: `preset` never co-acquires `bubble` — it is a leaf lock,
    /// same shape as `watchlist_manager` (§1A).
    preset: std::sync::RwLock<PresetManager>,

    /// Template manager — drawing primitive / indicator / compare / chart
    /// templates and indicator sets.
    ///
    /// Previously on `ChartPanelApp.template_manager` (before that,
    /// `mlc-core::templates::manager::TemplateManager`). Moved here per
    /// doctrine 1A. Accessed exclusively through the `Templates` /
    /// `TemplatesMut` facade (`BackendCore::templates` / `templates_mut`).
    ///
    /// LOCK ORDER: `template` never co-acquires `bubble` — leaf lock.
    template: std::sync::RwLock<TemplateManager>,

    /// User profile + settings snapshots + vault key.
    ///
    /// Previously duplicated across `mlc-app::user_manager::{UserManager,
    /// ProfileManager}` and `mlc-app-shell-vello::AppState`. Moved here per
    /// doctrine 1B — a headless daemon UI needs `UserProfile` (bar_count,
    /// data_load, recalc_mode, scale_mode) to behave correctly, same as a
    /// native window. Accessed exclusively through the `User` / `UserMut`
    /// facade (`BackendCore::user` / `user_mut`).
    ///
    /// LOCK ORDER: `user` never co-acquires `bubble` — leaf lock.
    user: std::sync::RwLock<UserProfileManager>,

    /// Watchlist manager — all named watchlists with their symbol rows.
    ///
    /// Previously on `SidebarState.watchlist_manager`. Moved here so that the
    /// sidebar render, input handlers, and agent snapshot builder all read from
    /// one place. Accessed exclusively through the `Watchlists` /
    /// `WatchlistsMut` facade (`BackendCore::watchlists` / `watchlists_mut`)
    /// — Step 7 doctrine cutover.
    watchlist_manager: std::sync::RwLock<WatchlistManager>,

    /// Tag manager — registry of `Tag`s paired 1:1 with bubbles, plus the
    /// member→tag routing table and the synced-panel set.
    ///
    /// Previously `mlc-app::tag::TagStore`. Moved here per doctrine 1B:
    /// bubble grouping / member routing is domain, a headless daemon UI
    /// cannot sync panels without it. Accessed exclusively through the
    /// `Tags` / `TagsMut` facade (`BackendCore::tags` / `tags_mut`).
    ///
    /// LOCK ORDER: `tag` never co-acquires `bubble` — it is a leaf lock,
    /// same shape as `watchlist_manager` (§1A).
    tag: std::sync::RwLock<TagManager>,

    /// Registry of attached `AppSession`s. One per consumer of this backend.
    ///
    /// In Mode 1 (embedded `ChartApp`) the host calls `register_session` once
    /// at construction and `deregister_session` on shutdown. In Mode 2 the
    /// daemon registers a session per external HTTP client.
    ///
    /// Sessions are pure metadata for now — `(app_id, label, created_at_secs,
    /// last_heartbeat_secs)`. The reactor logic that runs per session lives
    /// on the per-app side (`ChartApp::react_to_bubble_event`); session id
    /// is the handle the agent server uses to scope queries. Accessed only
    /// through `register_session`/`deregister_session`/`heartbeat_session`/
    /// `reap_idle_sessions`/`sessions_snapshot` — no facade needed, this
    /// registry never held a raw-guard escape.
    sessions: std::sync::RwLock<SessionRegistry>,

    /// Claim registry: `preset_id -> owning app_id`, enforcing the
    /// shared-BackendCore-arc invariant that a preset is either
    /// parked-nowhere (never opened this session) or live in exactly one
    /// window (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §5.3.5).
    /// Before this arc "elsewhere" meant "a different, unrelated
    /// `BackendCore`" so collision was structurally impossible; under one
    /// shared core it is now a real hazard park/unpark must guard against.
    /// Release is tied to the claiming window's lifecycle the same way
    /// `sessions` is — see `try_claim_preset` / `release_preset` /
    /// `release_all_presets_for_app`.
    open_presets: std::sync::RwLock<HashMap<String, String>>,

    /// The drawing manager — one editing session per bubble
    /// (`BubbleDrawingSession`, keyed by `BubbleId` internally).
    ///
    /// Drawing data (`primitives`) lives on `DataBubble.primitives`; this
    /// manager mutates it through `&mut BubbleStore`. Accessed exclusively
    /// through the `Drawings` / `DrawingsMut` facade — see
    /// `BackendCore::drawings` / `drawings_mut` below.
    ///
    /// In single-threaded embedded mode lock contention is zero — guards are
    /// taken and released within each logical frame / input handler arm.
    drawing: std::sync::RwLock<DrawingManager>,

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
            alert: std::sync::RwLock::new(AlertManager::default()),
            indicators: std::sync::RwLock::new(IndicatorManager::new()),
            broadcast_rx: std::sync::Mutex::new(broadcast_rx),
            bridge,
            mini_ticker: DashMap::new(),
            exchange_symbols: std::sync::RwLock::new(ExchangeSymbolsManager::new()),
            trading: trading.map(std::sync::Mutex::new),
            preset: std::sync::RwLock::new(PresetManager::new()),
            template: std::sync::RwLock::new(TemplateManager::new()),
            user: std::sync::RwLock::new(UserProfileManager::new()),
            watchlist_manager: std::sync::RwLock::new(WatchlistManager::default()),
            tag: std::sync::RwLock::new(TagManager::new()),
            sessions: std::sync::RwLock::new(SessionRegistry::default()),
            open_presets: std::sync::RwLock::new(HashMap::new()),
            drawing: std::sync::RwLock::new(DrawingManager::default()),
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

    /// Claim `preset_id` for `app_id`. Returns `true` if the claim succeeds
    /// (preset was unclaimed, or already claimed by this same `app_id` —
    /// re-claiming your own already-open preset is not a collision).
    /// Returns `false` if another `app_id` already holds it.
    ///
    /// Call before parking a preset into `live_preset_cache` or unparking /
    /// cold-loading it into `panel_grid` — see `mlc-app/src/persistence/
    /// profile.rs`. Guards the shared-BackendCore-arc invariant that a
    /// preset is live in at most one window at a time (§5.3.5).
    pub fn try_claim_preset(&self, preset_id: &str, app_id: &str) -> bool {
        let mut reg = self
            .open_presets
            .write()
            .expect("BackendCore.open_presets poisoned (write)");
        match reg.get(preset_id) {
            Some(owner) => owner == app_id,
            None => {
                reg.insert(preset_id.to_string(), app_id.to_string());
                true
            }
        }
    }

    /// Release `preset_id`'s claim if it is currently held by `app_id`.
    /// No-op (and returns `false`) if unclaimed or held by a different
    /// `app_id` — releasing a preset you don't own is never valid.
    pub fn release_preset(&self, preset_id: &str, app_id: &str) -> bool {
        let mut reg = self
            .open_presets
            .write()
            .expect("BackendCore.open_presets poisoned (write)");
        if reg.get(preset_id).map(String::as_str) == Some(app_id) {
            reg.remove(preset_id);
            true
        } else {
            false
        }
    }

    /// Release every preset claimed by `app_id`. Callers tie this to the
    /// same lifecycle event as `deregister_session` (window close / session
    /// reap) so a crashed or force-closed window never leaves a preset
    /// claimed forever (§5.3.5, risk 5 — "do not invent a second cleanup
    /// mechanism").
    pub fn release_all_presets_for_app(&self, app_id: &str) -> usize {
        let mut reg = self
            .open_presets
            .write()
            .expect("BackendCore.open_presets poisoned (write)");
        let before = reg.len();
        reg.retain(|_, owner| owner != app_id);
        before - reg.len()
    }

    /// Read-only facade over the indicator manager. The ONLY doctrinally-
    /// sanctioned access path to indicator data (Step 3.3,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A). Does not
    /// co-acquire `bubble` — every read-only method operates purely on
    /// manager state.
    #[inline]
    pub fn indicators(&self) -> Indicators<'_> {
        let mgr = self.indicators.read().expect("BackendCore.indicators poisoned (read)");
        Indicators::new(mgr)
    }

    /// Mutable facade over the indicator manager + bubble store.
    ///
    /// LOCK ORDER: bubble first, then indicators.
    #[inline]
    pub fn indicators_mut(&self) -> IndicatorsMut<'_> {
        let bubbles = self.bubble.write().expect("BackendCore.bubble poisoned (write)");
        let mgr = self.indicators.write().expect("BackendCore.indicators poisoned (write)");
        IndicatorsMut::new(mgr, bubbles)
    }

    /// Read-only facade over the bubble store. The ONLY doctrinally-
    /// sanctioned access path for external (app/daemon/headless) callers
    /// (Step 3.2, `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A).
    ///
    /// LOCK ORDER: bubble is the base lock — nothing else needs to be
    /// acquired before it.
    #[inline]
    pub fn bubbles(&self) -> Bubbles<'_> {
        Bubbles::new(self.bubble.read().expect("BackendCore.bubble poisoned (read)"))
    }

    /// Mutable facade over the bubble store.
    #[inline]
    pub fn bubbles_mut(&self) -> BubblesMut<'_> {
        BubblesMut::new(self.bubble.write().expect("BackendCore.bubble poisoned (write)"))
    }

    /// Read-only facade over the alert manager + bubble store. The ONLY
    /// doctrinally-sanctioned access path to alert data
    /// (`docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A, Step 3.0).
    #[inline]
    pub fn alerts(&self) -> Alerts<'_> {
        // LOCK ORDER: bubble first, then alert.
        let bubbles = self.bubble.read().expect("BackendCore.bubble poisoned (read)");
        let mgr = self.alert.read().expect("BackendCore.alert poisoned (read)");
        Alerts::new(mgr, bubbles)
    }

    /// Mutable facade over the alert manager + bubble store.
    #[inline]
    pub fn alerts_mut(&self) -> AlertsMut<'_> {
        // LOCK ORDER: bubble first, then alert.
        let bubbles = self.bubble.write().expect("BackendCore.bubble poisoned (write)");
        let mgr = self.alert.write().expect("BackendCore.alert poisoned (write)");
        AlertsMut::new(mgr, bubbles)
    }

    /// Read-only facade over the trading manager. Returns `None` if trading
    /// is disabled. The ONLY doctrinally-sanctioned access path to trading
    /// data (doctrine 3.7c, `docs/mlc/plans/doctrine-cutover-2026-06-27.md`
    /// §1A). `TradingManager` never touches `BubbleStore` — single guard,
    /// no co-acquisition.
    #[inline]
    pub fn trading(&self) -> Option<Trading<'_>> {
        self.trading.as_ref().map(|m| Trading::new(m.lock().expect("BackendCore.trading poisoned")))
    }

    /// Mutable facade over the trading manager. Returns `None` if trading is
    /// disabled.
    #[inline]
    pub fn trading_mut(&self) -> Option<TradingMut<'_>> {
        self.trading.as_ref().map(|m| TradingMut::new(m.lock().expect("BackendCore.trading poisoned")))
    }

    /// Read-only facade over the exchange symbols catalogue. The ONLY
    /// doctrinally-sanctioned access path (Step 7,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A).
    #[inline]
    pub fn exchange_symbols(&self) -> ExchangeSymbols<'_> {
        ExchangeSymbols::new(self.exchange_symbols.read().expect("BackendCore.exchange_symbols poisoned (read)"))
    }

    /// Mutable facade over the exchange symbols catalogue.
    #[inline]
    pub fn exchange_symbols_mut(&self) -> ExchangeSymbolsMut<'_> {
        ExchangeSymbolsMut::new(self.exchange_symbols.write().expect("BackendCore.exchange_symbols poisoned (write)"))
    }

    /// Read-only facade over the preset manager. The ONLY doctrinally-
    /// sanctioned access path to preset data (Step 3.6d,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A).
    ///
    /// LOCK ORDER: preset is a leaf lock — no `bubble` co-acquisition needed.
    #[inline]
    pub fn presets(&self) -> Presets<'_> {
        Presets::new(self.preset.read().expect("BackendCore.preset poisoned (read)"))
    }

    /// Mutable facade over the preset manager.
    #[inline]
    pub fn presets_mut(&self) -> PresetsMut<'_> {
        PresetsMut::new(self.preset.write().expect("BackendCore.preset poisoned (write)"))
    }

    /// Read-only facade over the template manager. The ONLY doctrinally-
    /// sanctioned access path to template data (Step 3.6d,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A).
    ///
    /// LOCK ORDER: template is a leaf lock — no `bubble` co-acquisition needed.
    #[inline]
    pub fn templates(&self) -> Templates<'_> {
        Templates::new(self.template.read().expect("BackendCore.template poisoned (read)"))
    }

    /// Mutable facade over the template manager.
    #[inline]
    pub fn templates_mut(&self) -> TemplatesMut<'_> {
        TemplatesMut::new(self.template.write().expect("BackendCore.template poisoned (write)"))
    }

    /// Read-only facade over the user profile manager. The ONLY doctrinally-
    /// sanctioned access path to profile/snapshots/vault-key data (Step 3.6c,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B).
    ///
    /// LOCK ORDER: user is a leaf lock — no `bubble` co-acquisition needed.
    #[inline]
    pub fn user(&self) -> User<'_> {
        User::new(self.user.read().expect("BackendCore.user poisoned (read)"))
    }

    /// Mutable facade over the user profile manager.
    #[inline]
    pub fn user_mut(&self) -> UserMut<'_> {
        UserMut::new(self.user.write().expect("BackendCore.user poisoned (write)"))
    }

    /// Read-only facade over the watchlist manager. The ONLY doctrinally-
    /// sanctioned access path to watchlist data (Step 3.3,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A).
    ///
    /// LOCK ORDER: watchlist is a leaf lock — no `bubble` co-acquisition
    /// needed (WatchlistManager never touches DataBubble, design §1.2).
    #[inline]
    pub fn watchlists(&self) -> Watchlists<'_> {
        Watchlists::new(self.watchlist_manager.read().expect("BackendCore.watchlist_manager poisoned (read)"))
    }

    /// Mutable facade over the watchlist manager.
    #[inline]
    pub fn watchlists_mut(&self) -> WatchlistsMut<'_> {
        WatchlistsMut::new(self.watchlist_manager.write().expect("BackendCore.watchlist_manager poisoned (write)"))
    }

    /// Read-only facade over the tag manager. The ONLY doctrinally-
    /// sanctioned access path to tag data (Step 3.6b,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B).
    ///
    /// LOCK ORDER: tag is a leaf lock — no `bubble` co-acquisition needed
    /// (TagManager never touches DataBubble).
    #[inline]
    pub fn tags(&self) -> Tags<'_> {
        Tags::new(self.tag.read().expect("BackendCore.tag poisoned (read)"))
    }

    /// Mutable facade over the tag manager.
    #[inline]
    pub fn tags_mut(&self) -> TagsMut<'_> {
        TagsMut::new(self.tag.write().expect("BackendCore.tag poisoned (write)"))
    }

    /// Read-only facade over the drawing manager. The ONLY doctrinally-
    /// sanctioned access path (Step 3.1, `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A).
    ///
    /// LOCK ORDER: bubble first, then drawing.
    #[inline]
    pub fn drawings(&self) -> Drawings<'_> {
        let bubbles = self.bubble.read().expect("BackendCore.bubble poisoned (read)");
        let mgr = self.drawing.read().expect("BackendCore.drawing poisoned (read)");
        Drawings::new(mgr, bubbles)
    }

    /// Mutable facade over the drawing manager.
    ///
    /// LOCK ORDER: bubble first, then drawing.
    #[inline]
    pub fn drawings_mut(&self) -> DrawingsMut<'_> {
        let bubbles = self.bubble.write().expect("BackendCore.bubble poisoned (write)");
        let mgr = self.drawing.write().expect("BackendCore.drawing poisoned (write)");
        DrawingsMut::new(mgr, bubbles)
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
                    let bids_for_instrument = self.bubbles()
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
                            let mut mgr = self.bubbles_mut();
                            let tf_name = tf_name.clone();
                            // Raw kline layer (unified-data-plane Stage Э1):
                            // ADDITIVE write, UNCONDITIONAL (fixed Э3) —
                            // independent of `bars_source` AND of the
                            // bubble's own active TF, same rationale as the
                            // `BackfillComplete`/`ScrollBarsLoaded` arms. A
                            // bubble can hold an independent Э3 "1m"
                            // fold-feed kline sub whose Phase-A page must
                            // still reach `kline_layers["1m"]`.
                            //
                            // `seed_block` (not a `push_live` loop, fixed P0
                            // 2026-07-09): `BarsLoaded` is always the
                            // right-edge Phase-A page for a (re-)subscribe.
                            // A `RampState` that survived a chart-type/TF
                            // round-trip can keep extending this SAME
                            // `kline_layers[tf]` older-ward from an anchor
                            // that predates the fresh page by months — a
                            // plain per-bar `push_live` loop would leave that
                            // stale block sitting under the fresh one with a
                            // gap between them. `seed_block` detects that
                            // disjoint case and resets the layer to the
                            // fresh block instead.
                            mgr.kline_layer_seed_block(bid, &tf_name, bars.clone());
                            // bars-ownership + TF-identity gate for the
                            // DISPLAY write only: a footprint-owned bubble's
                            // bar series is written by `FootprintUpdate` —
                            // never here. A genuine kline Phase-A/backfill
                            // batch writes ONLY when `tf_name` matches the
                            // bubble's OWN active timeframe — the check that
                            // stops an independent "1m" fold-feed sub (Э3) on
                            // a renko/pnf/kagi/three_line_break bubble (still
                            // `bars_source == Kline`) from wholesale-
                            // replacing `bubble.bars` with 1m candles when
                            // the bubble's real active tf is something else
                            // entirely.
                            let display_writable = mgr.bars_source(bid) != crate::bubble::BarsSource::FootprintOhlc
                                && mgr.timeframe(bid).map(|tf| tf.name.as_str()) == Some(tf_name.as_str());
                            if !display_writable {
                                drop(mgr);
                                continue;
                            }
                            let prev_bars = mgr.bars(bid).to_vec();
                            // Display = a VIEW of the continuous layer, not a
                            // separate replace target (P0 fix 2026-07-09):
                            // rebuilding from the layer snapshot instead of
                            // the raw event page means a later
                            // `ScrollBarsLoaded` prepend (which only ever
                            // touches the layer) stays visible in
                            // `bubble.bars` without a second merge step. Falls
                            // back to the event's own bars when the layer
                            // read is empty (defensive only — `seed_block`
                            // just wrote into it above).
                            let display_bars = mgr.kline_layer_snapshot(bid, &tf_name);
                            let display_bars = if display_bars.is_empty() { bars.clone() } else { display_bars };
                            mgr.replace_bars(bid, display_bars.clone());
                            drop(mgr);
                            events.push(BubbleEvent::BarsLoaded {
                                bid,
                                exchange_id,
                                symbol: symbol.clone(),
                                account_type: account_type.short_label().to_string(),
                                timeframe: tf_name,
                                bars: display_bars,
                                prev_bars,
                            });
                        }
                    }
                }

                // ── BackfillComplete ────────────────────────────────────────────
                LiveUpdate::BackfillComplete { exchange_id, account_type, symbol, timeframe: tf_name, bars } => {
                    let bids = self.bubbles()
                        .bubbles_for_instrument(exchange_id.as_str(), account_type.short_label(), &symbol);
                    for bid in bids {
                        // Same rationale as BarsLoaded above: snapshot the OLD
                        // bubble.bars before overwrite so the reactor's
                        // capture_view_anchor sees real prior state.
                        let mut mgr = self.bubbles_mut();
                        // Raw kline layer (unified-data-plane Stage Э1, fixed
                        // Э3): ADDITIVE write, UNCONDITIONAL — independent of
                        // `bars_source` AND of the bubble's own active TF. A
                        // bubble can hold an independent Э3 "1m" fold-feed
                        // kline sub (range/volume/dollar/renko/pnf/kagi/
                        // three_line_break) whose `BackfillComplete` pages
                        // must reach `kline_layers["1m"]` even when the
                        // bubble's OWN active tf/chart_type is something else
                        // entirely — moved above both gates below.
                        //
                        // `seed_block` (fixed P0 2026-07-09): same rationale
                        // as the `BarsLoaded` arm — `BackfillComplete` is
                        // also a right-edge Phase-A-class page (the ladder's
                        // final page), not a scroll-left prepend, so a stale
                        // disjoint layer tail must reset rather than leave a
                        // gap under the fresh page.
                        mgr.kline_layer_seed_block(bid, &tf_name, bars.clone());
                        // bars-ownership + TF-identity gate for the DISPLAY
                        // write only: kline REST backfill on the bubble's OWN
                        // active timeframe is the only legitimate writer of
                        // `bubble.bars` here (the trade-agg family folds
                        // client-side via `TradeFoldRuntime`/`FoldRuntime`,
                        // unified-data-plane Э5b). The TF check (added Э3) is what stops
                        // an independent "1m" fold-feed sub on a renko/pnf/
                        // kagi/three_line_break bubble (still `bars_source ==
                        // Kline`, since those four never stamp `TradeAggBars`)
                        // from wholesale-replacing `bubble.bars` with 1m
                        // candles when the bubble's real active tf is, say,
                        // "1h" — `bars_source` alone does not encode "which
                        // TF this bubble is actually displaying".
                        if mgr.bars_source(bid) != crate::bubble::BarsSource::Kline
                            || mgr.timeframe(bid).map(|tf| tf.name.as_str()) != Some(tf_name.as_str())
                        {
                            continue;
                        }
                        let prev_bars = mgr.bars(bid).to_vec();
                        // Display = a VIEW of the continuous layer — see the
                        // matching comment on the `BarsLoaded` arm above.
                        let display_bars = mgr.kline_layer_snapshot(bid, &tf_name);
                        let display_bars = if display_bars.is_empty() { bars.clone() } else { display_bars };
                        mgr.replace_bars(bid, display_bars.clone());
                        drop(mgr);
                        events.push(BubbleEvent::BackfillCompleted {
                            bid,
                            exchange_id,
                            symbol: symbol.clone(),
                            account_type: account_type.short_label().to_string(),
                            timeframe: tf_name.clone(),
                            bars: display_bars,
                            prev_bars,
                        });
                    }
                }

                // ── ScrollBarsLoaded ────────────────────────────────────────────
                // The bridge's `request_scroll_bars` REST path returns ONLY the
                // older page (bars older than `before_ts`). Replacing bubble.bars
                // with that page wholesale would drop every live / right-edge
                // bar the chart has accumulated since seed. The raw kline layer
                // absorbs the page via `prepend_batch` (seam-deduped, sorted
                // oldest→newest); the DISPLAY array is then rebuilt from that
                // SAME layer (P0 fix 2026-07-09, see the arm body) instead of
                // running its own separate merge — a display-only merge could
                // drift from the layer's contiguous truth whenever the two
                // arrays' prior states diverged (the exact kline-display-
                // corruption mechanism this fix closes).
                LiveUpdate::ScrollBarsLoaded { exchange_id, account_type, symbol, timeframe: tf_name, bars, prepend_count } => {
                    let at_label = account_type.short_label().to_string();
                    let bids = self.bubbles()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, &symbol);
                    for bid in bids {
                        let mut mgr = self.bubbles_mut();
                        // Raw kline layer (unified-data-plane Stage Э1, fixed
                        // Э3): ADDITIVE prepend, UNCONDITIONAL — `ScrollBarsLoaded`
                        // only ever carries genuine kline data for `tf_name`,
                        // independent of which chart_type/`bars_source` this
                        // bubble's ACTIVE display currently owns. Moved above
                        // the bars-ownership gate below: a `TradeAggBars`- or
                        // `FootprintOhlc`-owned bubble can simultaneously hold
                        // an independent Э3 "1m" fold-feed kline sub, whose
                        // pages must still reach the raw layer even though
                        // the display write right below is correctly skipped
                        // for it.
                        mgr.kline_layer_prepend_batch(bid, &tf_name, bars.clone());
                        // bars-ownership + TF-identity gate for the DISPLAY
                        // write only (see the matching comment on
                        // `BackfillComplete` above for the full rationale):
                        // kline REST scroll on the bubble's OWN active
                        // timeframe is the only legitimate writer of
                        // `bubble.bars` here.
                        if mgr.bars_source(bid) != crate::bubble::BarsSource::Kline
                            || mgr.timeframe(bid).map(|tf| tf.name.as_str()) != Some(tf_name.as_str())
                        {
                            continue;
                        }
                        // Snapshot BEFORE the merge so the reactor's anchor
                        // capture can index `view_start` into the OLD (pre-
                        // prepend) coordinate space. See `ScrollBarsAppended`
                        // doc comment for why re-reading the bubble after
                        // the merge would collapse the anchor. `prepend_count`
                        // is untouched by the display-write change below — it
                        // is the engine's own count of bars prepended into
                        // the raw layer above (`kline_layer_prepend_batch`),
                        // forwarded straight from the `LiveUpdate` event, and
                        // the reactor never recomputes it from `bars.len()`.
                        let prev_bars = mgr.bars(bid).to_vec();
                        // Display = a VIEW of the continuous layer (P0 fix
                        // 2026-07-09): the layer write above already merged
                        // this page in with correct seam dedup; rebuild
                        // `bubble.bars` from that SAME layer instead of a
                        // second, display-only `merge_scroll_bars` pass that
                        // only ever saw the display array's own (potentially
                        // already-corrupted) prior state. Falls back to the
                        // event's own bars when the layer read is empty
                        // (defensive only — the prepend above just wrote
                        // into it, so this only triggers if the bubble
                        // itself vanished between the two facade calls,
                        // which `kline_layer_snapshot`/`replace_bars` both
                        // already no-op on).
                        let display_bars = mgr.kline_layer_snapshot(bid, &tf_name);
                        let display_bars = if display_bars.is_empty() { bars.clone() } else { display_bars };
                        mgr.replace_bars(bid, display_bars.clone());
                        drop(mgr);
                        events.push(BubbleEvent::ScrollBarsAppended {
                            bid,
                            exchange_id,
                            symbol: symbol.clone(),
                            account_type: at_label.clone(),
                            timeframe: tf_name.clone(),
                            bars: display_bars,
                            prev_bars,
                            prepend_count,
                        });
                    }
                }

                // ── BarUpdate — pass-through ────────────────────────────────────
                // Bubble write depends on per-window `is_derived` / `chart_type` flags;
                // reactor owns the full arm.
                LiveUpdate::BarUpdate { exchange_id, account_type: bar_at, symbol: bar_sym, timeframe: bar_tf_name, bar } => {
                    // Raw kline layer (unified-data-plane Stage Э1, keying
                    // fixed in Э3): ADDITIVE live-tick write, independent of
                    // `bars_source`/chart_type — every bubble on this
                    // instrument gets the tick written into the layer named
                    // by the EVENT's OWN `timeframe` field, not whichever TF
                    // the bubble's `timeframe` happens to be set to right
                    // now. A bubble can hold an independent "1m" fold-feed
                    // subscription (Э3 FoldRuntime) while its own `timeframe`
                    // is a different wall-clock TF — keying off the bubble's
                    // own field here would have silently written "1m" live
                    // ticks into e.g. the "1h" layer.
                    if !bar_tf_name.is_empty() {
                        let at_label = bar_at.short_label();
                        // NOTE: bind the bid list BEFORE the loop — a read
                        // guard living in the `for` scrutinee stays alive for
                        // the whole loop body, and `bubbles_mut()` inside
                        // would re-enter the RwLock (wasm: instant abort).
                        let layer_bids: Vec<_> = self.bubbles().bubbles_for_instrument(exchange_id.as_str(), at_label, &bar_sym);
                        for bid in layer_bids {
                            self.bubbles_mut().kline_layer_push_live(bid, &bar_tf_name, bar);
                            // Client-side view fold (unified-data-plane Э3):
                            // every bubble that has installed a FoldRuntime
                            // for one of the 7 kline-sufficient chart types
                            // folds this SAME "1m" tick incrementally — the
                            // fold only ever consumes the "1m" layer (see
                            // `bubble::view_fold` module doc), so other TFs'
                            // live ticks are a no-op here (`fold_on_live_1m`
                            // itself is a no-op when `active_fold` is `None`,
                            // but the TF gate avoids feeding an installed
                            // "1m" fold a tick from an unrelated TF sub on
                            // the same bubble).
                            if bar_tf_name == "1m" {
                                if let Some(ev) = self.fold_on_live_1m(bid, bar, exchange_id, &bar_sym, at_label) {
                                    events.push(ev);
                                }
                                // Trade-fed fold family (Э4): only the TPO
                                // Kline1m flavour consumes "1m" bars — every
                                // other trade-fed runtime's `on_live_1m` is a
                                // documented no-op (see
                                // `TradeFoldRuntime::on_live_1m`).
                                if let Some(ev) = self.trade_fold_on_live_1m(bid, bar, exchange_id, &bar_sym, at_label) {
                                    events.push(ev);
                                }
                            }
                        }
                    }
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
                    // Raw trade layer (unified-data-plane Stage Э1): ADDITIVE
                    // live-tick write into EVERY bubble on this instrument —
                    // single-writer-core, backend-side (not mlc-app), and
                    // unconditional (not gated by chart_type/TF, unlike the
                    // kline-candle-builder path in react.rs's
                    // `TradeUpdatePassthrough` arm, which keeps ALL its
                    // existing gates).
                    let at_label = account_type.short_label();
                    let raw_trade = crate::bubble::RawTrade {
                        ts_ms: timestamp,
                        price,
                        qty: quantity,
                        is_buyer_maker,
                    };
                    // Guard-lifetime note: bind before looping (see BarUpdate arm).
                    let trade_bids: Vec<_> = self.bubbles().bubbles_for_instrument(exchange_id.as_str(), at_label, &symbol);
                    for bid in trade_bids {
                        self.bubbles_mut().trade_layer_push_live(bid, raw_trade);
                        // Client-side trade fold (unified-data-plane Э4):
                        // every bubble that has installed a `TradeFoldRuntime`
                        // for one of the 6 trade-fed chart types folds this
                        // SAME live trade incrementally — no-op when
                        // `active_trade_fold` is `None` (not a trade-fed
                        // chart_type) or the runtime is the TPO Kline1m
                        // flavour (folds `Event::Bar`, not `Event::Trade` —
                        // see `TradeFoldRuntime::on_live_trade`'s no-op
                        // guard). Independent of the CVD companion fold
                        // below — a bubble can have BOTH installed at once.
                        if let Some(ev) = self.trade_fold_on_live_trade(bid, raw_trade, exchange_id, &symbol, at_label) {
                            events.push(ev);
                        }
                        // CVD companion fold: independent slot, independent
                        // lifecycle (installed by `subscribe_cvd_fold` when
                        // a CVD indicator instance is added, not by a
                        // chart_type switch) — see `DataBubble.active_cvd_fold`
                        // doc comment for why it isn't merged into
                        // `active_trade_fold`.
                        if let Some(ev) = self.cvd_fold_on_live_trade(bid, raw_trade, exchange_id, &symbol, at_label) {
                            events.push(ev);
                        }
                    }
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

                // ── TradeHistoryPrepended (unified-data-plane Э2) ────────────────
                // `BackfillEngine`'s trade-layer backfill page — ADDITIVE prepend
                // into every bubble on this instrument's raw trade layer. No
                // active-series write (trades do not drive `bubble.bars`
                // directly today); Э4's client-side folds are the intended
                // future active-series consumer of this data.
                LiveUpdate::TradeHistoryPrepended { exchange_id, account_type, symbol, trades } => {
                    if trades.is_empty() { continue; }
                    let at_label = account_type.short_label();
                    let raw_trades: Vec<crate::bubble::RawTrade> = trades.iter().map(|t| crate::bubble::RawTrade {
                        ts_ms: t.ts_ms,
                        price: t.price,
                        qty: t.qty,
                        is_buyer_maker: t.is_buyer_maker,
                    }).collect();
                    // Guard-lifetime note: bind before looping (see BarUpdate arm).
                    let prepend_bids: Vec<_> = self.bubbles().bubbles_for_instrument(exchange_id.as_str(), at_label, &symbol);
                    for bid in prepend_bids {
                        self.bubbles_mut().trade_layer_prepend_batch(bid, raw_trades.clone());
                    }
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
                    self.exchange_symbols_mut().set_symbols(exchange_id, symbols);
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

                // ── LayerBackfilled (unified-data-plane Э2, consumed Э3+Э4) ──────
                // The actual `kline_layers`/`trade_layer` write already
                // happened via the `ScrollBarsLoaded`/`BackfillComplete`/
                // `TradeHistoryPrepended` arms above — this is the
                // layer-scoped SIGNAL. For `layer == "kline:1m"`, every
                // bubble on this instrument that has installed a `FoldRuntime`
                // (one of the 7 kline-sufficient chart types) re-folds from
                // scratch over the now-deeper layer and replaces its output
                // slot wholesale — cheap in-memory recompute, no network.
                // `layer == "trades"` (Э4): same in-place refold, but for
                // `active_trade_fold` (6 trade-fed chart types) AND
                // `active_cvd_fold` (CVD companion) — a bubble may have
                // BOTH installed at once.
                LiveUpdate::LayerBackfilled { exchange_id, symbol, account_type, layer, exhausted, .. } => {
                    if layer == "kline:1m" {
                        let at_label = account_type.short_label();
                        // Guard-lifetime note: bind before looping (see BarUpdate arm).
                        let fold_bids: Vec<_> = self.bubbles().bubbles_for_instrument(exchange_id.as_str(), at_label, &symbol);
                        for bid in fold_bids {
                            let has_fold = self.bubbles().with_bubble(bid, |b| b.active_fold.is_some()).unwrap_or(false);
                            if !has_fold { continue; }
                            let layer_snapshot: crate::bubble::RawLayer<Bar> = match self.bubbles().kline_layer(bid, "1m") {
                                Some(l) => l.clone(),
                                None => continue,
                            };
                            let outputs_and_type = self.bubbles_mut().with_bubble_mut(bid, |b| {
                                b.active_fold.as_mut().map(|f| (f.reseed(&layer_snapshot), f.chart_type()))
                            }).flatten();
                            let Some((outputs, chart_type)) = outputs_and_type else { continue; };
                            if let Some(ev) = self.apply_fold_reseed_outputs(bid, chart_type, outputs, exchange_id, &symbol, at_label) {
                                events.push(ev);
                            }
                            // History-depth badge (venue exhaustion / wall):
                            // the "1m" layer ramp has stopped for good — the
                            // active fold's history is as deep as it will
                            // ever get. Mirrors the station-derived path's
                            // own `HistoryDepthWire::Capped` report.
                            if exhausted {
                                let achieved = layer_snapshot.len();
                                if self.bubbles_mut().set_history_depth(bid, chart_type, crate::bubble::HistoryDepth::Capped { achieved }) {
                                    events.push(BubbleEvent::HistoryDepthReported {
                                        bid, exchange_id, symbol: symbol.clone(), account_type: at_label.to_string(),
                                        chart_type, depth: crate::bubble::HistoryDepth::Capped { achieved },
                                    });
                                }
                            }
                        }
                    } else if layer == "trades" {
                        let at_label = account_type.short_label();
                        // Guard-lifetime note: bind before looping (see BarUpdate arm).
                        let tfold_bids: Vec<_> = self.bubbles().bubbles_for_instrument(exchange_id.as_str(), at_label, &symbol);
                        for bid in tfold_bids {
                            let trade_snapshot: crate::bubble::RawLayer<crate::bubble::RawTrade> = match self.bubbles().trade_layer(bid) {
                                Some(l) => l.clone(),
                                None => continue,
                            };
                            let empty_kline: crate::bubble::RawLayer<Bar> = crate::bubble::RawLayer::new(0);

                            let has_trade_fold = self.bubbles().with_bubble(bid, |b| b.active_trade_fold.is_some()).unwrap_or(false);
                            if has_trade_fold {
                                let outputs_and_type = self.bubbles_mut().with_bubble_mut(bid, |b| {
                                    b.active_trade_fold.as_mut().map(|f| (f.reseed(&trade_snapshot, &empty_kline), f.chart_type()))
                                }).flatten();
                                if let Some((outputs, chart_type)) = outputs_and_type {
                                    if let Some(ev) = self.apply_trade_fold_reseed_outputs(bid, chart_type, outputs, exchange_id, &symbol, at_label) {
                                        events.push(ev);
                                    }
                                    if exhausted {
                                        let achieved = trade_snapshot.len();
                                        if self.bubbles_mut().set_history_depth(bid, chart_type, crate::bubble::HistoryDepth::Capped { achieved }) {
                                            events.push(BubbleEvent::HistoryDepthReported {
                                                bid, exchange_id, symbol: symbol.clone(), account_type: at_label.to_string(),
                                                chart_type, depth: crate::bubble::HistoryDepth::Capped { achieved },
                                            });
                                        }
                                    }
                                }
                            }

                            let has_cvd_fold = self.bubbles().with_bubble(bid, |b| b.active_cvd_fold.is_some()).unwrap_or(false);
                            if has_cvd_fold {
                                let outputs = self.bubbles_mut().with_bubble_mut(bid, |b| {
                                    b.active_cvd_fold.as_mut().map(|f| f.reseed(&trade_snapshot, &empty_kline))
                                }).flatten();
                                if let Some(outputs) = outputs {
                                    if let Some(ev) = self.apply_trade_fold_reseed_outputs(bid, "cvd", outputs, exchange_id, &symbol, at_label) {
                                        events.push(ev);
                                    }
                                }
                            }
                        }
                    }
                }

                // ── Trading (Order / Balance / Position) ─────────────────────────
                LiveUpdate::OrderUpdate { .. }
                | LiveUpdate::BalanceUpdate { .. }
                | LiveUpdate::PositionUpdate { .. } => {
                    events.push(BubbleEvent::TradingUpdate { update });
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
                // Write target depends on per-app `dom_heatmap_show` overlay flag.
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
                // Write target depends on per-app `dom_heatmap_show` overlay flag — pass through.
                LiveUpdate::DomTradeDot { exchange_id, account_type: at, symbol: sym, dot } => {
                    events.push(BubbleEvent::DomTradeDotPassthrough {
                        exchange_id,
                        account_type: at.short_label().to_string(),
                        symbol: sym,
                        dot,
                    });
                }

                // ── LiquidationEvent — pass-through ─────────────────────────────
                // Write target depends on per-app `liq_heatmap_show` overlay flag.
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
                // Write target depends on per-app `liq_heatmap_show` overlay flag.
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

                // ── HistoryDepthReported ───────────────────────────────────────
                // Bubble-storage write (hybrid-data-modes-plan-2026-07-08,
                // Step 1) — `DataBubble::set_history_depth` (via the facade)
                // rejects the write on its own if `chart_type` no longer
                // matches the bubble's active type (stale event from a
                // just-switched-away subscribe). ONE `BubbleEvent` per
                // written bubble so the per-app reactor can clear a stuck
                // `pending_symbol_load` (Step 4 — eternal-skeleton fix).
                LiveUpdate::HistoryDepthReported { exchange_id, account_type: at, symbol: sym, chart_type, depth } => {
                    let at_label = at.short_label().to_string();
                    let depth = match depth {
                        mlc_backend_data_bridge::HistoryDepthWire::Full => crate::bubble::HistoryDepth::Full,
                        mlc_backend_data_bridge::HistoryDepthWire::Capped { achieved } => crate::bubble::HistoryDepth::Capped { achieved },
                        mlc_backend_data_bridge::HistoryDepthWire::LiveOnly => crate::bubble::HistoryDepth::LiveOnly,
                    };
                    let bids = self.bubbles()
                        .bubbles_for_instrument(exchange_id.as_str(), &at_label, sym.as_str());
                    for bid in bids {
                        if self.bubbles_mut().set_history_depth(bid, chart_type, depth) {
                            events.push(BubbleEvent::HistoryDepthReported {
                                bid,
                                exchange_id,
                                symbol: sym.clone(),
                                account_type: at_label.clone(),
                                chart_type,
                                depth,
                            });
                        }
                    }
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

// ── Chart data-plane facade methods (Step 11 of chart-type registry arc) ──
// These wrap DataBridge calls that are NOT chart-type dispatch (which goes
// through mlc_backend::chart_type::engine::SubscribeOps). One method per
// distinct data-plane operation used by mlc-app / mlc-app-shell-vello callers.
//
// Step 11 adds these methods only — no call site is touched here. Step 12
// repoints `self.core.bridge.*` / `self.bridge.*` call sites at the matching
// method below and closes the field to private; the handful of remaining
// direct-bridge callers (chart-type dispatch + methods with no facade
// wrapper) go through `BackendCore::bridge_handle()` / `bridge_arc()` instead.
impl BackendCore {
    /// Fetch (or refresh) a page of kline bars for an instrument + timeframe.
    ///
    /// Thin wrapper over [`DataBridge::request_bars`] — parameter names kept
    /// as `_limit`/`_total_bars` to match the underlying signature verbatim
    /// (the leading underscores are the bridge's own naming, not a typo).
    pub fn request_kline(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        tf: &Timeframe,
        account_type: AccountType,
        limit: Option<u16>,
        total_bars: Option<usize>,
        force: bool,
    ) {
        self.bridge.request_bars(exchange_id, symbol, tf, account_type, limit, total_bars, force);
    }

    /// Abort the active kline forwarder for an instrument + timeframe.
    pub fn abort_kline_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        timeframe: &str,
    ) {
        self.bridge.abort_kline_bars(exchange_id, symbol, account_type, timeframe);
    }

    /// Fetch the next older page of bars for scroll-left backfill.
    ///
    /// Unified-data-plane Э2: routed through `BackfillEngine`'s scroll-
    /// pressure entry (`on_kline_scroll_pressure`) instead of the legacy
    /// one-page-per-call `request_scroll_bars`. `before_ts`/`batch_size` are
    /// no longer threaded through — the engine tracks its own
    /// `oldest_anchor` per key and ramps the group size itself; the caller
    /// (`tick.rs`'s scroll-threshold site) only needs to signal "pressure
    /// now", which is what a scroll-left crossing means.
    pub fn request_scroll_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        tf: &Timeframe,
        account_type: AccountType,
        _before_ts: i64,
        _batch_size: usize,
    ) {
        self.bridge.on_kline_scroll_pressure(exchange_id, symbol, tf, account_type);
    }

    /// Paginate the connector's REST history toward `target_bars` in the
    /// background (unified-data-plane Э2: `BackfillEngine::ensure_kline_backfill`
    /// — progressive ramp 1→1→2→4→… instead of the legacy one-shot 40-page
    /// ladder emitted as a single `BackfillComplete`).
    pub fn request_background_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        tf: &Timeframe,
        account_type: AccountType,
        target_bars: u32,
    ) {
        self.bridge.ensure_kline_backfill(exchange_id, symbol, tf, account_type, Some(target_bars));
    }

    /// Subscribe to the Open Interest overlay stream for an instrument.
    pub fn subscribe_open_interest(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.request_open_interest(exchange_id, symbol, account_type);
    }

    /// Cancel the Open Interest overlay subscription for an instrument.
    pub fn unsubscribe_open_interest(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_open_interest(exchange_id, symbol, account_type);
    }

    /// Subscribe to the Funding Rate overlay stream for an instrument.
    pub fn subscribe_funding_rate(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.request_funding_rate(exchange_id, symbol, account_type);
    }

    /// Cancel the Funding Rate overlay subscription for an instrument.
    pub fn unsubscribe_funding_rate(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_funding_rate(exchange_id, symbol, account_type);
    }

    /// Subscribe to the DOM Heatmap overlay stream for an instrument
    /// (chart-type → overlay migration — was a chart_type subscribe).
    pub fn subscribe_dom_heatmap(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.request_dom_heatmap(exchange_id, symbol, account_type);
    }

    /// Cancel the DOM Heatmap overlay subscription for an instrument.
    pub fn unsubscribe_dom_heatmap(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_dom_heatmap(exchange_id, symbol, account_type);
    }

    /// Subscribe to the Liquidation Heatmap overlay stream for an instrument
    /// (chart-type → overlay migration — was a chart_type subscribe).
    pub fn subscribe_liquidation_heatmap(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.request_liquidation_heatmap(exchange_id, symbol, account_type);
    }

    /// Cancel the Liquidation Heatmap overlay subscription for an instrument.
    pub fn unsubscribe_liquidation_heatmap(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_liquidation_heatmap(exchange_id, symbol, account_type);
    }

    /// Ensure a connector for `exchange_id` is connected, spawning it if needed.
    pub fn ensure_connector(&self, exchange_id: ExchangeId) {
        self.bridge.ensure_connector(exchange_id);
    }

    /// Subscribe to the live depth (orderbook delta) stream for an instrument.
    pub fn subscribe_depth(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.subscribe_depth(exchange_id, symbol, account_type);
    }

    /// Remove one consumer interest in the depth stream for an instrument.
    pub fn unsubscribe_depth(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_depth(exchange_id, symbol, account_type);
    }

    /// Remove one consumer interest in the trade stream for an instrument.
    pub fn unsubscribe_trades(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_trades(exchange_id, symbol, account_type);
    }

    /// Remove one consumer interest in the orderbook stream for an instrument.
    pub fn unsubscribe_orderbook(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_orderbook(exchange_id, symbol, account_type);
    }

    /// Unsubscribe a single mini-ticker symbol (e.g. removed from watchlist).
    pub fn unsubscribe_mini_ticker(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        self.bridge.unsubscribe_mini_ticker(exchange_id, symbol, account_type);
    }

    /// Fetch the tradable symbol list for `exchange_id` (cache hit or async REST fetch).
    ///
    /// Cold-boot bootstrap call, made before a `ChartApp` exists (shell-vello
    /// calls this directly on its own `Arc<BackendCore>`); functionally
    /// identical to a plain data-plane symbol request, no `ChartApp` state
    /// involved.
    pub fn bootstrap_request_symbols(&self, exchange_id: ExchangeId) {
        self.bridge.request_symbols(exchange_id);
    }

    /// Read-only snapshot of per-connector WebSocket/REST metrics.
    pub fn agent_metrics_snapshot(&self) -> Vec<(ExchangeId, digdigdig3::core::types::ConnectorStats, usize)> {
        self.bridge.collect_metrics()
    }

    /// Whether deep trade history is available for `(exchange_id,
    /// account_type)` — true when the venue's live REST tier already reaches
    /// back arbitrarily far (`RestDeep`) or far-but-bounded (`RestWindow`),
    /// **or** the venue publishes a bulk daily trade-archive the backfill
    /// engine can chain into once the live tier bottoms out (`ArchiveSpec`
    /// via `TradeArchiveCapabilities::for_account`). `RecentOnly` with no
    /// archive is the only case that resolves `false` — the sole tier with
    /// no path to deep history at all.
    ///
    /// Single source of truth for "does this venue have trade depth" —
    /// every consumer that used to read `TradeHistoryTier` bare (dropdown
    /// marks, footprint hybrid/takeover switch) goes through this instead,
    /// so an archive-backed `RecentOnly` venue (Bybit spot) is treated
    /// identically to a `RestDeep` one. `None` (exchange not connected yet)
    /// propagates capability-unknown to the caller rather than guessing.
    ///
    /// Thin wrapper over [`crate::chart_type::deep_trade_history_available`]
    /// (same join, `Dig3AccountType` there vs `mlc_core::instrument::
    /// AccountType` here — this facade does the conversion) — that sibling
    /// is what [`crate::chart_type::is_footprint_hybrid`] consults, so both
    /// entry points share one implementation.
    ///
    /// `nemo/docs/dig3/plans/filedump-trade-archives-2026-07-09.md` §Ф3.
    pub fn deep_trade_history_available(
        &self,
        exchange_id: ExchangeId,
        account_type: mlc_core::instrument::AccountType,
    ) -> Option<bool> {
        let dig3_at = mlc_backend_data_bridge::account_type_to_dig3(account_type);
        crate::chart_type::deep_trade_history_available(&self.bridge, exchange_id, dig3_at)
    }

    /// Whether `chart_type` would run in hybrid/live-only mode on
    /// `(exchange_id, account_type)` — resolved from `ChartTypeDef::
    /// data_requirement` + [`Self::deep_trade_history_available`]. `None`
    /// exchange-not-connected or an unrecognized `chart_type` id resolves to
    /// `Unknown` (dropdown shows no marker rather than a wrong one).
    ///
    /// mlc-core stays capability-blind: this is the ONE call site that joins
    /// the static per-chart_type classification (`mlc-core`) with the
    /// runtime per-venue capability (`digdigdig3`, via `DataBridge`) — see
    /// `nemo/docs/mlc/plans/hybrid-data-modes-plan-2026-07-08.md` Step 5.
    ///
    /// `Klines` types (candle-family, renko/pnf/kagi/tlb, tpo, and — since
    /// dig3 0.3.29 — range_bar/volume_bar/dollar_bar) are trade-tier-blind:
    /// their history comes from kline REST pagination, not
    /// `TradeHistoryCapabilities.spot/futures`. The only capability that can
    /// degrade them is `kline_backpage` (Kraken/HTX/Gemini honestly cap kline
    /// backward pagination to one page) — no `DataMode` variant exists for
    /// that yet, so (matching every other Klines type today) it resolves to
    /// `Full` here; the actual cap surfaces per-bubble via dig3's
    /// `SeedOutcome` → `HistoryDepth::Capped` (`tick.rs`/`overlays.rs`), not
    /// through this dropdown-marking facade.
    pub fn data_mode_for(
        &self,
        chart_type: &str,
        exchange_id: ExchangeId,
        account_type: mlc_core::instrument::AccountType,
    ) -> mlc_core::chart_type::DataMode {
        use mlc_core::chart_type::{DataMode, DataRequirement};

        let Some(def) = mlc_core::chart_type::by_id(chart_type) else {
            return DataMode::Unknown;
        };
        match def.data_requirement {
            DataRequirement::Klines => DataMode::Full,
            DataRequirement::DeepTrades => {
                let Some(deep) = self.deep_trade_history_available(exchange_id, account_type) else {
                    return DataMode::Unknown;
                };
                match (deep, chart_type) {
                    (true, _) => DataMode::Full,
                    (false, "footprint") => DataMode::Hybrid,
                    (false, _) => DataMode::LiveOnly,
                }
            }
        }
    }

    /// Whether `interval` (canonical dig3 form — `"1m"`, `"1h"`, `"1M"`, see
    /// `mlc_app::active_series::tf_id_to_dig3_interval` for the id→canonical
    /// mapping) is natively served by `(exchange_id, account_type)`'s kline
    /// WS/REST — `None` when the exchange isn't connected yet (capability
    /// unknown; caller renders no marker rather than a wrong one).
    ///
    /// Consumed by the TF/bucket dropdown's per-item "(no history)" marking
    /// (`mlc-app::ChartApp::timeframe_native_map`,
    /// `nemo/docs/mlc/plans/resolution-availability-2026-07-08.md` Phase 3):
    /// a non-native interval is still selectable (the station falls back to
    /// live trade aggregation, `TradeToBarDerived`) but carries no REST
    /// backfill — client-only doctrine forbids claiming history that doesn't
    /// exist.
    pub fn kline_interval_native(
        &self,
        interval: &str,
        exchange_id: ExchangeId,
        account_type: mlc_core::instrument::AccountType,
    ) -> Option<bool> {
        let caps = self.bridge.kline_interval_capabilities(exchange_id)?;
        let dig3_at = mlc_backend_data_bridge::account_type_to_dig3(account_type);
        Some(caps.supports(interval, dig3_at))
    }

    /// Subscribe to the live trade stream for an instrument, returning the
    /// station's shared trade series handle for direct reads (trade tape,
    /// big-trades, volume-profile panels).
    ///
    /// Thin wrapper over [`DataBridge::subscribe_trades`] — kept as a
    /// pass-through `Option` (not unwrapped) because the bridge itself
    /// returns `None` on the rare path where the subscribe future hasn't
    /// completed yet; callers already handle that case.
    pub fn subscribe_trades(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<SharedTradeSeries> {
        let handle = self.bridge.subscribe_trades(exchange_id, symbol, account_type);
        // Unified-data-plane Э2: warm the raw trade layer with one shallow
        // page immediately on activation so the layer has depth from the
        // first frame, instead of only accumulating from this point forward
        // via live ticks. Unbounded target (`None`) — trades default to
        // "no fixed target, ramp until scroll stops" per owner doctrine;
        // this call only seeds the FIRST page (single-flight guard makes it
        // a no-op if a backfill for this key is already running).
        self.bridge.ensure_trade_backfill(exchange_id, symbol, account_type, None);
        handle
    }

    /// Subscribe to the live orderbook for an instrument, returning the
    /// shared `OrderBookTracker` handle for direct reads (DOM, heatmap,
    /// L2 tape, paper-engine BBO).
    ///
    /// Thin wrapper over [`DataBridge::subscribe_orderbook`].
    pub fn subscribe_orderbook(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<SharedOrderBookTracker> {
        self.bridge.subscribe_orderbook(exchange_id, symbol, account_type)
    }

    /// Narrow escape hatch to the raw `DataBridge` for the handful of
    /// legitimate direct callers that predate (or fall outside) the facade:
    /// the chart-type registry's `SubscribeOps`/`AbortOps` dispatch (needs a
    /// `&DataBridge` to hand to per-type function pointers), and a few
    /// bridge methods with no facade wrapper (`subscribe_mini_ticker`,
    /// `ws_task_count_total`, `pool()`, `is_kline_alive`, `request_footprint`,
    /// `abort_derived_chart`, `request_deeper_seed_for_active`,
    /// `enable_connector`/`disable_connector`, `warmup_all_connectors`).
    ///
    /// Do not add new call sites here for methods that already have (or
    /// could trivially get) a facade wrapper above — this exists to avoid
    /// re-litigating every bridge method into a 1:1 facade forwarder, not to
    /// route around the facade.
    pub fn bridge_handle(&self) -> &DataBridge {
        &self.bridge
    }

    /// Owned `Arc<DataBridge>` handle, for the construction sites that need
    /// to store their own clone rather than borrow through `bridge_handle()`
    /// — `LiveDataProvider::new` (per-window data provider construction) and
    /// `ChartApp::new_window` / shell process-spawn plumbing, which predate
    /// the shared-`BackendCore` arc and still take `Arc<DataBridge>` as a
    /// constructor parameter in their own right.
    pub fn bridge_arc(&self) -> Arc<DataBridge> {
        self.bridge.clone()
    }

    // =========================================================================
    // === Client-side view fold (unified-data-plane Stage Э3, 2026-07-09) ====
    // =========================================================================

    /// Install (or replace) `bid`'s [`bubble::FoldRuntime`] for one of the 7
    /// kline-sufficient chart types, ensure the bubble's shared "1m" kline
    /// layer is network-fed, and reseed synchronously from whatever the
    /// layer already holds (instant if warm, empty if this is the very
    /// first touch and the async backfill hasn't landed a page yet).
    ///
    /// Entirely synchronous except the "ensure network feed" step (fire-and-
    /// forget `request_bars`/`ensure_kline_backfill`, same async machinery
    /// every other chart type already uses for its OWN kline sub — no new
    /// network primitive). The reseed itself touches zero network: it folds
    /// whatever `kline_layers["1m"]` already contains in memory.
    ///
    /// Returns the [`BubbleEvent`] the caller should feed through
    /// `ChartApp::react_to_bubble_event` to run the same viewport reconcile
    /// (`seed_fresh_load`/`reconcile_derived_series`) a station-driven
    /// `tick_writers` event would have triggered — `None` when `bid` doesn't
    /// resolve to a live bubble.
    pub fn subscribe_fold_chart_type(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        params: crate::bubble::FoldParams,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        derived_kline_target: Option<u32>,
    ) -> Option<BubbleEvent> {
        let one_minute = Timeframe::new("1m", 60_000);
        // Live feed: idempotent per (eid, at, symbol, "1m") — a cheap no-op
        // if another window already opened this exact kline sub. Kept ahead
        // of the reseed/install below (unlike `ensure_kline_backfill`, this
        // has no refold-race — the live-tick writer path independently
        // checks `active_fold.is_some()` per event, it never sweeps a
        // point-in-time bubble list the way `LayerBackfilled` does).
        self.bridge.request_bars(exchange_id, symbol, &one_minute, account_type, None, None, false);

        // Fix 2 (chart-type-engine-consolidation B1): install BEFORE
        // ensure — build + reseed the fold runtime over whatever the "1m"
        // layer holds RIGHT NOW, install it into the bubble, and only THEN
        // fire the async ramped backfill. This closes the install-order
        // race: `LiveUpdate::LayerBackfilled`'s refold sweep
        // (`BackendCore::tick_writers`) only re-folds bubbles that already
        // have `active_fold.is_some()` — if the backfill's first page had
        // landed and broadcast BEFORE `active_fold` was set, that sweep
        // would silently skip this bubble and the fresh page would never
        // reach it. By the time `ensure_kline_backfill` returns (it merely
        // enqueues a background task — the fetch itself is async), the
        // install below has already completed synchronously, so no
        // `LayerBackfilled` for this key can possibly land in between.
        let fold = crate::bubble::FoldRuntime::new(chart_type, params, exchange_id, symbol)?;
        let event = self.reseed_fold(bid, fold, exchange_id, symbol, account_type);

        // History: ramped backfill toward `derived_kline_target` (profile-
        // configurable, default handled by the caller) — progressive, every
        // page lands via the existing `ScrollBarsLoaded`/`BackfillComplete`
        // + `LayerBackfilled` writers already wired in `tick_writers`.
        self.bridge.ensure_kline_backfill(exchange_id, symbol, &one_minute, account_type, derived_kline_target);

        event
    }

    /// Re-seed `bid`'s existing (or freshly-installed) fold runtime from the
    /// bubble's current "1m" kline layer snapshot, replace the output slot
    /// wholesale, and return the reconcile-triggering [`BubbleEvent`].
    fn reseed_fold(
        &self,
        bid: BubbleId,
        mut fold: crate::bubble::FoldRuntime,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<BubbleEvent> {
        let layer_snapshot: crate::bubble::RawLayer<Bar> = self.bubbles()
            .kline_layer(bid, "1m")
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let outputs = fold.reseed(&layer_snapshot);
        let chart_type = fold.chart_type();
        self.bubbles_mut().with_bubble_mut(bid, |b| b.active_fold = Some(fold));
        let account_type_label = account_type.short_label().to_string();
        self.apply_fold_reseed_outputs(bid, chart_type, outputs, exchange_id, symbol, &account_type_label)
    }

    /// Replace `bid`'s fold output slot wholesale with `outputs` (a full
    /// reseed's worth of points, oldest-first) and build the matching
    /// reconcile-triggering [`BubbleEvent`]. Shared by
    /// [`Self::reseed_fold`] (install/param-change/chart-type-switch path)
    /// and the `LiveUpdate::LayerBackfilled` arm in [`Self::tick_writers`]
    /// (scroll-deepen refold path) — both are "the layer got deeper, redo
    /// the full fold" operations that differ only in WHEN they fire, not in
    /// how the result lands.
    fn apply_fold_reseed_outputs(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        outputs: Vec<crate::bubble::FoldOutput>,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();
        // Identity-key mapping is registry-driven (chart-type-engine-
        // consolidation B3, S6) — `def.identity` selects the merge key fn
        // via the shape's own `*_identity_key_fn` resolver instead of an
        // arm hardcoding "renko → renko_identity" as tribal per-arm
        // knowledge. The output-SHAPE match below (which `FoldOutput`
        // variant this chart_type's machine actually produces) necessarily
        // stays — that's data shape, not policy.
        let identity = mlc_core::chart_type::by_id(chart_type).map(|d| d.identity).unwrap_or(mlc_core::chart_type::IdentityKey::None);
        let event = match chart_type {
            "range_bar" | "volume_bar" | "dollar_bar" => {
                // The fold machine emits a snapshot on every consumed 1m
                // kline (in-progress bar growing), not just on bar-close. A
                // reseed replay must collapse same-identity snapshots to the
                // last one or the seed array keeps every intermediate as a
                // separate bar. Identity mirrors `fold_on_live_1m`'s
                // `last_ts_identity` rule exactly.
                let key_fn = mlc_core::chart_type::bar_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::bar_identity);
                let mut bars: Vec<Bar> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Bar(b) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bars, b, key_fn);
                }
                let mut mgr = self.bubbles_mut();
                let prev_bars = mgr.bars(bid).to_vec();
                let tf_name = mgr.timeframe(bid).map(|tf| tf.name.clone()).unwrap_or_default();
                mgr.replace_bars(bid, bars.clone());
                drop(mgr);
                BubbleEvent::BarsLoaded {
                    bid, exchange_id, symbol, account_type: account_type_label,
                    timeframe: tf_name, bars, prev_bars,
                }
            }
            "renko" => {
                // Mirrors `append_renko`'s sorted upsert (the live path this
                // reseed must agree with).
                let key_fn = mlc_core::chart_type::renko_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::renko_identity);
                let mut bricks: Vec<mlc_core::types::RenkoBrick> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Renko(brick) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bricks, brick, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.renko_bars = Some(bricks));
                BubbleEvent::RenkoBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "pnf" => {
                // Mirrors `append_pnf`'s sorted upsert (the live path this
                // reseed must agree with).
                let key_fn = mlc_core::chart_type::pnf_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::pnf_identity);
                let mut cols: Vec<mlc_core::types::PnfColumn> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Pnf(col) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut cols, col, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.pnf_columns = Some(cols));
                BubbleEvent::PnfBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "kagi" => {
                // Mirrors `append_kagi`'s sorted upsert (the live path this
                // reseed must agree with).
                let key_fn = mlc_core::chart_type::kagi_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::kagi_identity);
                let mut segs: Vec<mlc_core::types::KagiSegment> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::Kagi(seg) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut segs, seg, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.kagi_segments = Some(segs));
                BubbleEvent::KagiBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "three_line_break" => {
                // Mirrors `append_three_line_break`'s sorted upsert (the
                // live path this reseed must agree with).
                let key_fn = mlc_core::chart_type::three_line_break_identity_key_fn(identity)
                    .unwrap_or(mlc_core::chart_type::three_line_break_identity);
                let mut lines: Vec<mlc_core::types::ThreeLineBreakLine> = Vec::new();
                for o in outputs {
                    let crate::bubble::FoldOutput::ThreeLineBreak(line) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut lines, line, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.three_line_break = Some(lines));
                BubbleEvent::TlbLineAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            _ => return None,
        };
        Some(event)
    }

    /// Fold one live "1m" kline tick into `bid`'s active fold runtime, if
    /// installed, and append/mutate-last EVERY output into the bubble's slot
    /// (a gap-crossing delta-tick can close several intermediate range bars
    /// / Renko bricks in one call — see [`crate::bubble::FoldRuntime::
    /// on_live_1m`]'s doc comment; each is applied in order with the same
    /// upsert/append logic a single output used to get). `exchange_id`/
    /// `symbol`/`account_type_label` are the SAME identity the triggering
    /// `LiveUpdate::BarUpdate` event carried (`tick_writers` is the only
    /// caller) — not re-derived from the bubble, since a bubble might not
    /// even be the primary owner of this instrument key. Returns ONE
    /// reconcile-triggering [`BubbleEvent`] built from the LAST output
    /// (avoids event-spam for a multi-element chain — every intermediate
    /// output has already been durably applied to the bubble's slot by the
    /// time this returns), `None` when the bubble has no active fold runtime
    /// (not one of the 7 fold types) or the fold produced nothing
    /// (duplicate/absorbed tick).
    fn fold_on_live_1m(
        &self,
        bid: BubbleId,
        bar: Bar,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let outputs = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_fold.as_mut().map(|f| f.on_live_1m(&bar))
        }).flatten()?;
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();

        let mut event = None;
        for output in outputs {
            event = Some(match output {
                crate::bubble::FoldOutput::Bar(new_bar) => {
                    let mut mgr = self.bubbles_mut();
                    let last_ts_identity = mgr.bars(bid).last().map(|b| {
                        if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }
                    });
                    let new_ts_identity = if new_bar.derived_ts_ms != 0 { new_bar.derived_ts_ms } else { new_bar.timestamp };
                    if last_ts_identity == Some(new_ts_identity) {
                        mgr.with_bubble_mut(bid, |b| {
                            if let Some(last) = b.last_bar_mut() {
                                last.open = new_bar.open;
                                last.high = new_bar.high;
                                last.low = new_bar.low;
                                last.close = new_bar.close;
                                last.volume = new_bar.volume;
                            }
                        });
                    } else {
                        mgr.with_bubble_mut(bid, |b| b.append_bar(new_bar));
                    }
                    drop(mgr);
                    BubbleEvent::BarUpdatePassthrough {
                        exchange_id, account_type: account_type_label.clone(), symbol: symbol.clone(),
                        bar: self.bubbles().bars(bid).last().copied().unwrap_or(new_bar),
                    }
                }
                crate::bubble::FoldOutput::Renko(brick) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_renko(brick));
                    BubbleEvent::RenkoBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::FoldOutput::Pnf(col) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_pnf(col));
                    BubbleEvent::PnfBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::FoldOutput::Kagi(seg) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_kagi(seg));
                    BubbleEvent::KagiBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::FoldOutput::ThreeLineBreak(line) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_three_line_break(line));
                    BubbleEvent::TlbLineAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
            });
        }
        event
    }

    // =========================================================================
    // === Client-side trade fold (unified-data-plane Stage Э4, 2026-07-09) ===
    // =========================================================================

    /// Install (or replace) `bid`'s [`bubble::TradeFoldRuntime`] for one of
    /// the trade-fed chart types (tick_bar/tick_imbalance_bar/
    /// volume_imbalance_bar/run_bar/footprint/tpo), ensure the bubble's
    /// trade layer is network-fed (continuous live subscribe + ramped
    /// backfill toward `derived_trade_target`), and reseed synchronously
    /// from whatever the layer already holds. Mirrors
    /// [`Self::subscribe_fold_chart_type`] exactly, one layer over (trade
    /// layer instead of the shared "1m" kline layer) — see that method's
    /// doc comment for the synchronous/async split rationale.
    ///
    /// `tpo_source` is ignored for every chart_type except `"tpo"` — see
    /// [`crate::bubble::TradeFoldMachine::new`]'s doc comment.
    ///
    /// The `"tpo"` Kline1m flavour also needs the "1m" kline layer
    /// network-fed (same as [`Self::subscribe_fold_chart_type`]'s callers
    /// use for the 7 kline-sufficient types) — handled here too so a caller
    /// never has to remember to call both entrypoints for that one flavour.
    pub fn subscribe_trade_fold_chart_type(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        params: crate::bubble::FoldParams,
        tpo_source: mlc_backend_data_bridge::TpoSource,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        derived_trade_target: Option<u32>,
    ) -> Option<BubbleEvent> {
        mlc_backend_data_bridge::dlog!(
            "[tradefold] subscribe ct={chart_type} count={} alpha={} min_ticks={} interval_ms={}",
            params.count, params.alpha, params.min_ticks, params.interval_ms
        );

        // Fix 2 (chart-type-engine-consolidation B1): install BEFORE
        // ensure — mirrors `subscribe_fold_chart_type`'s ordering fix, one
        // layer over (trade layer instead of "1m" kline). Build + reseed
        // the trade-fold runtime over whatever the trade layer (and, for
        // TPO Kline1m, the "1m" kline layer) holds RIGHT NOW, install it
        // into the bubble, and only THEN fire the async network calls
        // below (`subscribe_trades` itself starts an unbounded
        // `ensure_trade_backfill` page — see its doc comment — so it is
        // just as much a part of this race as the explicit
        // `derived_trade_target` call). By the time this method returns,
        // `active_trade_fold` is already installed, so the
        // `LiveUpdate::LayerBackfilled` refold sweep in `tick_writers` can
        // never find this bubble mid-install and skip it.
        let fold = crate::bubble::TradeFoldRuntime::new(chart_type, params, tpo_source, exchange_id, symbol)?;
        let event = self.reseed_trade_fold(bid, fold, exchange_id, symbol, account_type);

        // Live feed: `subscribe_trades` is idempotent per (eid, at, symbol)
        // and already fires the first shallow `ensure_trade_backfill` page
        // (see its doc comment) — calling it again here is a cheap no-op
        // when another window/indicator already opened the same trade sub.
        self.subscribe_trades(exchange_id, symbol, account_type);
        // History: ramped backfill toward `derived_trade_target`.
        self.bridge.ensure_trade_backfill(exchange_id, symbol, account_type, derived_trade_target.map(|t| t as usize));

        if matches!(chart_type, "tpo") && matches!(tpo_source, mlc_backend_data_bridge::TpoSource::Kline1m) {
            let one_minute = Timeframe::new("1m", 60_000);
            self.bridge.request_bars(exchange_id, symbol, &one_minute, account_type, None, None, false);
            self.bridge.ensure_kline_backfill(exchange_id, symbol, &one_minute, account_type, None);
        }

        event
    }

    /// Re-seed `bid`'s existing (or freshly-installed) trade-fold runtime
    /// from the bubble's current trade-layer (and, for TPO Kline1m, "1m"
    /// kline-layer) snapshot, replace the output slot wholesale, and return
    /// the reconcile-triggering [`BubbleEvent`].
    fn reseed_trade_fold(
        &self,
        bid: BubbleId,
        mut fold: crate::bubble::TradeFoldRuntime,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
    ) -> Option<BubbleEvent> {
        let trade_snapshot: crate::bubble::RawLayer<crate::bubble::RawTrade> = self.bubbles()
            .trade_layer(bid)
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let kline_snapshot: crate::bubble::RawLayer<Bar> = self.bubbles()
            .kline_layer(bid, "1m")
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let outputs = fold.reseed(&trade_snapshot, &kline_snapshot);
        let chart_type = fold.chart_type();
        self.bubbles_mut().with_bubble_mut(bid, |b| b.active_trade_fold = Some(fold));
        let account_type_label = account_type.short_label().to_string();
        self.apply_trade_fold_reseed_outputs(bid, chart_type, outputs, exchange_id, symbol, &account_type_label)
    }

    /// Replace `bid`'s trade-fold output slot wholesale with `outputs` (a
    /// full reseed's worth of points, oldest-first) and build the matching
    /// reconcile-triggering [`BubbleEvent`]. Shared by
    /// [`Self::reseed_trade_fold`] and the `LiveUpdate::LayerBackfilled` arm
    /// in [`Self::tick_writers`] (scroll-deepen refold path for
    /// `layer == "trades"`) — mirrors [`Self::apply_fold_reseed_outputs`]'s
    /// role for the kline-fold family.
    fn apply_trade_fold_reseed_outputs(
        &self,
        bid: BubbleId,
        chart_type: &'static str,
        outputs: Vec<crate::bubble::TradeFoldOutput>,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();
        // Identity-key mapping is registry-driven (chart-type-engine-
        // consolidation B3, S6) — see `apply_fold_reseed_outputs`'s doc
        // comment for the same rationale. `"cvd"` is not a registry row
        // (see `view_fold.rs`'s doc comment on the synthetic id) — its
        // identity is `IdentityKey::TsMs` directly, not a `by_id` lookup.
        let identity = if chart_type == "cvd" {
            mlc_core::chart_type::IdentityKey::TsMs
        } else {
            mlc_core::chart_type::by_id(chart_type).map(|d| d.identity).unwrap_or(mlc_core::chart_type::IdentityKey::None)
        };
        let event = match chart_type {
            "tick_bar" | "tick_imbalance_bar" | "volume_imbalance_bar" | "run_bar" => {
                // The machines emit an output on EVERY consumed trade — an
                // in-progress snapshot of the currently-open bar (open frozen
                // at the bar's first trade, close walking) plus the final
                // closed shape when it rolls. The LIVE path merges those by
                // ts-identity in `trade_fold_on_live_trade`'s upsert branch;
                // a reseed replay MUST apply the same last-wins merge or the
                // seed array keeps every intermediate snapshot as a separate
                // bar (N copies per bar → solid staircase blocks on screen).
                let key_fn = mlc_core::chart_type::bar_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::bar_identity);
                let mut bars: Vec<Bar> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Bar(b) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bars, b, key_fn);
                }
                let mut mgr = self.bubbles_mut();
                let prev_bars = mgr.bars(bid).to_vec();
                let tf_name = mgr.timeframe(bid).map(|tf| tf.name.clone()).unwrap_or_default();
                mgr.replace_bars(bid, bars.clone());
                drop(mgr);
                BubbleEvent::BarsLoaded {
                    bid, exchange_id, symbol, account_type: account_type_label,
                    timeframe: tf_name, bars, prev_bars,
                }
            }
            "footprint" => {
                // Mirrors the live upsert in `trade_fold_on_live_trade`'s
                // `TradeFoldOutput::Footprint` arm (the machine emits a
                // growing-cluster snapshot per trade, not just on bar-close).
                let key_fn = mlc_core::chart_type::footprint_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::footprint_identity);
                let mut bars: Vec<mlc_core::types::FootprintBar> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Footprint(b) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut bars, b, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| {
                    // Fold-sourced footprint mirrors the OHLC ONLY when this
                    // bubble is on the full-takeover path (`bars_source ==
                    // FootprintOhlc`) — same gate the station-derived
                    // `LiveUpdate::FootprintUpdate` arm uses. Hybrid mode
                    // (RecentOnly venue, `bars_source` stays `Kline`) leaves
                    // `bubble.bars` alone — the kline backdrop writer owns
                    // it (see `Self::trade_fold_on_live_trade`'s hybrid
                    // handling for the matching live-path rule).
                    if b.bars_source == crate::bubble::BarsSource::FootprintOhlc {
                        let ohlc: Vec<mlc_core::Bar> = bars.iter().map(|fb| mlc_core::Bar::with_volume(
                            fb.open_time / 1000, fb.open, fb.high, fb.low, fb.close, fb.volume,
                        )).collect();
                        b.replace_bars(ohlc);
                    }
                    b.footprint_bars = Some(bars);
                });
                BubbleEvent::FootprintBarAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "tpo" => {
                // Dedup consecutive same-identity snapshots (the fold emits
                // one output per upstream event, many trades/bars share the
                // same session until it rolls) down to the LAST snapshot per
                // session via `merge_last_wins`, then cap to the same
                // recency window the live station-derived path uses
                // (`MAX_TPO_SESSIONS = 11`) — the cap is TPO-specific
                // bookkeeping the shared helper doesn't know about, so it
                // stays a second pass here.
                let key_fn = mlc_core::chart_type::tpo_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::tpo_identity);
                let mut deduped: Vec<mlc_core::types::TpoSession> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Tpo(s) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut deduped, s, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| {
                    const MAX_TPO_SESSIONS: usize = 11;
                    if deduped.len() > MAX_TPO_SESSIONS {
                        let drop_n = deduped.len() - MAX_TPO_SESSIONS;
                        deduped.drain(0..drop_n);
                    }
                    b.tpo_sessions = Some(deduped);
                });
                BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol, account_type: account_type_label }
            }
            "cvd" => {
                // The machine emits one sample per consumed trade;
                // consecutive same-identity samples collapse to the last
                // value instead of oversampling the line.
                let key_fn = mlc_core::chart_type::cvd_identity_key_fn(identity).unwrap_or(mlc_core::chart_type::cvd_identity);
                let mut samples: Vec<mlc_core::types::CvdSample> = Vec::new();
                for o in outputs {
                    let crate::bubble::TradeFoldOutput::Cvd(s) = o else { continue };
                    mlc_core::chart_type::merge_last_wins(&mut samples, s, key_fn);
                }
                self.bubbles_mut().with_bubble_mut(bid, |b| b.cvd_line = Some(samples));
                BubbleEvent::CvdSampleAppended { bid, exchange_id, symbol, account_type: account_type_label }
            }
            _ => return None,
        };
        Some(event)
    }

    /// Fold one live trade tick into `bid`'s active trade-fold runtime, if
    /// installed, and append/upsert EVERY output into the bubble's slot, in
    /// order. Mirrors [`Self::fold_on_live_1m`]'s role (and multi-output
    /// handling) for the trade-fed family — `exchange_id`/`symbol`/
    /// `account_type_label` are the SAME identity the triggering
    /// `LiveUpdate::TradeUpdate` event carried. Returns ONE reconcile-
    /// triggering [`BubbleEvent`] built from the LAST output (see
    /// [`Self::fold_on_live_1m`]'s doc comment on the event-spam rationale),
    /// `None` when the bubble has no active trade-fold runtime or the fold
    /// produced nothing.
    fn trade_fold_on_live_trade(
        &self,
        bid: BubbleId,
        trade: crate::bubble::RawTrade,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let outputs = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_trade_fold.as_mut().map(|f| f.on_live_trade(&trade))
        }).flatten()?;
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();

        let mut event = None;
        for output in outputs {
            event = Some(match output {
                crate::bubble::TradeFoldOutput::Bar(new_bar) => {
                    let mut mgr = self.bubbles_mut();
                    let last_ts_identity = mgr.bars(bid).last().map(|b| {
                        if b.derived_ts_ms != 0 { b.derived_ts_ms } else { b.timestamp }
                    });
                    let new_ts_identity = if new_bar.derived_ts_ms != 0 { new_bar.derived_ts_ms } else { new_bar.timestamp };
                    mlc_backend_data_bridge::dlog!(
                        "[tradefold] live o={} h={} l={} c={} id={new_ts_identity} last={last_ts_identity:?} branch={} len={}",
                        new_bar.open, new_bar.high, new_bar.low, new_bar.close,
                        if last_ts_identity == Some(new_ts_identity) { "upsert" } else { "append" },
                        mgr.bars(bid).len()
                    );
                    if last_ts_identity == Some(new_ts_identity) {
                        mgr.with_bubble_mut(bid, |b| {
                            if let Some(last) = b.last_bar_mut() {
                                last.open = new_bar.open;
                                last.high = new_bar.high;
                                last.low = new_bar.low;
                                last.close = new_bar.close;
                                last.volume = new_bar.volume;
                            }
                        });
                    } else {
                        mgr.with_bubble_mut(bid, |b| b.append_bar(new_bar));
                    }
                    drop(mgr);
                    BubbleEvent::BarUpdatePassthrough {
                        exchange_id, account_type: account_type_label.clone(), symbol: symbol.clone(),
                        bar: self.bubbles().bars(bid).last().copied().unwrap_or(new_bar),
                    }
                }
                crate::bubble::TradeFoldOutput::Footprint(new_bar) => {
                    let mirror_ohlc = self.bubbles().bars_source(bid) == crate::bubble::BarsSource::FootprintOhlc;
                    self.bubbles_mut().with_bubble_mut(bid, |b| {
                        let ohlc_ts = new_bar.open_time / 1000;
                        let store = b.footprint_bars.get_or_insert_with(Vec::new);
                        match store.last_mut() {
                            Some(last) if last.open_time == new_bar.open_time => *last = new_bar.clone(),
                            _ => store.push(new_bar.clone()),
                        }
                        // Hybrid mode (RecentOnly venue): `mirror_ohlc == false`
                        // — the kline backdrop writer owns `bubble.bars`, same
                        // gate `LiveUpdate::FootprintUpdate`'s station-derived
                        // arm already uses (see that arm's doc comment).
                        if mirror_ohlc {
                            let ohlc = mlc_core::Bar::with_volume(
                                ohlc_ts, new_bar.open, new_bar.high, new_bar.low, new_bar.close, new_bar.volume,
                            );
                            match b.bars.last_mut() {
                                Some(last) if last.timestamp == ohlc_ts => *last = ohlc,
                                _ => b.append_bar(ohlc),
                            }
                        }
                    });
                    BubbleEvent::FootprintBarAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::TradeFoldOutput::Tpo(session) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| {
                        let store = b.tpo_sessions.get_or_insert_with(Vec::new);
                        match store.last_mut() {
                            Some(last) if last.open_time == session.open_time => *last = session,
                            _ => {
                                store.push(session);
                                const MAX_TPO_SESSIONS: usize = 11;
                                while store.len() > MAX_TPO_SESSIONS {
                                    store.remove(0);
                                }
                            }
                        }
                    });
                    BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
                crate::bubble::TradeFoldOutput::Cvd(sample) => {
                    self.bubbles_mut().with_bubble_mut(bid, |b| b.append_cvd(sample));
                    BubbleEvent::CvdSampleAppended { bid, exchange_id, symbol: symbol.clone(), account_type: account_type_label.clone() }
                }
            });
        }
        event
    }

    /// Fold one live "1m" kline tick into `bid`'s active trade-fold runtime
    /// IF it's the TPO Kline1m flavour (every other trade-fed chart_type's
    /// runtime is a no-op for `Event::Bar` — see
    /// [`crate::bubble::TradeFoldRuntime::on_live_1m`]). Mirrors
    /// [`Self::fold_on_live_1m`]'s upsert-into-`tpo_sessions` shape, applied
    /// to EVERY output in order (today always 0 or 1 — TPO never populates a
    /// pending queue — kept as a loop for symmetry with the other live-fold
    /// handlers, see [`Self::fold_on_live_1m`]'s doc comment).
    fn trade_fold_on_live_1m(
        &self,
        bid: BubbleId,
        bar: Bar,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let sessions: Vec<mlc_core::types::TpoSession> = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_trade_fold.as_mut().map(|f| f.on_live_1m(&bar))
        }).flatten()?.into_iter().filter_map(|o| match o {
            crate::bubble::TradeFoldOutput::Tpo(s) => Some(s),
            _ => None,
        }).collect();
        if sessions.is_empty() {
            return None;
        }
        let symbol = symbol.to_string();
        let account_type_label = account_type_label.to_string();
        for session in sessions {
            self.bubbles_mut().with_bubble_mut(bid, |b| {
                let store = b.tpo_sessions.get_or_insert_with(Vec::new);
                match store.last_mut() {
                    Some(last) if last.open_time == session.open_time => *last = session,
                    _ => {
                        store.push(session);
                        const MAX_TPO_SESSIONS: usize = 11;
                        while store.len() > MAX_TPO_SESSIONS {
                            store.remove(0);
                        }
                    }
                }
            });
        }
        Some(BubbleEvent::TpoSessionUpdated { bid, exchange_id, symbol, account_type: account_type_label })
    }

    // =========================================================================
    // === Client-side CVD companion fold (unified-data-plane Stage Э4/Э5) ====
    // =========================================================================

    /// Install (or replace) `bid`'s [`bubble::TradeFoldRuntime`] CVD
    /// companion runtime, ensure the trade layer is network-fed, and reseed
    /// synchronously from whatever the layer already holds. Called from
    /// `subscribe_cvd_line_for_indicator_add` (mlc-app) — the ONLY CVD data
    /// path since Stage Э5 removed the station-derived `request_cvd_line`
    /// subscribe.
    pub fn subscribe_cvd_fold(
        &self,
        bid: BubbleId,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        derived_trade_target: Option<u32>,
    ) -> Option<BubbleEvent> {
        self.subscribe_trades(exchange_id, symbol, account_type);
        self.bridge.ensure_trade_backfill(exchange_id, symbol, account_type, derived_trade_target.map(|t| t as usize));

        let mut fold = crate::bubble::TradeFoldRuntime::new(
            "cvd", crate::bubble::FoldParams::default(), mlc_backend_data_bridge::TpoSource::TradeBucket,
            exchange_id, symbol,
        )?;
        let trade_snapshot: crate::bubble::RawLayer<crate::bubble::RawTrade> = self.bubbles()
            .trade_layer(bid)
            .cloned()
            .unwrap_or_else(|| crate::bubble::RawLayer::new(0));
        let outputs = fold.reseed(&trade_snapshot, &crate::bubble::RawLayer::new(0));
        self.bubbles_mut().with_bubble_mut(bid, |b| b.active_cvd_fold = Some(fold));
        let account_type_label = account_type.short_label().to_string();
        self.apply_trade_fold_reseed_outputs(bid, "cvd", outputs, exchange_id, symbol, &account_type_label)
    }

    /// Fold one live trade tick into `bid`'s active CVD companion runtime,
    /// if installed. Mirrors [`Self::trade_fold_on_live_trade`]'s CVD arm,
    /// scoped to the independent `active_cvd_fold` slot. Applies EVERY
    /// output in order (today always 0 or 1 — CVD never populates a pending
    /// queue — kept as a loop for symmetry, see [`Self::fold_on_live_1m`]'s
    /// doc comment).
    fn cvd_fold_on_live_trade(
        &self,
        bid: BubbleId,
        trade: crate::bubble::RawTrade,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type_label: &str,
    ) -> Option<BubbleEvent> {
        let samples: Vec<mlc_core::types::CvdSample> = self.bubbles_mut().with_bubble_mut(bid, |b| {
            b.active_cvd_fold.as_mut().map(|f| f.on_live_trade(&trade))
        }).flatten()?.into_iter().filter_map(|o| match o {
            crate::bubble::TradeFoldOutput::Cvd(s) => Some(s),
            _ => None,
        }).collect();
        if samples.is_empty() {
            return None;
        }
        for sample in samples {
            self.bubbles_mut().with_bubble_mut(bid, |b| b.append_cvd(sample));
        }
        Some(BubbleEvent::CvdSampleAppended {
            bid, exchange_id, symbol: symbol.to_string(), account_type: account_type_label.to_string(),
        })
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

#[cfg(test)]
mod open_presets_claim_tests {
    //! Coverage for the `open_presets` claim registry (shared-BackendCore
    //! arc §5.3.5): "a preset is either parked-nowhere or live in exactly
    //! one window" — `try_claim_preset` / `release_preset` /
    //! `release_all_presets_for_app`.

    use super::*;

    /// Real `BackendCore` construction needs a `DataBridge`, which is async
    /// (spins up the digdigdig3 station + persistence dirs, no network dial
    /// until `ensure_connector`). A plain current-thread runtime is enough
    /// — these tests never touch the bridge itself.
    fn build_core() -> Arc<BackendCore> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio rt");
        let (bridge, _live_rx, _connector_ready_rx) = rt.block_on(DataBridge::new());
        BackendCore::new(Arc::new(bridge), None)
    }

    #[test]
    fn first_claim_succeeds() {
        let core = build_core();
        assert!(core.try_claim_preset("preset-a", "app-1"));
    }

    #[test]
    fn reclaim_by_same_app_succeeds() {
        let core = build_core();
        assert!(core.try_claim_preset("preset-a", "app-1"));
        // Re-claiming your own already-open preset is not a collision —
        // e.g. park then immediately unpark in the same window.
        assert!(core.try_claim_preset("preset-a", "app-1"));
    }

    #[test]
    fn claim_by_different_app_is_refused() {
        let core = build_core();
        assert!(core.try_claim_preset("preset-a", "app-1"));
        assert!(
            !core.try_claim_preset("preset-a", "app-2"),
            "a second window must not be able to claim a preset already live in another window"
        );
    }

    #[test]
    fn release_then_reclaim_by_different_app_succeeds() {
        let core = build_core();
        assert!(core.try_claim_preset("preset-a", "app-1"));
        assert!(core.release_preset("preset-a", "app-1"));
        assert!(
            core.try_claim_preset("preset-a", "app-2"),
            "once released, a different window must be able to claim the preset"
        );
    }

    #[test]
    fn release_by_non_owner_is_a_no_op() {
        let core = build_core();
        assert!(core.try_claim_preset("preset-a", "app-1"));
        assert!(
            !core.release_preset("preset-a", "app-2"),
            "releasing a claim you don't own must fail, not steal it"
        );
        // Original owner's claim must still be intact.
        assert!(
            !core.try_claim_preset("preset-a", "app-2"),
            "claim must still belong to app-1 after app-2's failed release"
        );
    }

    #[test]
    fn release_unclaimed_preset_is_a_no_op() {
        let core = build_core();
        assert!(!core.release_preset("never-claimed", "app-1"));
    }

    #[test]
    fn release_all_presets_for_app_releases_only_that_apps_claims() {
        let core = build_core();
        assert!(core.try_claim_preset("preset-a", "app-1"));
        assert!(core.try_claim_preset("preset-b", "app-1"));
        assert!(core.try_claim_preset("preset-c", "app-2"));

        let released = core.release_all_presets_for_app("app-1");
        assert_eq!(released, 2);

        // app-1's presets are now claimable by anyone else.
        assert!(core.try_claim_preset("preset-a", "app-2"));
        assert!(core.try_claim_preset("preset-b", "app-2"));
        // app-2's own preset was never touched — re-claiming it as app-2 is
        // just a reaffirm, not a fresh claim, but a THIRD app must still be
        // refused.
        assert!(!core.try_claim_preset("preset-c", "app-3"));
    }
}

#[cfg(test)]
mod phase_b_facade_accessor_tests {
    //! Coverage for the new facade accessors added while sweeping `main.rs`
    //! off `AppState`'s domain mirror onto the shared core directly
    //! (`docs/mlc/plans/shared-backend-core-2026-07-03.md` Phase B):
    //! `Watchlists::snapshot()` (wholesale serialization for
    //! `watchlists.json`) and `TemplatesMut::set_vault_key` (the
    //! template-manager vault-key write path `save_all`/unlock handlers now
    //! go through instead of poking `TemplateManager.vault_key` directly).

    use super::*;

    fn build_core() -> Arc<BackendCore> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio rt");
        let (bridge, _live_rx, _connector_ready_rx) = rt.block_on(DataBridge::new());
        BackendCore::new(Arc::new(bridge), None)
    }

    #[test]
    fn watchlists_snapshot_returns_owned_clone_matching_live_state() {
        let core = build_core();
        let list_id = core.watchlists_mut().create_list("My List".to_string());
        let address = mlc_core::InstrumentAddress::new(
            "binance", "BTCUSDT", mlc_core::instrument::AccountType::Spot,
        );
        core.watchlists_mut().add_symbol(list_id, address.clone());

        let snapshot = core.watchlists().snapshot();
        assert!(snapshot.find(list_id).is_some());
        assert!(snapshot.contains(list_id, &address));

        // Snapshot is a clone — mutating the live manager afterward must not
        // retroactively change the already-taken snapshot.
        core.watchlists_mut().remove_symbol(list_id, &address);
        assert!(snapshot.contains(list_id, &address), "snapshot must be a point-in-time owned clone, not a live view");
        assert!(!core.watchlists().contains(list_id, &address));
    }

    #[test]
    fn templates_set_vault_key_round_trips_through_facade() {
        let core = build_core();
        assert!(core.templates().save_to_default_dir(None).is_ok());

        let key: mlc_vault::vault::VaultKey = [7u8; 32];
        core.templates_mut().set_vault_key(Some(key));
        // No direct read-accessor for vault_key on the read facade (it's a
        // write-only concern from the app's perspective — save paths pass
        // it explicitly) — round-trip via a second set + no panic/poison
        // confirms the RwLock write path is sound end-to-end.
        core.templates_mut().set_vault_key(None);
    }
}

#[cfg(test)]
mod fold_reseed_last_wins_tests {
    //! Regression coverage for the reseed-replay dup-bar bug (2026-07-10):
    //! fold machines emit an output snapshot on EVERY consumed event, not
    //! just on element-close. `apply_fold_reseed_outputs` /
    //! `apply_trade_fold_reseed_outputs` must collapse same-identity
    //! snapshots to the LAST one (mirrors the live-path upsert rule) before
    //! landing them in the bubble's slot — a straight `.collect()` would
    //! keep every intermediate snapshot as a separate element.
    //!
    //! Each case feeds a synthetic `outputs` vec with 3 snapshots sharing
    //! one identity (values 1.0/2.0/3.0, only the last must survive) plus
    //! 1 snapshot of a second identity, and expects exactly 2 elements in
    //! the bubble's slot afterward with the last-wins value for the first.

    use super::*;

    fn build_core() -> Arc<BackendCore> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio rt");
        let (bridge, _live_rx, _connector_ready_rx) = rt.block_on(DataBridge::new());
        BackendCore::new(Arc::new(bridge), None)
    }

    fn make_bubble(core: &BackendCore) -> BubbleId {
        core.bubbles_mut().create_bubble("BTCUSDT".to_string(), Timeframe::m1())
    }

    #[test]
    fn range_bar_reseed_collapses_same_identity_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let outputs = vec![
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 1.0, ..Bar::default() }),
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 2.0, ..Bar::default() }),
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 3.0, ..Bar::default() }),
            crate::bubble::FoldOutput::Bar(Bar { timestamp: 160, derived_ts_ms: 160_000, close: 9.0, ..Bar::default() }),
        ];
        core.apply_fold_reseed_outputs(bid, "range_bar", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bars = mgr.bars(bid);
        assert_eq!(bars.len(), 2, "3 same-identity snapshots + 1 distinct must collapse to 2 bars");
        assert_eq!(bars[0].close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bars[1].close, 9.0);
    }

    #[test]
    fn renko_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, top: f64| mlc_core::types::RenkoBrick {
            open_time, bottom: 0.0, top, up: true, volume: 0.0, trades_count: 0,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::Renko(mk(100, 1.0)),
            crate::bubble::FoldOutput::Renko(mk(100, 2.0)),
            crate::bubble::FoldOutput::Renko(mk(100, 3.0)),
            crate::bubble::FoldOutput::Renko(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "renko", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bricks = mgr.renko_bars(bid).expect("renko_bars must be populated");
        assert_eq!(bricks.len(), 2);
        assert_eq!(bricks[0].top, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bricks[1].top, 9.0);
    }

    #[test]
    fn pnf_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, top: f64| mlc_core::types::PnfColumn {
            open_time, column_id: 1, is_x: true, bottom: 0.0, top, volume: 0.0, trades_count: 0, box_size: 1.0,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::Pnf(mk(100, 1.0)),
            crate::bubble::FoldOutput::Pnf(mk(100, 2.0)),
            crate::bubble::FoldOutput::Pnf(mk(100, 3.0)),
            crate::bubble::FoldOutput::Pnf(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "pnf", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let cols = mgr.pnf_columns(bid).expect("pnf_columns must be populated");
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].top, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(cols[1].top, 9.0);
    }

    #[test]
    fn kagi_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, end_price: f64| mlc_core::types::KagiSegment {
            open_time, start_price: 0.0, end_price, up: true, yang: true, is_connector: false,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::Kagi(mk(100, 1.0)),
            crate::bubble::FoldOutput::Kagi(mk(100, 2.0)),
            crate::bubble::FoldOutput::Kagi(mk(100, 3.0)),
            crate::bubble::FoldOutput::Kagi(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "kagi", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let segs = mgr.kagi_segments(bid).expect("kagi_segments must be populated");
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].end_price, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(segs[1].end_price, 9.0);
    }

    #[test]
    fn three_line_break_reseed_collapses_same_ts_open_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |ts_open: i64, price_close: f64| mlc_core::types::ThreeLineBreakLine {
            ts_open, ts_close: ts_open, price_open: 0.0, price_close, volume: 0.0, direction: 0,
        };
        let outputs = vec![
            crate::bubble::FoldOutput::ThreeLineBreak(mk(100, 1.0)),
            crate::bubble::FoldOutput::ThreeLineBreak(mk(100, 2.0)),
            crate::bubble::FoldOutput::ThreeLineBreak(mk(100, 3.0)),
            crate::bubble::FoldOutput::ThreeLineBreak(mk(200, 9.0)),
        ];
        core.apply_fold_reseed_outputs(bid, "three_line_break", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let lines = mgr.three_line_break(bid).expect("three_line_break must be populated");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].price_close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(lines[1].price_close, 9.0);
    }

    #[test]
    fn tick_bar_reseed_collapses_same_identity_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let outputs = vec![
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 1.0, ..Bar::default() }),
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 2.0, ..Bar::default() }),
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 100, derived_ts_ms: 100_000, close: 3.0, ..Bar::default() }),
            crate::bubble::TradeFoldOutput::Bar(Bar { timestamp: 160, derived_ts_ms: 160_000, close: 9.0, ..Bar::default() }),
        ];
        core.apply_trade_fold_reseed_outputs(bid, "tick_bar", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bars = mgr.bars(bid);
        assert_eq!(bars.len(), 2, "3 same-identity snapshots + 1 distinct must collapse to 2 bars");
        assert_eq!(bars[0].close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bars[1].close, 9.0);
    }

    #[test]
    fn footprint_reseed_collapses_same_open_time_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |open_time: i64, close: f64| mlc_core::types::FootprintBar {
            open_time, close, ..mlc_core::types::FootprintBar::default()
        };
        let outputs = vec![
            crate::bubble::TradeFoldOutput::Footprint(mk(100, 1.0)),
            crate::bubble::TradeFoldOutput::Footprint(mk(100, 2.0)),
            crate::bubble::TradeFoldOutput::Footprint(mk(100, 3.0)),
            crate::bubble::TradeFoldOutput::Footprint(mk(200, 9.0)),
        ];
        core.apply_trade_fold_reseed_outputs(bid, "footprint", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let bars = mgr.footprint_bars(bid).expect("footprint_bars must be populated");
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].close, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(bars[1].close, 9.0);
    }

    #[test]
    fn cvd_reseed_collapses_same_ts_ms_snapshots() {
        let core = build_core();
        let bid = make_bubble(&core);
        let mk = |ts_ms: i64, value: f64| mlc_core::types::CvdSample { ts_ms, value };
        let outputs = vec![
            crate::bubble::TradeFoldOutput::Cvd(mk(100, 1.0)),
            crate::bubble::TradeFoldOutput::Cvd(mk(100, 2.0)),
            crate::bubble::TradeFoldOutput::Cvd(mk(100, 3.0)),
            crate::bubble::TradeFoldOutput::Cvd(mk(200, 9.0)),
        ];
        core.apply_trade_fold_reseed_outputs(bid, "cvd", outputs, ExchangeId::Binance, "BTCUSDT", "S");
        let mgr = core.bubbles();
        let samples = mgr.cvd_line(bid).expect("cvd_line must be populated");
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].value, 3.0, "must keep the LAST snapshot's value, not the first");
        assert_eq!(samples[1].value, 9.0);
    }

    /// «4R != 100R» re-cut regression (chart-type-engine-consolidation B4,
    /// T2.2) — end-to-end through the SAME seams the real subscribe/refold
    /// path uses: `FoldRuntime::reseed` over a synthetic "1m" `RawLayer`,
    /// then `apply_fold_reseed_outputs` (the exact merge-and-land call the
    /// live reseed path makes) rather than hand-building `FoldOutput`s. A
    /// tight range threshold (small R) MUST cut visibly more bars than a
    /// wide one (big R) over the SAME input — the live re-cut bug this
    /// pins had chart-type/param switches over-fold identically because a
    /// shallow "1m" layer ramp was starved after the first fold type
    /// claimed it (see this crate's docs on the 4R=100R bug); this test
    /// pins the FOLD MATH side (param actually changes the cut), not the
    /// backfill-ramp side (covered elsewhere).
    #[test]
    fn range_bar_recut_small_r_produces_far_more_bars_than_big_r() {
        // Synthetic price walk: sin (amplitude 200) riding a linear trend
        // (+8/bar) over 200 1m bars, each expanded into a 4-leg synthetic
        // trade path (O -> L/H -> H/L -> C) by `kline_to_synthetic_trades`
        // — the actual granularity a range-bar fold walks over. Amplitude
        // and trend both dwarf both R thresholds under test (R=1 tight,
        // R=25 wide) so neither collapses to a near-empty cut.
        const N: usize = 200;
        const SMALL_R: f64 = 1.0;
        const BIG_R: f64 = 25.0;
        let mut layer: crate::bubble::RawLayer<Bar> = crate::bubble::RawLayer::new(0);
        let mut base = 1000.0_f64;
        for i in 0..N {
            let ts_sec = (i as i64 + 1) * 60;
            let open = base + 200.0 * (i as f64 * 0.3).sin();
            base += 8.0;
            let close = base + 200.0 * ((i + 1) as f64 * 0.3).sin();
            let high = open.max(close) + 15.0;
            let low = open.min(close) - 15.0;
            layer.push_live(Bar {
                timestamp: ts_sec,
                open, high, low, close,
                volume: 10.0,
                derived_ts_ms: ts_sec * 1000,
            });
        }

        let run = |scaled_units: f64| -> Vec<Bar> {
            let core = build_core();
            let bid = make_bubble(&core);
            let params = crate::bubble::FoldParams { scaled_units, ..Default::default() };
            let mut fold = crate::bubble::FoldRuntime::new("range_bar", params, ExchangeId::Binance, "BTCUSDT")
                .expect("range_bar is a fold type");
            let outputs = fold.reseed(&layer);
            core.apply_fold_reseed_outputs(bid, "range_bar", outputs, ExchangeId::Binance, "BTCUSDT", "S");
            let bars = core.bubbles().bars(bid).to_vec();
            bars
        };

        let small_bars = run(SMALL_R);
        let big_bars = run(BIG_R);

        assert!(
            small_bars.len() > 3 * big_bars.len(),
            "small R={SMALL_R} must cut far more bars than big R={BIG_R}: small={} big={}",
            small_bars.len(), big_bars.len(),
        );
        // Both cuts must be non-trivial relative to the input — neither R
        // starved down to a near-empty fold of the 200-bar layer (the
        // actual shape of the live "same tiny window forever" bug: an
        // apparently successful re-cut that only ever touches a sliver of
        // the layer). Compared against the raw bar count, not the 4x
        // synthetic-trade-leg expansion, since a range-bar cut's element
        // count is naturally far smaller than its leg-granularity input.
        assert!(small_bars.len() > N / 2, "small R cut only {} bars from {N} input bars", small_bars.len());
        assert!(big_bars.len() > N / 2, "big R cut only {} bars from {N} input bars", big_bars.len());

        // Every CLOSED big-R bar (all but the last, which may still be
        // in-progress) must be pinned to AT LEAST the configured range on
        // its crossing side and must chain open-to-prior-close — the "4R
        // vs 100R identical" bug's other face was bars that didn't
        // actually respect the configured threshold at all. Width can
        // exceed R (not just equal it): the crossing side is pinned exactly
        // to `range` (`TradeToRangeBarDerived::fold_trade`'s `bar.high =
        // close_price` / `bar.low = close_price` on the crossing branch),
        // but the OPPOSITE side keeps whatever organic excursion the bar
        // absorbed before it crossed — real (non-single-gap-leg) price
        // walks routinely produce this, so `>=` is the correct invariant
        // here (the single-leg case where width == R exactly is already
        // pinned by `reseed_range_bar_gap_crossing_kline_leg_produces_chained_bars`).
        let (closed, _last) = big_bars.split_at(big_bars.len().saturating_sub(1));
        for (i, b) in closed.iter().enumerate() {
            assert!(
                b.high - b.low >= BIG_R - 1e-9,
                "closed big-R bar #{i} has |high-low|={} < R={BIG_R}", b.high - b.low,
            );
            if i > 0 {
                assert!(
                    (b.open - closed[i - 1].close).abs() < 1e-9,
                    "big-R bar #{i}.open ({}) must equal bar #{}.close ({})",
                    b.open, i - 1, closed[i - 1].close,
                );
            }
        }
    }
}
