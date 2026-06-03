//! Phase 4 — Branch-as-Group model.
//!
//! Defines the unified docking payload (`PanelLeaf`) and the sidecar
//! metadata (`BranchMeta`) that consumers attach to a uzor Branch by
//! `BranchId`.
//!
//! See `docs/plans/phase4-branch-as-group-2026-06-04.md` for the full
//! design rationale.  These types are **stubs** in S1: they compile,
//! they are typed correctly, they are not yet consumed by any caller.
//! Wiring happens in S2 onward.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::state::chart_container_leaf::ChartPanelId;
use crate::state::chart_container::ConnectionStatus;
use crate::state::Timeframe;
use crate::tag_manager::SyncGroupId;
use crate::state::history::CommandHistory;

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
/// Variant set parallels `sidebar_content::free_slot::FreeItem` for
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
    /// Discriminant — useful for `AcceptPolicy` checks and rendering
    /// dispatch tables.
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
// BranchMeta — sidecar metadata attached to a uzor Branch
// =============================================================================

/// Semantic kind of a branch.  Distinct from `WindowLayout` (which is
/// the geometric arrangement uzor cares about); this is the consumer's
/// classification of *what the branch represents*.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BranchKind {
    /// Chart container — holds exactly one `ChartPanel` + N
    /// `IndicatorPanel` leaves.  Symbol / timeframe / indicator source
    /// are shared across all children.
    ChartContainer,
    /// Trading container — accepts any panel kind, supports nested
    /// branches.  No shared symbol at the branch level; each leaf
    /// carries its own.
    TradingContainer,
    /// Tag group — visual grouping with sync semantics but no panel
    /// kind restrictions.  Single-leaf tag-groups are normal.
    TagGroup,
    /// Plain group — no semantic meaning, default for branches uzor
    /// creates implicitly during `split_leaf` without a consumer-side
    /// classification.
    Plain,
}

impl BranchKind {
    /// When true the branch MUST be marked
    /// `tree.set_branch_preserve_if_empty(true)` after creation, so
    /// that its `BranchMeta` survives transient single-child states.
    pub fn needs_preservation(&self) -> bool {
        !matches!(self, BranchKind::Plain)
    }
}

/// Acceptance policy on drop.  Consumers check this before calling
/// uzor's `apply_panel_drop` / `dock_floating` — uzor does not enforce
/// it natively.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AcceptPolicy {
    /// Only the listed leaf kinds may be dropped into this branch.
    Only(HashSet<PanelLeafKind>),
    /// Any leaf kind allowed.
    Any,
}

impl AcceptPolicy {
    pub fn accepts(&self, kind: PanelLeafKind) -> bool {
        match self {
            AcceptPolicy::Only(s) => s.contains(&kind),
            AcceptPolicy::Any => true,
        }
    }

    /// Convenience constructor: ChartContainer's policy.
    pub fn chart_container() -> Self {
        let mut s = HashSet::with_capacity(2);
        s.insert(PanelLeafKind::ChartPanel);
        s.insert(PanelLeafKind::IndicatorPanel);
        AcceptPolicy::Only(s)
    }
}

/// Sidecar metadata attached to one uzor Branch.
///
/// Lives in the app's `HashMap<BranchId, BranchMeta>` map.  Branches
/// without an entry are treated as `Plain` with `AcceptPolicy::Any`
/// — backwards compatible with implicit branches uzor creates during
/// split.
#[derive(Clone, Debug)]
pub struct BranchMeta {
    pub kind: BranchKind,
    pub accepts: AcceptPolicy,
    /// Sync group this branch belongs to.
    pub group_id: Option<SyncGroupId>,
    /// Symbol context for chart-kind branches.  None for non-chart.
    pub symbol: Option<String>,
    pub timeframe: Option<Timeframe>,
    pub exchange: Option<String>,
    pub account_type: Option<String>,
    /// Display label.
    pub label: Option<String>,
    /// Live/delayed/disconnected status for this chart window.
    /// Phase 4 S2.3 mirror of `ChartContainer.connection_status`; `None`
    /// until first write.
    pub connection_status: Option<ConnectionStatus>,
    /// Previous session close price (first bar's open), used for the
    /// prev-close horizontal line. Phase 4 S2.3 mirror of
    /// `ChartContainer.prev_close_price`; re-derived on every
    /// `set_bars` / `update_bars`.
    pub prev_close_price: Option<f64>,
    /// Indicator instance IDs that existed BEFORE this window joined a
    /// sync group. Restored on desync so window-local indicators
    /// survive a join/leave cycle. Phase 4 S2.4 mirror of
    /// `ChartContainer.pre_tag_indicator_ids`.
    pub pre_tag_indicator_ids: Vec<u64>,
    /// Window-local undo history stashed at sync-group join time,
    /// restored on desync. Phase 4 S2.4 mirror of
    /// `ChartContainer.stashed_command_history`.
    pub stashed_command_history: Option<CommandHistory>,
}

impl BranchMeta {
    /// Build a ChartContainer-flavored metadata entry.
    pub fn chart_container(
        symbol: impl Into<String>,
        timeframe: Timeframe,
        exchange: impl Into<String>,
        account_type: impl Into<String>,
    ) -> Self {
        Self {
            kind: BranchKind::ChartContainer,
            accepts: AcceptPolicy::chart_container(),
            group_id: None,
            symbol: Some(symbol.into()),
            timeframe: Some(timeframe),
            exchange: Some(exchange.into()),
            account_type: Some(account_type.into()),
            label: None,
            connection_status: None,
            prev_close_price: None,
            pre_tag_indicator_ids: Vec::new(),
            stashed_command_history: None,
        }
    }

    /// Build a TradingContainer-flavored metadata entry.
    pub fn trading_container(label: impl Into<String>) -> Self {
        Self {
            kind: BranchKind::TradingContainer,
            accepts: AcceptPolicy::Any,
            group_id: None,
            symbol: None,
            timeframe: None,
            exchange: None,
            account_type: None,
            label: Some(label.into()),
            connection_status: None,
            prev_close_price: None,
            pre_tag_indicator_ids: Vec::new(),
            stashed_command_history: None,
        }
    }

    /// Build a TagGroup-flavored metadata entry.
    pub fn tag_group(group_id: SyncGroupId, label: Option<String>) -> Self {
        Self {
            kind: BranchKind::TagGroup,
            accepts: AcceptPolicy::Any,
            group_id: Some(group_id),
            symbol: None,
            timeframe: None,
            exchange: None,
            account_type: None,
            label,
            connection_status: None,
            prev_close_price: None,
            pre_tag_indicator_ids: Vec::new(),
            stashed_command_history: None,
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
    fn accept_policy_only() {
        let chart = AcceptPolicy::chart_container();
        assert!(chart.accepts(PanelLeafKind::ChartPanel));
        assert!(chart.accepts(PanelLeafKind::IndicatorPanel));
        assert!(!chart.accepts(PanelLeafKind::Dom));
    }

    #[test]
    fn accept_policy_any() {
        let any = AcceptPolicy::Any;
        for k in [
            PanelLeafKind::ChartPanel,
            PanelLeafKind::Dom,
            PanelLeafKind::OrderEntry,
        ] {
            assert!(any.accepts(k));
        }
    }

    #[test]
    fn branch_kind_preservation() {
        assert!(BranchKind::ChartContainer.needs_preservation());
        assert!(BranchKind::TradingContainer.needs_preservation());
        assert!(BranchKind::TagGroup.needs_preservation());
        assert!(!BranchKind::Plain.needs_preservation());
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
