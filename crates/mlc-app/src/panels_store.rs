//! State store for the 11 trading panels that live inside the free-slot
//! sidebar hyperspace (Slot1..Slot4).
//!
//! `TradingPanelsStore` is chart-app-owned and holds heavy state keyed by
//! `PanelId`. The lightweight sidebar docking trees hold only `Panel`
//! variants that carry the matching `PanelId`, keeping `sidebar-content`
//! free of any `zengeld-panels` dependency.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use mlc_core::state::{PanelId, Panel};
use mlc_trading_manager::SharedTradingSnapshot;
use mlc_core::state::ChartPanelId;
use mlc_core::ChartPanelState;

use mlc_panels::panel_trait::TradingPanel;
use mlc_panels::trading::order_flow::{
    big_trades::BigTradesState,
    dom::DomState,
    footprint::FootprintState,
    indicator_panel::IndicatorPanelState,
    l2_tape::L2TapeState,
    liquidity_heatmap::LiquidityHeatmapState,
    multi_dom::{MultiDomConfig, MultiDomState},
    trade_tape::TradeTapeState,
    volume_profile::VolumeProfileState,
};
use mlc_panels::trading::trading::{
    order_entry::OrderEntryState,
    position_manager::PositionManagerState,
    risk_calculator::RiskCalculatorState,
    trade_log::TradeLogState,
    trading_container::TradingContainerState,
};

/// Owns the heavy state for all 11 trading panel kinds.
///
/// All state maps are keyed by `PanelId`. The `next_id` counter is used to
/// allocate fresh ids; after restoring from a preset, call `set_min_next_id`
/// to prevent collisions with persisted ids.
pub struct TradingPanelsStore {
    next_id: AtomicU64,
    trading_snapshot: Option<SharedTradingSnapshot>,
    pub dom: HashMap<PanelId, DomState>,
    pub footprint: HashMap<PanelId, FootprintState>,
    pub volume_profile: HashMap<PanelId, VolumeProfileState>,
    pub liquidity_heatmap: HashMap<PanelId, LiquidityHeatmapState>,
    /// Multi-exchange consolidated DOM panels.
    pub multi_dom: HashMap<PanelId, MultiDomState>,
    pub big_trades: HashMap<PanelId, BigTradesState>,
    pub l2_tape: HashMap<PanelId, L2TapeState>,
    pub trade_tape: HashMap<PanelId, TradeTapeState>,
    pub order_entry: HashMap<PanelId, OrderEntryState>,
    pub position_manager: HashMap<PanelId, PositionManagerState>,
    pub trade_log: HashMap<PanelId, TradeLogState>,
    pub risk_calculator: HashMap<PanelId, RiskCalculatorState>,
    pub trading_container: HashMap<PanelId, TradingContainerState>,
    /// Standalone indicator-visualization panels.  See
    /// `mlc_panels::trading::order_flow::indicator_panel`.
    pub indicator_panel: HashMap<PanelId, IndicatorPanelState>,

    /// Phase 4 / S2 chart canvas state, keyed by [`ChartPanelId`].
    ///
    /// Empty in S2.1. Subsequent S2.X commits migrate canvas-leaf fields
    /// (bars / viewport / scales / drawing_manager / display options / …)
    /// out of `ChartContainer` into [`ChartPanelState`].
    pub chart_panel: HashMap<ChartPanelId, ChartPanelState>,
}

