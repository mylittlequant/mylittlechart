//! MainPanel enum — all panel types that can live in the main workspace (center area).

use serde::{Deserialize, Serialize};
use uzor::panels::DockPanel;

use super::dockable::{DockablePanel, PlacementRules};

/// Stable identifier for a chart instance.
pub type ChartId = u64;

/// All panel variants that can appear in the main workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MainPanel {
    // --- Category C: main-locked ---
    Chart(ChartId),

    // --- Category D: migratable singletons ---
    OrderEntry,
    Positions,
    Dom,
    Orders,
    Account,
    TradeHistory,
}

impl DockPanel for MainPanel {
    fn title(&self) -> &str {
        match self {
            Self::Chart(_)     => "Chart",
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
            Self::Chart(_)     => "panel.chart",
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
            Self::Chart(_) => (400.0, 300.0),
            _ => (240.0, 180.0),
        }
    }

    fn closable(&self) -> bool {
        true
    }
}

impl DockablePanel for MainPanel {
    fn placement_rules(&self) -> PlacementRules {
        match self {
            Self::Chart(_) => PlacementRules::MAIN_ONLY,

            Self::OrderEntry | Self::Positions | Self::Dom
            | Self::Orders | Self::Account | Self::TradeHistory
                => PlacementRules::MIGRATABLE_SINGLETON,
        }
    }
}

/// Convert a sidebar panel variant to the corresponding main panel variant.
/// Returns None if the panel type cannot live in the main workspace.
pub fn sidebar_to_main(p: &super::SidebarPanel) -> Option<MainPanel> {
    use super::SidebarPanel;
    match p {
        SidebarPanel::OrderEntry   => Some(MainPanel::OrderEntry),
        SidebarPanel::Positions    => Some(MainPanel::Positions),
        SidebarPanel::Dom          => Some(MainPanel::Dom),
        SidebarPanel::Orders       => Some(MainPanel::Orders),
        SidebarPanel::Account      => Some(MainPanel::Account),
        SidebarPanel::TradeHistory => Some(MainPanel::TradeHistory),
        _ => None,
    }
}

/// Convert a main panel variant to the corresponding sidebar panel variant.
/// Returns None if the panel type cannot live in the sidebar workspace.
pub fn main_to_sidebar(p: &MainPanel) -> Option<super::SidebarPanel> {
    use super::SidebarPanel;
    match p {
        MainPanel::OrderEntry   => Some(SidebarPanel::OrderEntry),
        MainPanel::Positions    => Some(SidebarPanel::Positions),
        MainPanel::Dom          => Some(SidebarPanel::Dom),
        MainPanel::Orders       => Some(SidebarPanel::Orders),
        MainPanel::Account      => Some(SidebarPanel::Account),
        MainPanel::TradeHistory => Some(SidebarPanel::TradeHistory),
        MainPanel::Chart(_)     => None,
    }
}
