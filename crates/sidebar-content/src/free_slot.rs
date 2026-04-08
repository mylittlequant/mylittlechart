//! Per-slot docking grid types for the free-slot hyperspace (Slot1..Slot4).
//!
//! Each of the 4 slot sidebars hosts its own `DockingManager<FreeItem>`. The
//! set `{Main, Slot1..Slot4}` forms a single cross-container drag hyperspace
//! in Phase 3-new, with the restriction that `FreeItem::Chart(_)` is locked to
//! Main. See [`docs/plans/sidebar-containers-docking.md`] for the full model.

// =============================================================================
// PanelId
// =============================================================================

/// Stable identity for a trading panel instance across restarts.
///
/// Held inside each `FreeItem` variant instead of the heavy state type so
/// that sidebar-content stays free of a `zengeld-panels` dependency. The
/// actual state lives in `chart-app`'s `TradingPanelsStore`, keyed by
/// `PanelId`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PanelId(pub u64);

// =============================================================================
// FreeItem
// =============================================================================

/// Payload stored in each leaf of a `DockingManager<FreeItem>`.
///
/// Separated from `AgentPaneLeaf` by type so that Agents panes can never leak
/// into the free-slot hyperspace and trading panels can never leak into the
/// Agents sidebar.
///
/// Each variant carries only a `PanelId` (u64 wrapper). The matching heavy
/// state lives in `chart-app::TradingPanelsStore` keyed by that id.
#[derive(Clone, Debug)]
pub enum FreeItem {
    Dom(PanelId),
    Footprint(PanelId),
    VolumeProfile(PanelId),
    LiquidityHeatmap(PanelId),
    BigTrades(PanelId),
    L2Tape(PanelId),
    OrderEntry(PanelId),
    PositionManager(PanelId),
    TradeLog(PanelId),
    RiskCalculator(PanelId),
    TradingContainer(PanelId),
}

impl FreeItem {
    /// Extract the `PanelId` from whichever variant is active.
    pub fn panel_id(&self) -> PanelId {
        match self {
            FreeItem::Dom(id)
            | FreeItem::Footprint(id)
            | FreeItem::VolumeProfile(id)
            | FreeItem::LiquidityHeatmap(id)
            | FreeItem::BigTrades(id)
            | FreeItem::L2Tape(id)
            | FreeItem::OrderEntry(id)
            | FreeItem::PositionManager(id)
            | FreeItem::TradeLog(id)
            | FreeItem::RiskCalculator(id)
            | FreeItem::TradingContainer(id) => *id,
        }
    }

    /// Short stable string identifying the variant (used as `type_id` and persistence key).
    pub fn kind_str(&self) -> &'static str {
        match self {
            FreeItem::Dom(_)               => "dom",
            FreeItem::Footprint(_)         => "footprint",
            FreeItem::VolumeProfile(_)     => "volume_profile",
            FreeItem::LiquidityHeatmap(_)  => "liquidity_heatmap",
            FreeItem::BigTrades(_)         => "big_trades",
            FreeItem::L2Tape(_)            => "l2_tape",
            FreeItem::OrderEntry(_)        => "order_entry",
            FreeItem::PositionManager(_)   => "position_manager",
            FreeItem::TradeLog(_)          => "trade_log",
            FreeItem::RiskCalculator(_)    => "risk_calculator",
            FreeItem::TradingContainer(_)  => "trading_container",
        }
    }
}

impl uzor::panels::DockPanel for FreeItem {
    fn title(&self) -> &str {
        match self {
            FreeItem::Dom(_)               => "DOM",
            FreeItem::Footprint(_)         => "Footprint",
            FreeItem::VolumeProfile(_)     => "Volume Profile",
            FreeItem::LiquidityHeatmap(_)  => "Liquidity Heatmap",
            FreeItem::BigTrades(_)         => "Big Trades",
            FreeItem::L2Tape(_)            => "L2 Tape",
            FreeItem::OrderEntry(_)        => "Order Entry",
            FreeItem::PositionManager(_)   => "Positions",
            FreeItem::TradeLog(_)          => "Trade Log",
            FreeItem::RiskCalculator(_)    => "Risk Calculator",
            FreeItem::TradingContainer(_)  => "Trading",
        }
    }

    /// Returns a stable, zero-allocation type identifier for the variant kind.
    ///
    /// The `panel_id` is intentionally NOT encoded here — panel identity is
    /// recovered during restore via `LayoutSnapshot::restore_tree_with_id`,
    /// which passes the leaf node id to the factory closure so it can look up
    /// the matching `PersistedFreeLeaf` by `leaf_id`.
    fn type_id(&self) -> &'static str {
        match self {
            FreeItem::Dom(_)               => "free_dom",
            FreeItem::Footprint(_)         => "free_footprint",
            FreeItem::VolumeProfile(_)     => "free_volume_profile",
            FreeItem::LiquidityHeatmap(_)  => "free_liquidity_heatmap",
            FreeItem::BigTrades(_)         => "free_big_trades",
            FreeItem::L2Tape(_)            => "free_l2_tape",
            FreeItem::OrderEntry(_)        => "free_order_entry",
            FreeItem::PositionManager(_)   => "free_position_manager",
            FreeItem::TradeLog(_)          => "free_trade_log",
            FreeItem::RiskCalculator(_)    => "free_risk_calculator",
            FreeItem::TradingContainer(_)  => "free_trading_container",
        }
    }

    /// Minimum panel size in pixels (width, height).
    /// Values are read from each panel wrapper's `min_size()` method in `zengeld-panels`.
    fn min_size(&self) -> (f32, f32) {
        match self {
            FreeItem::Dom(_)               => (200.0, 150.0),
            FreeItem::Footprint(_)         => (300.0, 200.0),
            FreeItem::VolumeProfile(_)     => (200.0, 300.0),
            FreeItem::LiquidityHeatmap(_)  => (300.0, 200.0),
            FreeItem::BigTrades(_)         => (250.0, 200.0),
            FreeItem::L2Tape(_)            => (200.0, 150.0),
            FreeItem::OrderEntry(_)        => (250.0, 300.0),
            FreeItem::PositionManager(_)   => (300.0, 150.0),
            FreeItem::TradeLog(_)          => (200.0, 150.0),
            FreeItem::RiskCalculator(_)    => (250.0, 200.0),
            FreeItem::TradingContainer(_)  => (400.0, 300.0),
        }
    }

    fn closable(&self) -> bool {
        true
    }
}

// =============================================================================
// SlotDockingManager — Clone/Debug wrapper
// =============================================================================

/// Newtype around `DockingManager<FreeItem>` providing manual `Clone` + `Debug`
/// so it can live inside the `#[derive]`-d `SidebarState`.
///
/// `Clone` returns an empty manager — same policy as `AgentDockingManager`,
/// used only for `SidebarState` snapshot/undo scenarios where a clean slate
/// is desired.
pub struct SlotDockingManager(pub uzor::panels::DockingManager<FreeItem>);

impl SlotDockingManager {
    pub fn new() -> Self {
        Self(uzor::panels::DockingManager::new())
    }

    pub fn inner(&self) -> &uzor::panels::DockingManager<FreeItem> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut uzor::panels::DockingManager<FreeItem> {
        &mut self.0
    }
}

impl Default for SlotDockingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SlotDockingManager {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SlotDockingManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotDockingManager").finish_non_exhaustive()
    }
}
