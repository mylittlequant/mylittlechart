//! Branch-level docking payload types.
//!
//! Defines `PanelLeaf` (unified docking payload), `PanelLeafKind`
//! (discriminant), and `InstanceId`.  `BranchMeta`, `BranchKind`, and
//! `AcceptPolicy` were deleted in Phase 1.4a of the DataBubble
//! migration — their data now lives on `DataBubble`.

use serde::{Deserialize, Serialize};

use crate::state::chart_container_leaf::ChartPanelId;

// =============================================================================
// Identifiers
// =============================================================================

/// Identity of one indicator instance.
///
/// Thin newtype over the indicator manager's `instance_id: u64` — the
/// canonical cross-subsystem key for an indicator that already
/// underlies sub-pane reconciliation, sync group propagation and
/// preset persistence.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceId(pub u64);

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for InstanceId {
    fn from(v: u64) -> Self {
        InstanceId(v)
    }
}

impl From<InstanceId> for u64 {
    fn from(v: InstanceId) -> Self {
        v.0
    }
}

// =============================================================================
// PanelLeaf — unified docking payload
// =============================================================================

/// Payload of one leaf node in the unified docking tree.
///
/// Each variant carries the identity of the panel state stored
/// elsewhere (in `panels_store` for trading panels, in
/// `panels_store.chart_panel` for chart canvases after S2 migration).
/// The leaf itself is a typed pointer.
///
/// Variant set parallels `mlc_sidebar_content::free_slot::FreeItem` for
/// the trading-panel kinds, plus the chart-specific
/// `ChartPanel` / `IndicatorPanel` Phase 4 entries.  S8 may merge the
/// two enums; until then they coexist.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelLeaf {
    /// OHLC canvas — bars, viewport, drawings, primitives.
    ChartPanel(ChartPanelId),
    /// Indicator visualization — chart sub-pane OR standalone panel.
    /// Same render path; data source differs only by viewport inherit.
    IndicatorPanel(InstanceId),
    Dom(u64),
    L2Tape(u64),
    TradeTape(u64),
    BigTrades(u64),
    Footprint(u64),
    VolumeProfile(u64),
    LiquidityHeatmap(u64),
    OrderEntry(u64),
    PositionManager(u64),
    TradeLog(u64),
    RiskCalculator(u64),
}

impl PanelLeaf {
    /// Discriminant — useful for rendering dispatch tables.
    pub fn kind(&self) -> PanelLeafKind {
        match self {
            PanelLeaf::ChartPanel(_) => PanelLeafKind::ChartPanel,
            PanelLeaf::IndicatorPanel(_) => PanelLeafKind::IndicatorPanel,
            PanelLeaf::Dom(_) => PanelLeafKind::Dom,
            PanelLeaf::L2Tape(_) => PanelLeafKind::L2Tape,
            PanelLeaf::TradeTape(_) => PanelLeafKind::TradeTape,
            PanelLeaf::BigTrades(_) => PanelLeafKind::BigTrades,
            PanelLeaf::Footprint(_) => PanelLeafKind::Footprint,
            PanelLeaf::VolumeProfile(_) => PanelLeafKind::VolumeProfile,
            PanelLeaf::LiquidityHeatmap(_) => PanelLeafKind::LiquidityHeatmap,
            PanelLeaf::OrderEntry(_) => PanelLeafKind::OrderEntry,
            PanelLeaf::PositionManager(_) => PanelLeafKind::PositionManager,
            PanelLeaf::TradeLog(_) => PanelLeafKind::TradeLog,
            PanelLeaf::RiskCalculator(_) => PanelLeafKind::RiskCalculator,
        }
    }

    /// Lowercase stable identifier — matches the `kind_str` convention
    /// of `FreeItem` so existing routing helpers keep working when
    /// S2/S8 unifies the two enums.
    pub fn kind_str(&self) -> &'static str {
        self.kind().as_str()
    }
}

