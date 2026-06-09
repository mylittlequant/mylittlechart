//! Per-slot docking grid types for the free-slot hyperspace (Slot1..Slot4).
//!
//! Each of the 4 slot sidebars hosts its own `DockingManager<FreeItem>`. The
//! set `{Main, Slot1..Slot4}` forms a single cross-container drag hyperspace.
//!
//! `FreeItem::Chart(u64)` wraps `ChartId.0` (raw `u64` to keep
//! `sidebar-content` free of a `zengeld-chart` dependency).  Conversion
//! happens at the `chart-app` boundary.
//!
//! `FreeItem::IndicatorPanel(u64)` is a standalone indicator-visualization
//! panel (not the chart-internal `SubPane`).  IndicatorPanel carries its
//! own symbol/exchange/account_type + sync-group membership + viewport,
//! like DOM / TradeTape / etc.  Wraps `PanelId.0` from `TradingPanelsStore`.
//!
//! See `docs/plans/chart-subpane-pid-refactor-2026-06-03.md`.

// =============================================================================
// PanelId — re-exported from mlc-core (K1 single source of truth)
// =============================================================================

/// Re-export of `mlc_core::state::PanelId`.  Existing
/// `mlc_sidebar_content::free_slot::PanelId` imports keep compiling; new
/// code should import from `mlc_core::state` directly.
pub use mlc_core::state::PanelId;

// =============================================================================
// FreeItem
// =============================================================================

/// Payload stored in each leaf of a `DockingManager<FreeItem>`.
///
/// Separated from `AgentPaneLeaf` by type so that Agents panes can never leak
/// into the free-slot hyperspace and trading panels can never leak into the
/// Agents sidebar.
///
/// Most variants carry only a `PanelId` (u64 wrapper).  The matching heavy
/// state lives in `chart-app::TradingPanelsStore` keyed by that id.
///
/// `Chart` is an exception: wraps raw `ChartId.0`.  Raw `u64` avoids a
/// cross-crate type dependency from `sidebar-content` to `zengeld-chart`;
/// conversion happens at the `chart-app` boundary, where the chart leaf
/// payload is resolved against the chart-window storage.
#[derive(Clone, Debug)]
pub enum FreeItem {
    Dom(PanelId),
    Footprint(PanelId),
    VolumeProfile(PanelId),
    LiquidityHeatmap(PanelId),
    BigTrades(PanelId),
    L2Tape(PanelId),
    TradeTape(PanelId),
    OrderEntry(PanelId),
    PositionManager(PanelId),
    TradeLog(PanelId),
    RiskCalculator(PanelId),
    TradingContainer(PanelId),
    /// Embedded chart leaf — wraps `ChartId.0`.  Resolved at the chart-app
    /// boundary against the chart-window storage.
    Chart(u64),
    /// Standalone indicator-visualization panel — wraps `PanelId.0` from
    /// `TradingPanelsStore`.  Carries its own symbol / exchange /
    /// account_type + viewport + sync-group membership, independent of any
    /// parent chart.  Distinct from chart-internal `SubPane`.
    IndicatorPanel(PanelId),
}

impl FreeItem {
    /// Extract the leaf payload's identity as a `PanelId` u64 wrapper.
    ///
    /// For trading-panel variants and `IndicatorPanel` this returns the
    /// real `PanelId` stored in `TradingPanelsStore`.  For `Chart` it wraps
    /// the `ChartId.0` — the chart lookup bypasses the `TradingPanelsStore`
    /// panel HashMaps.  Callers distinguish by `kind_str()` or by matching
    /// the variant directly.
    pub fn panel_id(&self) -> PanelId {
        match self {
            FreeItem::Dom(id)
            | FreeItem::Footprint(id)
            | FreeItem::VolumeProfile(id)
            | FreeItem::LiquidityHeatmap(id)
            | FreeItem::BigTrades(id)
            | FreeItem::L2Tape(id)
            | FreeItem::TradeTape(id)
            | FreeItem::OrderEntry(id)
            | FreeItem::PositionManager(id)
            | FreeItem::TradeLog(id)
            | FreeItem::RiskCalculator(id)
            | FreeItem::TradingContainer(id)
            | FreeItem::IndicatorPanel(id) => *id,
            FreeItem::Chart(raw) => PanelId(*raw),
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
            FreeItem::TradeTape(_)         => "trade_tape",
            FreeItem::OrderEntry(_)        => "order_entry",
            FreeItem::PositionManager(_)   => "position_manager",
            FreeItem::TradeLog(_)          => "trade_log",
            FreeItem::RiskCalculator(_)    => "risk_calculator",
            FreeItem::TradingContainer(_)  => "trading_container",
            FreeItem::Chart(_)             => "chart",
            FreeItem::IndicatorPanel(_)    => "indicator_panel",
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
            FreeItem::TradeTape(_)         => "Trade Tape",
            FreeItem::OrderEntry(_)        => "Order Entry",
            FreeItem::PositionManager(_)   => "Positions",
            FreeItem::TradeLog(_)          => "Trade Log",
            FreeItem::RiskCalculator(_)    => "Risk Calculator",
            FreeItem::TradingContainer(_)  => "Trading",
            FreeItem::Chart(_)             => "Chart",
            FreeItem::IndicatorPanel(_)    => "Indicator",
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
            FreeItem::TradeTape(_)         => "free_trade_tape",
            FreeItem::OrderEntry(_)        => "free_order_entry",
            FreeItem::PositionManager(_)   => "free_position_manager",
            FreeItem::TradeLog(_)          => "free_trade_log",
            FreeItem::RiskCalculator(_)    => "free_risk_calculator",
            FreeItem::TradingContainer(_)  => "free_trading_container",
            FreeItem::Chart(_)             => "free_chart",
            FreeItem::IndicatorPanel(_)    => "free_indicator_panel",
        }
    }

    /// Minimum panel size in pixels (width, height).
    ///
    /// Uniform 100x80 for most variants — enough for a header strip + a
    /// couple of data rows.  Chart and IndicatorPanel need more room for
    /// their price-axis / time-axis rendering to remain legible.
    fn min_size(&self) -> (f32, f32) {
        match self {
            FreeItem::Chart(_)          => (200.0, 150.0),
            FreeItem::IndicatorPanel(_) => (160.0, 80.0),
            _                           => (100.0, 80.0),
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
pub struct SlotDockingManager(pub uzor::layout::DockState<FreeItem>);

impl SlotDockingManager {
    pub fn new() -> Self {
        Self(uzor::layout::DockState::new())
    }

    pub fn inner(&self) -> &uzor::layout::DockState<FreeItem> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut uzor::layout::DockState<FreeItem> {
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