impl TradingPanelsStore {
    /// Create an empty store. Counter starts at 1 (0 is reserved for "null").
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            trading_snapshot: None,
            dom: HashMap::new(),
            footprint: HashMap::new(),
            volume_profile: HashMap::new(),
            liquidity_heatmap: HashMap::new(),
            multi_dom: HashMap::new(),
            big_trades: HashMap::new(),
            l2_tape: HashMap::new(),
            trade_tape: HashMap::new(),
            order_entry: HashMap::new(),
            position_manager: HashMap::new(),
            trade_log: HashMap::new(),
            risk_calculator: HashMap::new(),
            trading_container: HashMap::new(),
            indicator_panel: HashMap::new(),
            chart_panel: HashMap::new(),
        }
    }

    fn alloc_id(&self) -> PanelId {
        PanelId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    pub fn set_trading_snapshot(&mut self, snap: SharedTradingSnapshot) {
        for state in self.order_entry.values_mut() {
            state.set_snapshot(snap.clone());
        }
        for state in self.position_manager.values_mut() {
            state.set_snapshot(snap.clone());
        }
        for state in self.trade_log.values_mut() {
            state.set_snapshot(snap.clone());
        }
        self.trading_snapshot = Some(snap);
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

    /// Allocate a new Multi-Exchange DOM panel.
    ///
    /// `exchanges` is the ordered list of exchange labels to track.
    /// The panel starts with an empty book for each exchange; the caller
    /// is responsible for wiring live-data subscriptions.
    pub fn create_multi_dom(&mut self, symbol: String, exchanges: Vec<String>) -> PanelId {
        let id = self.alloc_id();
        let config = MultiDomConfig {
            exchanges,
            grid_size: 0.5,
            depth_per_side: 30,
        };
        self.multi_dom.insert(id, MultiDomState::new(symbol, config));
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

    /// Allocate a new Trade Tape panel.
    pub fn create_trade_tape(&mut self) -> PanelId {
        let id = self.alloc_id();
        self.trade_tape.insert(id, TradeTapeState::new());
        id
    }

    /// Allocate a new Order Entry panel.
    pub fn create_order_entry(&mut self, symbol: String) -> PanelId {
        let id = self.alloc_id();
        let mut state = OrderEntryState::new(symbol);
        if let Some(snap) = &self.trading_snapshot {
            state.set_snapshot(snap.clone());
        }
        self.order_entry.insert(id, state);
        id
    }

    /// Allocate a new Position Manager panel.
    pub fn create_position_manager(&mut self) -> PanelId {
        let id = self.alloc_id();
        let mut state = PositionManagerState::new();
        if let Some(snap) = &self.trading_snapshot {
            state.set_snapshot(snap.clone());
        }
        self.position_manager.insert(id, state);
        id
    }

    /// Allocate a new Trade Log panel.
    pub fn create_trade_log(&mut self) -> PanelId {
        let id = self.alloc_id();
        let mut state = TradeLogState::new();
        if let Some(snap) = &self.trading_snapshot {
            state.set_snapshot(snap.clone());
        }
        self.trade_log.insert(id, state);
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

    /// Allocate a new standalone IndicatorPanel.  `instance_id == 0` is
    /// allowed (panel renders an "unbound" placeholder until the user picks
    /// an indicator).
    pub fn create_indicator_panel(&mut self, symbol: String, instance_id: u64) -> PanelId {
        let id = self.alloc_id();
        self.indicator_panel.insert(id, IndicatorPanelState::with_instance(symbol, instance_id));
        id
    }

    // -------------------------------------------------------------------------
    // get_panel / get_panel_mut — trait-object access for migrated panels
    // -------------------------------------------------------------------------

    /// Get an immutable reference to a panel's `TradingPanel` impl, if migrated.
    /// Returns `None` for panels not yet migrated to the trait.
    ///
    /// `Panel::Chart` is NOT a trading panel — it has a dedicated render
    /// path that resolves through `ChartPanelGrid` / chart-window storage.
    /// `Panel::IndicatorPanel` IS a trading panel — it implements the
    /// `TradingPanel` trait like DOM/TradeTape/etc. and is dispatched
    /// through this method normally.
    pub fn get_panel(&self, item: &Panel) -> Option<&dyn TradingPanel> {
        match item {
            Panel::Dom(id) => self.dom.get(id).map(|s| s as &dyn TradingPanel),
            Panel::MultiDom(id) => self.multi_dom.get(id).map(|s| s as &dyn TradingPanel),
            Panel::Footprint(id) => self.footprint.get(id).map(|s| s as &dyn TradingPanel),
            Panel::VolumeProfile(id) => self.volume_profile.get(id).map(|s| s as &dyn TradingPanel),
            Panel::LiquidityHeatmap(id) => self.liquidity_heatmap.get(id).map(|s| s as &dyn TradingPanel),
            Panel::BigTrades(id) => self.big_trades.get(id).map(|s| s as &dyn TradingPanel),
            Panel::L2Tape(id) => self.l2_tape.get(id).map(|s| s as &dyn TradingPanel),
            Panel::TradeTape(id) => self.trade_tape.get(id).map(|s| s as &dyn TradingPanel),
            Panel::OrderEntry(id) => self.order_entry.get(id).map(|s| s as &dyn TradingPanel),
            Panel::PositionManager(id) => self.position_manager.get(id).map(|s| s as &dyn TradingPanel),
            Panel::TradeLog(id) => self.trade_log.get(id).map(|s| s as &dyn TradingPanel),
            Panel::RiskCalculator(id) => self.risk_calculator.get(id).map(|s| s as &dyn TradingPanel),
            Panel::TradingContainer(id) => self.trading_container.get(id).map(|s| s as &dyn TradingPanel),
            Panel::IndicatorPanel(id) => self.indicator_panel.get(id).map(|s| s as &dyn TradingPanel),
            Panel::Chart(_) => None,
        }
    }

    /// Get a mutable reference to a panel's `TradingPanel` impl, if migrated.
    /// `Panel::Chart` returns `None` — see [`get_panel`].
    pub fn get_panel_mut(&mut self, item: &Panel) -> Option<&mut dyn TradingPanel> {
        match item {
            Panel::Dom(id) => self.dom.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::MultiDom(id) => self.multi_dom.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::Footprint(id) => self.footprint.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::VolumeProfile(id) => self.volume_profile.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::LiquidityHeatmap(id) => self.liquidity_heatmap.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::BigTrades(id) => self.big_trades.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::L2Tape(id) => self.l2_tape.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::TradeTape(id) => self.trade_tape.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::OrderEntry(id) => self.order_entry.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::PositionManager(id) => self.position_manager.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::TradeLog(id) => self.trade_log.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::RiskCalculator(id) => self.risk_calculator.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::TradingContainer(id) => self.trading_container.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::IndicatorPanel(id) => self.indicator_panel.get_mut(id).map(|s| s as &mut dyn TradingPanel),
            Panel::Chart(_) => None,
        }
    }

    // -------------------------------------------------------------------------
    // clone_item — duplicate a Panel with fresh PanelId + copied config
    // -------------------------------------------------------------------------

    /// Create a new `Panel` of the same variant as `source`, with a fresh
    /// `PanelId` and state cloned from the source's config (symbol, tick_size,
    /// etc.). Returns `None` if the source's state is missing from the store.
    pub fn clone_item(&mut self, source: &Panel) -> Option<Panel> {
        match source {
            Panel::Dom(id) => {
                let s = self.dom.get(id)?;
                let pid = self.alloc_id();
                self.dom.insert(pid, DomState::new(s.symbol.clone(), s.tick_size));
                Some(Panel::Dom(pid))
            }
            Panel::MultiDom(id) => {
                let s = self.multi_dom.get(id)?;
                let pid = self.alloc_id();
                self.multi_dom.insert(pid, MultiDomState::new(s.symbol.clone(), s.config.clone()));
                Some(Panel::MultiDom(pid))
            }
            Panel::Footprint(id) => {
                let s = self.footprint.get(id)?;
                let pid = self.alloc_id();
                self.footprint.insert(pid, FootprintState::new(s.symbol.clone(), s.tick_size));
                Some(Panel::Footprint(pid))
            }
            Panel::VolumeProfile(id) => {
                let s = self.volume_profile.get(id)?;
                let pid = self.alloc_id();
                self.volume_profile.insert(pid, VolumeProfileState::new(s.symbol.clone(), s.tick_size));
                Some(Panel::VolumeProfile(pid))
            }
            Panel::LiquidityHeatmap(id) => {
                let s = self.liquidity_heatmap.get(id)?;
                let pid = self.alloc_id();
                self.liquidity_heatmap.insert(pid, LiquidityHeatmapState::new(
                    s.symbol.clone(), s.tick_size, s.snapshot_interval_ms,
                ));
                Some(Panel::LiquidityHeatmap(pid))
            }
            Panel::BigTrades(_) => {
                let pid = self.alloc_id();
                self.big_trades.insert(pid, BigTradesState::new());
                Some(Panel::BigTrades(pid))
            }
            Panel::L2Tape(_) => {
                let pid = self.alloc_id();
                self.l2_tape.insert(pid, L2TapeState::new());
                Some(Panel::L2Tape(pid))
            }
            Panel::TradeTape(_) => {
                let pid = self.alloc_id();
                self.trade_tape.insert(pid, TradeTapeState::new());
                Some(Panel::TradeTape(pid))
            }
            Panel::OrderEntry(id) => {
                let s = self.order_entry.get(id)?;
                let pid = self.alloc_id();
                let mut state = OrderEntryState::new(s.symbol.clone());
                if let Some(snap) = &self.trading_snapshot {
                    state.set_snapshot(snap.clone());
                }
                self.order_entry.insert(pid, state);
                Some(Panel::OrderEntry(pid))
            }
            Panel::PositionManager(_) => {
                let pid = self.alloc_id();
                let mut state = PositionManagerState::new();
                if let Some(snap) = &self.trading_snapshot {
                    state.set_snapshot(snap.clone());
                }
                self.position_manager.insert(pid, state);
                Some(Panel::PositionManager(pid))
            }
            Panel::TradeLog(_) => {
                let pid = self.alloc_id();
                let mut state = TradeLogState::new();
                if let Some(snap) = &self.trading_snapshot {
                    state.set_snapshot(snap.clone());
                }
                self.trade_log.insert(pid, state);
                Some(Panel::TradeLog(pid))
            }
            Panel::RiskCalculator(_) => {
                let pid = self.alloc_id();
                self.risk_calculator.insert(pid, RiskCalculatorState::new());
                Some(Panel::RiskCalculator(pid))
            }
            Panel::TradingContainer(id) => {
                let s = self.trading_container.get(id)?;
                let pid = self.alloc_id();
                self.trading_container.insert(pid, TradingContainerState::new(
                    s.symbol.clone(), s.tick_size, s.market_price,
                ));
                Some(Panel::TradingContainer(pid))
            }
            Panel::IndicatorPanel(id) => {
                let s = self.indicator_panel.get(id)?;
                let pid = self.alloc_id();
                let mut clone = IndicatorPanelState::with_instance(
                    s.symbol.clone(),
                    s.instance_id,
                );
                clone.exchange = s.exchange.clone();
                clone.account_type = s.account_type.clone();
                clone.indicator_config_id = s.indicator_config_id;
                clone.bar_spacing = s.bar_spacing;
                clone.view_position = s.view_position;
                clone.price_min = s.price_min;
                clone.price_max = s.price_max;
                clone.auto_scale = s.auto_scale;
                clone.height_ratio = s.height_ratio;
                self.indicator_panel.insert(pid, clone);
                Some(Panel::IndicatorPanel(pid))
            }
            // Chart cloning is not yet wired — Stage 8 (central area cleanup)
            // will give panels_store ownership of ChartWindows.  For now:
            // refuse to clone a Chart leaf.
            Panel::Chart(_) => None,
        }
    }

    // -------------------------------------------------------------------------
    // remove — delete state when a leaf is closed
    // -------------------------------------------------------------------------

    /// Remove the state associated with the given `Panel`, freeing memory.
    pub fn remove(&mut self, item: &Panel) {
        match item {
            Panel::Dom(id)              => { self.dom.remove(id); }
            Panel::MultiDom(id)         => { self.multi_dom.remove(id); }
            Panel::Footprint(id)        => { self.footprint.remove(id); }
            Panel::VolumeProfile(id)    => { self.volume_profile.remove(id); }
            Panel::LiquidityHeatmap(id) => { self.liquidity_heatmap.remove(id); }
            Panel::BigTrades(id)        => { self.big_trades.remove(id); }
            Panel::L2Tape(id)           => { self.l2_tape.remove(id); }
            Panel::TradeTape(id)        => { self.trade_tape.remove(id); }
            Panel::OrderEntry(id)       => { self.order_entry.remove(id); }
            Panel::PositionManager(id)  => { self.position_manager.remove(id); }
            Panel::TradeLog(id)         => { self.trade_log.remove(id); }
            Panel::RiskCalculator(id)   => { self.risk_calculator.remove(id); }
            Panel::TradingContainer(id) => { self.trading_container.remove(id); }
            Panel::IndicatorPanel(id)   => { self.indicator_panel.remove(id); }
            // Chart removal is not yet wired — Stage 8 (central area cleanup)
            // will give panels_store ownership of ChartWindows.  For now:
            // no-op so closing a chart leaf doesn't crash.
            Panel::Chart(_) => {}
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