impl uzor::panels::DockPanel for PanelLeaf {
    fn title(&self) -> &str {
        match self {
            PanelLeaf::ChartPanel(_) => "Chart",
            PanelLeaf::IndicatorPanel(_) => "Indicator",
            PanelLeaf::Dom(_) => "DOM",
            PanelLeaf::L2Tape(_) => "L2 Tape",
            PanelLeaf::TradeTape(_) => "Trade Tape",
            PanelLeaf::BigTrades(_) => "Big Trades",
            PanelLeaf::Footprint(_) => "Footprint",
            PanelLeaf::VolumeProfile(_) => "Volume Profile",
            PanelLeaf::LiquidityHeatmap(_) => "Liquidity Heatmap",
            PanelLeaf::OrderEntry(_) => "Order Entry",
            PanelLeaf::PositionManager(_) => "Positions",
            PanelLeaf::TradeLog(_) => "Trade Log",
            PanelLeaf::RiskCalculator(_) => "Risk",
        }
    }

    fn type_id(&self) -> &'static str {
        match self {
            PanelLeaf::ChartPanel(_) => "panel_chart_panel",
            PanelLeaf::IndicatorPanel(_) => "panel_indicator_panel",
            PanelLeaf::Dom(_) => "panel_dom",
            PanelLeaf::L2Tape(_) => "panel_l2_tape",
            PanelLeaf::TradeTape(_) => "panel_trade_tape",
            PanelLeaf::BigTrades(_) => "panel_big_trades",
            PanelLeaf::Footprint(_) => "panel_footprint",
            PanelLeaf::VolumeProfile(_) => "panel_volume_profile",
            PanelLeaf::LiquidityHeatmap(_) => "panel_liquidity_heatmap",
            PanelLeaf::OrderEntry(_) => "panel_order_entry",
            PanelLeaf::PositionManager(_) => "panel_position_manager",
            PanelLeaf::TradeLog(_) => "panel_trade_log",
            PanelLeaf::RiskCalculator(_) => "panel_risk_calculator",
        }
    }

    fn min_size(&self) -> (f32, f32) {
        match self {
            PanelLeaf::ChartPanel(_) => (240.0, 160.0),
            PanelLeaf::IndicatorPanel(_) => (160.0, 60.0),
            PanelLeaf::OrderEntry(_) => (180.0, 200.0),
            PanelLeaf::PositionManager(_) | PanelLeaf::TradeLog(_) => (240.0, 120.0),
            _ => (160.0, 80.0),
        }
    }

    fn closable(&self) -> bool {
        true
    }
}

/// Discriminant of `PanelLeaf`.  Used for `AcceptPolicy::Only(...)`
/// sets and for dispatch tables that don't need the id payload.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum PanelLeafKind {
    ChartPanel,
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
}

impl PanelLeafKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            PanelLeafKind::ChartPanel => "chart_panel",
            PanelLeafKind::IndicatorPanel => "indicator_panel",
            PanelLeafKind::Dom => "dom",
            PanelLeafKind::L2Tape => "l2_tape",
            PanelLeafKind::TradeTape => "trade_tape",
            PanelLeafKind::BigTrades => "big_trades",
            PanelLeafKind::Footprint => "footprint",
            PanelLeafKind::VolumeProfile => "volume_profile",
            PanelLeafKind::LiquidityHeatmap => "liquidity_heatmap",
            PanelLeafKind::OrderEntry => "order_entry",
            PanelLeafKind::PositionManager => "position_manager",
            PanelLeafKind::TradeLog => "trade_log",
            PanelLeafKind::RiskCalculator => "risk_calculator",
        }
    }
}


// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_str_matches_kind_enum() {
        assert_eq!(
            PanelLeaf::ChartPanel(ChartPanelId(1)).kind(),
            PanelLeafKind::ChartPanel
        );
        assert_eq!(
            PanelLeaf::IndicatorPanel(InstanceId(7)).kind_str(),
            "indicator_panel"
        );
    }

    #[test]
    fn instance_id_round_trip_u64() {
        let id: InstanceId = 42_u64.into();
        let raw: u64 = id.into();
        assert_eq!(raw, 42);
    }

    #[test]
    fn dock_panel_min_size_per_kind() {
        use uzor::panels::DockPanel as _;
        let chart = PanelLeaf::ChartPanel(ChartPanelId(1));
        let (w, h) = chart.min_size();
        assert!(w >= 240.0 && h >= 160.0);
        let ind = PanelLeaf::IndicatorPanel(InstanceId(1));
        let (w, h) = ind.min_size();
        assert!(w >= 160.0 && h >= 60.0);
    }
}
