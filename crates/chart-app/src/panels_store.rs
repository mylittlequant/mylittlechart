//! State store for the 11 trading panels that live inside the free-slot
//! sidebar hyperspace (Slot1..Slot4).
//!
//! `TradingPanelsStore` is chart-app-owned and holds heavy state keyed by
//! `PanelId`. The lightweight sidebar docking trees hold only `FreeItem`
//! variants that carry the matching `PanelId`, keeping `sidebar-content`
//! free of any `zengeld-panels` dependency.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use sidebar_content::free_slot::PanelId;

use zengeld_panels::trading::order_flow::{
    big_trades::BigTradesState,
    dom::DomState,
    footprint::FootprintState,
    l2_tape::L2TapeState,
    liquidity_heatmap::LiquidityHeatmapState,
    volume_profile::VolumeProfileState,
};
use zengeld_panels::trading::trading::{
    order_entry::OrderEntryState,
    position_manager::PositionManagerState,
    risk_calculator::RiskCalculatorState,
    trade_log::TradeLogState,
    trading_container::TradingContainerState,
};

// Re-export FreeItem for convenience in callers that need it alongside the store.
use sidebar_content::free_slot::FreeItem;

/// Owns the heavy state for all 11 trading panel kinds.
///
/// All state maps are keyed by `PanelId`. The `next_id` counter is used to
/// allocate fresh ids; after restoring from a preset, call `set_min_next_id`
/// to prevent collisions with persisted ids.
pub struct TradingPanelsStore {
    next_id: AtomicU64,
    pub dom: HashMap<PanelId, DomState>,
    pub footprint: HashMap<PanelId, FootprintState>,
    pub volume_profile: HashMap<PanelId, VolumeProfileState>,
    pub liquidity_heatmap: HashMap<PanelId, LiquidityHeatmapState>,
    pub big_trades: HashMap<PanelId, BigTradesState>,
    pub l2_tape: HashMap<PanelId, L2TapeState>,
    pub order_entry: HashMap<PanelId, OrderEntryState>,
    pub position_manager: HashMap<PanelId, PositionManagerState>,
    pub trade_log: HashMap<PanelId, TradeLogState>,
    pub risk_calculator: HashMap<PanelId, RiskCalculatorState>,
    pub trading_container: HashMap<PanelId, TradingContainerState>,
}

