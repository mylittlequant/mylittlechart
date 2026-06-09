//! SidebarPanel enum — all panel types that can live in the sidebar workspace.

use serde::{Deserialize, Serialize};
use uzor::panels::DockPanel;

use super::dockable::{DockablePanel, PlacementRules};

/// All panel variants that can appear in the sidebar workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SidebarPanel {
    // --- Category A: sidebar-locked, pinned to rail button, singleton ---
    Watchlist,
    Alerts,
    ObjectTree,
    Signals,
    Connectors,
    Performance,

    // --- Category B: sidebar-locked, multi-instance ---
    Agents(gate4agent::InstanceId),

    // --- Category D: migratable singletons (placeholder for Phase 4) ---
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

impl DockablePanel for SidebarPanel {
    fn placement_rules(&self) -> PlacementRules {
        match self {
            Self::Watchlist | Self::Alerts | Self::ObjectTree
            | Self::Signals | Self::Connectors | Self::Performance
                => PlacementRules::SIDEBAR_PINNED,

            Self::Agents(_) => PlacementRules::SIDEBAR_MULTI,

            Self::OrderEntry | Self::Positions | Self::Dom
            | Self::Orders | Self::Account | Self::TradeHistory
                => PlacementRules::MIGRATABLE_SINGLETON,
        }
    }
}
