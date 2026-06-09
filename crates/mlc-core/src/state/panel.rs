//! Unified panel taxonomy (K1 — panel enum unification).
//!
//! This module defines the single `Panel` enum used as the docking payload
//! across BOTH the outer grid (was `DockState<PanelLeaf>`) and the sidebar
//! slots (was `DockState<FreeItem>`).  It supersedes:
//!
//! - `crate::state::branch::PanelLeaf` (chart-side enum, 13 variants)
//! - `mlc_sidebar_content::free_slot::FreeItem` (sidebar enum, 14 variants)
//!
//! Owner mandate: "чарт должен стать панелью, как и индикатор панель —
//! попасть в общий enum панелей".  After K1 the chart is a sibling
//! variant of DOM / TradeTape / IndicatorPanel / etc.
//!
//! ## Owner decisions (from `audit-K1-panel-enum-unification-2026-06-09.md`)
//!
//! - **Q1** — `IndicatorPanel(PanelId)` uses the TradingPanelsStore key,
//!   uniform with every other trading panel.  The chart sub-pane case
//!   (an indicator INSIDE a chart container) lives in the separate
//!   `ChartContainerLeaf` enum and is untouched by K1.
//! - **Q2** — `Chart(ChartId)` carries the chart-container id, not the
//!   inner canvas leaf id.  From the outer tree's perspective a chart-
//!   container IS the panel.
//! - **Q3** — `TradingContainer(PanelId)` is part of the unified enum.
//!   The outer grid may host one (AcceptPolicy can restrict if needed).
//! - **Q4** — `type_id()` strings keep the `"free_*"` canon used by
//!   sidebar persistence.  Outer-dock saves that used `"panel_*"` strings
//!   get a one-off migration.
//! - **Q5** — `PanelId` lives in mlc-core (this module).  Sidebar-content
//!   re-exports it for backward-compatible imports.

use serde::{Deserialize, Serialize};

use crate::state::chart_container::ChartId;

// =============================================================================
// PanelId
// =============================================================================

/// Stable identity for a trading-panel instance across restarts.
///
/// Held inside trading variants of `Panel` instead of the heavy state type
/// so that `mlc-sidebar-content` stays free of an mlc-core dep cycle.  The
/// matching heavy state lives in `mlc-app::TradingPanelsStore` keyed by
/// `PanelId`.
///
/// Moved from `mlc-sidebar-content::free_slot::PanelId` in K1.1.  Old call
/// sites that imported it via `mlc_sidebar_content` keep working through a
/// re-export.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PanelId(pub u64);

impl std::fmt::Display for PanelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for PanelId {
    fn from(v: u64) -> Self {
        PanelId(v)
    }
}

impl From<PanelId> for u64 {
    fn from(v: PanelId) -> Self {
        v.0
    }
}

// =============================================================================
// Panel
// =============================================================================

/// The unified docking-payload enum.
///
/// One leaf node in `DockState<Panel>` for both the outer multi-chart grid
/// and each sidebar slot.  Every variant carries an identity wrapper — the
/// heavy state lives in `mlc-app::TradingPanelsStore` (trading variants and
/// IndicatorPanel) or in `mlc-core` chart storage (`Chart`).
#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    /// Chart container (1 OHLC canvas + N indicator sub-panes, all bound
    /// to one bubble).  Payload is the container's `ChartId`.
    Chart(ChartId),
    /// Standalone indicator-visualisation panel.  Distinct from a chart
    /// sub-pane (which lives inside a `Chart` via `ChartContainerLeaf`).
    IndicatorPanel(PanelId),
    Dom(PanelId),
    L2Tape(PanelId),
    TradeTape(PanelId),
    BigTrades(PanelId),
    Footprint(PanelId),
    VolumeProfile(PanelId),
    LiquidityHeatmap(PanelId),
    OrderEntry(PanelId),
    PositionManager(PanelId),
    TradeLog(PanelId),
    RiskCalculator(PanelId),
    /// Group of trading panels treated as one drag target.  Today
    /// sidebar-only; AcceptPolicy may restrict where it's allowed.
    TradingContainer(PanelId),
}

impl Panel {
    /// Discriminant — useful for dispatch tables that don't need the id.
    pub fn kind(&self) -> PanelKind {
        match self {
            Panel::Chart(_)             => PanelKind::Chart,
            Panel::IndicatorPanel(_)    => PanelKind::IndicatorPanel,
            Panel::Dom(_)               => PanelKind::Dom,
            Panel::L2Tape(_)            => PanelKind::L2Tape,
            Panel::TradeTape(_)         => PanelKind::TradeTape,
            Panel::BigTrades(_)         => PanelKind::BigTrades,
            Panel::Footprint(_)         => PanelKind::Footprint,
            Panel::VolumeProfile(_)     => PanelKind::VolumeProfile,
            Panel::LiquidityHeatmap(_)  => PanelKind::LiquidityHeatmap,
            Panel::OrderEntry(_)        => PanelKind::OrderEntry,
            Panel::PositionManager(_)   => PanelKind::PositionManager,
            Panel::TradeLog(_)          => PanelKind::TradeLog,
            Panel::RiskCalculator(_)    => PanelKind::RiskCalculator,
            Panel::TradingContainer(_)  => PanelKind::TradingContainer,
        }
    }