impl TradingPanelsStore {
    /// Create an empty store. Counter starts at 1 (0 is reserved for "null").
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            dom: HashMap::new(),
            footprint: HashMap::new(),
            volume_profile: HashMap::new(),
            liquidity_heatmap: HashMap::new(),
            big_trades: HashMap::new(),
            l2_tape: HashMap::new(),
            order_entry: HashMap::new(),
            position_manager: HashMap::new(),
            trade_log: HashMap::new(),
            risk_calculator: HashMap::new(),
            trading_container: HashMap::new(),
        }
    }

    fn alloc_id(&self) -> PanelId {
        PanelId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    // -------------------------------------------------------------------------
    // create_* — allocate a fresh id, insert default state, return id
    // -------------------------------------------------------------------------

    /// Allocate a new DOM panel with the given symbol and tick size.
    pub fn create_dom(&mut self, symbol: String, tick_size: f64) -> PanelId {
        let id = self.alloc_id();
        self.dom.insert(id, DomState::new(symbol, tick_size));
        id
    }

    /// Allocate a new Footprint panel.
    pub fn create_footprint(&mut self, symbol: String, tick_size: f64) -> PanelId {
        let id = self.alloc_id();
        self.footprint.insert(id, FootprintState::new(symbol, tick_size));
        id
    }

    /// Allocate a new Volume Profile panel.
    pub fn create_volume_profile(&mut self, symbol: String, tick_size: f64) -> PanelId {
        let id = self.alloc_id();
        self.volume_profile.insert(id, VolumeProfileState::new(symbol, tick_size));
        id
    }

    /// Allocate a new Liquidity Heatmap panel.
    ///
    /// `snapshot_interval_ms` defaults to 5000 (5 s) when not known yet.
    pub fn create_liquidity_heatmap(
        &mut self,
        symbol: String,
        tick_size: f64,
        snapshot_interval_ms: u64,
    ) -> PanelId {
        let id = self.alloc_id();
        self.liquidity_heatmap.insert(
            id,
            LiquidityHeatmapState::new(symbol, tick_size, snapshot_interval_ms),
        );
        id
    }

    /// Allocate a new Big Trades panel.
    pub fn create_big_trades(&mut self) -> PanelId {
        let id = self.alloc_id();
        self.big_trades.insert(id, BigTradesState::new());
        id
    }

    /// Allocate a new L2 Tape panel.
    pub fn create_l2_tape(&mut self) -> PanelId {
        let id = self.alloc_id();
        self.l2_tape.insert(id, L2TapeState::new());
        id
    }

    /// Allocate a new Order Entry panel.
    pub fn create_order_entry(&mut self, symbol: String) -> PanelId {
        let id = self.alloc_id();
        self.order_entry.insert(id, OrderEntryState::new(symbol));
        id
    }

    /// Allocate a new Position Manager panel.
    pub fn create_position_manager(&mut self) -> PanelId {
        let id = self.alloc_id();
        self.position_manager.insert(id, PositionManagerState::new());
        id
    }

    /// Allocate a new Trade Log panel.
    pub fn create_trade_log(&mut self) -> PanelId {
        let id = self.alloc_id();
        self.trade_log.insert(id, TradeLogState::new());
        id
    }

    /// Allocate a new Risk Calculator panel.
    pub fn create_risk_calculator(&mut self) -> PanelId {
        let id = self.alloc_id();
        self.risk_calculator.insert(id, RiskCalculatorState::new());
        id
    }

    /// Allocate a new Trading Container panel.
    pub fn create_trading_container(
        &mut self,
        symbol: String,
        tick_size: f64,
        market_price: f64,
    ) -> PanelId {
        let id = self.alloc_id();
        self.trading_container.insert(
            id,
            TradingContainerState::new(symbol, tick_size, market_price),
        );
        id
    }

    // -------------------------------------------------------------------------
    // clone_item — duplicate a FreeItem with fresh PanelId + copied config
    // -------------------------------------------------------------------------

    /// Create a new `FreeItem` of the same variant as `source`, with a fresh
    /// `PanelId` and state cloned from the source's config (symbol, tick_size,
    /// etc.). Returns `None` if the source's state is missing from the store.
    pub fn clone_item(&mut self, source: &FreeItem) -> Option<FreeItem> {
        match source {
            FreeItem::Dom(id) => {
                let s = self.dom.get(id)?;
                let pid = self.alloc_id();
                self.dom.insert(pid, DomState::new(s.symbol.clone(), s.tick_size));
                Some(FreeItem::Dom(pid))
            }
            FreeItem::Footprint(id) => {
                let s = self.footprint.get(id)?;
                let pid = self.alloc_id();
                self.footprint.insert(pid, FootprintState::new(s.symbol.clone(), s.tick_size));
                Some(FreeItem::Footprint(pid))
            }
            FreeItem::VolumeProfile(id) => {
                let s = self.volume_profile.get(id)?;
                let pid = self.alloc_id();
                self.volume_profile.insert(pid, VolumeProfileState::new(s.symbol.clone(), s.tick_size));
                Some(FreeItem::VolumeProfile(pid))
            }
            FreeItem::LiquidityHeatmap(id) => {
                let s = self.liquidity_heatmap.get(id)?;
                let pid = self.alloc_id();
                self.liquidity_heatmap.insert(pid, LiquidityHeatmapState::new(
                    s.symbol.clone(), s.tick_size, s.snapshot_interval_ms,
                ));
                Some(FreeItem::LiquidityHeatmap(pid))
            }
            FreeItem::BigTrades(_) => {
                let pid = self.alloc_id();
                self.big_trades.insert(pid, BigTradesState::new());
                Some(FreeItem::BigTrades(pid))
            }
            FreeItem::L2Tape(_) => {
                let pid = self.alloc_id();
                self.l2_tape.insert(pid, L2TapeState::new());
                Some(FreeItem::L2Tape(pid))
            }
            FreeItem::OrderEntry(id) => {
                let s = self.order_entry.get(id)?;
                let pid = self.alloc_id();
                self.order_entry.insert(pid, OrderEntryState::new(s.symbol.clone()));
                Some(FreeItem::OrderEntry(pid))
            }
            FreeItem::PositionManager(_) => {
                let pid = self.alloc_id();
                self.position_manager.insert(pid, PositionManagerState::new());
                Some(FreeItem::PositionManager(pid))
            }
            FreeItem::TradeLog(_) => {
                let pid = self.alloc_id();
                self.trade_log.insert(pid, TradeLogState::new());
                Some(FreeItem::TradeLog(pid))
            }
            FreeItem::RiskCalculator(_) => {
                let pid = self.alloc_id();
                self.risk_calculator.insert(pid, RiskCalculatorState::new());
                Some(FreeItem::RiskCalculator(pid))
            }
            FreeItem::TradingContainer(id) => {
                let s = self.trading_container.get(id)?;
                let pid = self.alloc_id();
                self.trading_container.insert(pid, TradingContainerState::new(
                    s.symbol.clone(), s.tick_size, s.market_price,
                ));
                Some(FreeItem::TradingContainer(pid))
            }
        }
    }

    // -------------------------------------------------------------------------
    // remove — delete state when a leaf is closed
    // -------------------------------------------------------------------------

    /// Remove the state associated with the given `FreeItem`, freeing memory.
    pub fn remove(&mut self, item: &FreeItem) {
        match item {
            FreeItem::Dom(id)              => { self.dom.remove(id); }
            FreeItem::Footprint(id)        => { self.footprint.remove(id); }
            FreeItem::VolumeProfile(id)    => { self.volume_profile.remove(id); }
            FreeItem::LiquidityHeatmap(id) => { self.liquidity_heatmap.remove(id); }
            FreeItem::BigTrades(id)        => { self.big_trades.remove(id); }
            FreeItem::L2Tape(id)           => { self.l2_tape.remove(id); }
            FreeItem::OrderEntry(id)       => { self.order_entry.remove(id); }
            FreeItem::PositionManager(id)  => { self.position_manager.remove(id); }
            FreeItem::TradeLog(id)         => { self.trade_log.remove(id); }
            FreeItem::RiskCalculator(id)   => { self.risk_calculator.remove(id); }
            FreeItem::TradingContainer(id) => { self.trading_container.remove(id); }
        }
    }

    // -------------------------------------------------------------------------
    // set_min_next_id — avoid id collisions after preset restore
    // -------------------------------------------------------------------------

    /// Ensure `next_id` is strictly greater than `min` so that freshly
    /// allocated ids will not collide with ids restored from a preset.
    pub fn set_min_next_id(&mut self, min: u64) {
        let current = self.next_id.load(Ordering::Relaxed);
        if min >= current {
            self.next_id.store(min + 1, Ordering::Relaxed);
        }
    }
}

impl Default for TradingPanelsStore {
    fn default() -> Self {
        Self::new()
    }
}
