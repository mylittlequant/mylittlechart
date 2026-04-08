//! `SidebarPanel` — the panel type stored in the sidebar docking workspace.
//!
//! This lives in `sidebar-content` so that `SidebarState` (also here) can
//! own a `DockingManager<SidebarPanel>` without a circular crate dependency.
//!
//! The richer `DockablePanel` trait (placement rules) is implemented in
//! `chart-app/src/workspace/sidebar_panel.rs` which re-exports this type and
//! adds the placement-rule impl on top.

use serde::{Deserialize, Serialize};
use uzor::panels::DockPanel;

/// All panel variants that can appear in the sidebar workspace.
///
/// Category A: sidebar-locked, pinned to rail button, singleton.
/// Category B: sidebar-locked, multi-instance (Agents).
/// Category D: migratable singletons (trading panels, Phase 4+).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SidebarPanel {
    // --- Category A ---
    Watchlist,
    Alerts,
    ObjectTree,
    Signals,
    Connectors,
    Performance,

    // --- Category B ---
    Agents(gate4agent::InstanceId),

    // --- Category D (Phase 4 placeholders) ---
    OrderEntry,
    Positions,
    Dom,
    Orders,
    Account,
    TradeHistory,
}

impl DockPanel for SidebarPanel {
    fn title(&self) -> &str {
        match self {
            Self::Watchlist    => "Watchlist",
            Self::Alerts       => "Alerts",
            Self::ObjectTree   => "Objects",
            Self::Signals      => "Signals",
            Self::Connectors   => "Connectors",
            Self::Performance  => "Performance",
            Self::Agents(_)    => "Agent",
            Self::OrderEntry   => "Order Entry",
            Self::Positions    => "Positions",
            Self::Dom          => "DOM",
            Self::Orders       => "Orders",
            Self::Account      => "Account",
            Self::TradeHistory => "Trade History",
        }
    }

    fn type_id(&self) -> &'static str {
        match self {
            Self::Watchlist    => "panel.watchlist",
            Self::Alerts       => "panel.alerts",
            Self::ObjectTree   => "panel.object_tree",
            Self::Signals      => "panel.signals",
            Self::Connectors   => "panel.connectors",
            Self::Performance  => "panel.performance",
            Self::Agents(_)    => "panel.agents",
            Self::OrderEntry   => "panel.order_entry",
            Self::Positions    => "panel.positions",
            Self::Dom          => "panel.dom",
            Self::Orders       => "panel.orders",
            Self::Account      => "panel.account",
            Self::TradeHistory => "panel.trade_history",
        }
    }

    fn min_size(&self) -> (f32, f32) {
        match self {
            Self::Agents(_) => (240.0, 160.0),
            _ => (240.0, 180.0),
        }
    }

    fn closable(&self) -> bool {
        true
    }
}

impl SidebarPanel {
    /// Returns `true` if this panel corresponds to a Category A singleton
    /// that is pinned to a rail button.
    pub fn is_pinned_singleton(&self) -> bool {
        matches!(
            self,
            Self::Watchlist | Self::Alerts | Self::ObjectTree
            | Self::Signals | Self::Connectors | Self::Performance
        )
    }

    /// Returns `true` if this panel variant matches the given panel variant
    /// (ignoring `Agents` instance id — any Agents leaf matches).
    pub fn variant_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Watchlist,    Self::Watchlist)    => true,
            (Self::Alerts,       Self::Alerts)       => true,
            (Self::ObjectTree,   Self::ObjectTree)   => true,
            (Self::Signals,      Self::Signals)      => true,
            (Self::Connectors,   Self::Connectors)   => true,
            (Self::Performance,  Self::Performance)  => true,
            (Self::Agents(_),    Self::Agents(_))    => true,
            (Self::OrderEntry,   Self::OrderEntry)   => true,
            (Self::Positions,    Self::Positions)    => true,
            (Self::Dom,          Self::Dom)          => true,
            (Self::Orders,       Self::Orders)       => true,
            (Self::Account,      Self::Account)      => true,
            (Self::TradeHistory, Self::TradeHistory) => true,
            _ => false,
        }
    }
}