    /// Lowercase stable identifier (kind_str + persistence key).
    pub fn kind_str(&self) -> &'static str {
        self.kind().as_str()
    }

    /// Extract the leaf's identity as a `PanelId`.
    ///
    /// For trading variants and IndicatorPanel this is the
    /// `TradingPanelsStore` key.  For `Chart` it wraps the `ChartId.0` —
    /// the chart lookup bypasses TradingPanelsStore.
    pub fn panel_id(&self) -> PanelId {
        match self {
            Panel::Chart(cid)               => PanelId(cid.0),
            Panel::IndicatorPanel(id)
            | Panel::Dom(id)
            | Panel::L2Tape(id)
            | Panel::TradeTape(id)
            | Panel::BigTrades(id)
            | Panel::Footprint(id)
            | Panel::VolumeProfile(id)
            | Panel::LiquidityHeatmap(id)
            | Panel::OrderEntry(id)
            | Panel::PositionManager(id)
            | Panel::TradeLog(id)
            | Panel::RiskCalculator(id)
            | Panel::TradingContainer(id)   => *id,
        }
    }
}

impl uzor::panels::DockPanel for Panel {
    fn title(&self) -> &str {
        match self {
            Panel::Chart(_)             => "Chart",
            Panel::IndicatorPanel(_)    => "Indicator",
            Panel::Dom(_)               => "DOM",
            Panel::L2Tape(_)            => "L2 Tape",
            Panel::TradeTape(_)         => "Trade Tape",
            Panel::BigTrades(_)         => "Big Trades",
            Panel::Footprint(_)         => "Footprint",
            Panel::VolumeProfile(_)     => "Volume Profile",
            Panel::LiquidityHeatmap(_)  => "Liquidity Heatmap",
            Panel::OrderEntry(_)        => "Order Entry",
            Panel::PositionManager(_)   => "Positions",
            Panel::TradeLog(_)          => "Trade Log",
            Panel::RiskCalculator(_)    => "Risk Calculator",
            Panel::TradingContainer(_)  => "Trading",
        }
    }

    /// `"free_*"` canon (Q4) for both outer-grid and sidebar persistence.
    /// Outer-grid saves that used `"panel_chart_panel"` get a one-off
    /// migration during K1.2 preset-loader updates.
    fn type_id(&self) -> &'static str {
        match self {
            Panel::Chart(_)             => "free_chart",
            Panel::IndicatorPanel(_)    => "free_indicator_panel",
            Panel::Dom(_)               => "free_dom",
            Panel::L2Tape(_)            => "free_l2_tape",
            Panel::TradeTape(_)         => "free_trade_tape",
            Panel::BigTrades(_)         => "free_big_trades",
            Panel::Footprint(_)         => "free_footprint",
            Panel::VolumeProfile(_)     => "free_volume_profile",
            Panel::LiquidityHeatmap(_)  => "free_liquidity_heatmap",
            Panel::OrderEntry(_)        => "free_order_entry",
            Panel::PositionManager(_)   => "free_position_manager",
            Panel::TradeLog(_)          => "free_trade_log",
            Panel::RiskCalculator(_)    => "free_risk_calculator",
            Panel::TradingContainer(_)  => "free_trading_container",
        }
    }

    fn min_size(&self) -> (f32, f32) {
        match self {
            Panel::Chart(_)             => (240.0, 160.0),
            Panel::IndicatorPanel(_)    => (160.0, 80.0),
            Panel::OrderEntry(_)        => (180.0, 200.0),
            Panel::PositionManager(_) | Panel::TradeLog(_) => (240.0, 120.0),
            _                           => (100.0, 80.0),
        }
    }

    fn closable(&self) -> bool {
        true
    }
}

// =============================================================================
// PanelKind
// =============================================================================

/// Discriminant of `Panel`.  Used for `AcceptPolicy::Only(...)` sets and
/// for dispatch tables that don't need the id payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PanelKind {
    Chart,
    IndicatorPanel,
    Dom,
    L2Tape,
    TradeTape,
    BigTrades,
    Footprint,
    VolumeProfile,
    LiquidityHeatmap,
    OrderEntry,
    PositionManager,
    TradeLog,
    RiskCalculator,
    TradingContainer,
}

impl PanelKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PanelKind::Chart             => "chart",
            PanelKind::IndicatorPanel    => "indicator_panel",
            PanelKind::Dom               => "dom",
            PanelKind::L2Tape            => "l2_tape",
            PanelKind::TradeTape         => "trade_tape",
            PanelKind::BigTrades         => "big_trades",
            PanelKind::Footprint         => "footprint",
            PanelKind::VolumeProfile     => "volume_profile",
            PanelKind::LiquidityHeatmap  => "liquidity_heatmap",
            PanelKind::OrderEntry        => "order_entry",
            PanelKind::PositionManager   => "position_manager",
            PanelKind::TradeLog          => "trade_log",
            PanelKind::RiskCalculator    => "risk_calculator",
            PanelKind::TradingContainer  => "trading_container",
        }
    }
}
