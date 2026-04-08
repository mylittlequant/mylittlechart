//! Serialization round-trip tests for workspace panel types.

use gate4agent::InstanceId;

use super::dockable::PlacementRules;
use super::main_panel::{main_to_sidebar, sidebar_to_main, MainPanel};
use super::sidebar_panel::SidebarPanel;
use super::DockablePanel;

// --- Round-trip serialization tests ---

#[test]
fn sidebar_watchlist_round_trip() {
    let panel = SidebarPanel::Watchlist;
    let json = serde_json::to_string(&panel).unwrap();
    let back: SidebarPanel = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, SidebarPanel::Watchlist));
}

#[test]
fn sidebar_agents_round_trip() {
    let id = InstanceId::new();
    let panel = SidebarPanel::Agents(id);
    let json = serde_json::to_string(&panel).unwrap();
    let back: SidebarPanel = serde_json::from_str(&json).unwrap();
    match back {
        SidebarPanel::Agents(back_id) => assert_eq!(back_id, id),
        _ => panic!("expected SidebarPanel::Agents"),
    }
}

#[test]
fn main_chart_round_trip() {
    let panel = MainPanel::Chart(42);
    let json = serde_json::to_string(&panel).unwrap();
    let back: MainPanel = serde_json::from_str(&json).unwrap();
    match back {
        MainPanel::Chart(id) => assert_eq!(id, 42),
        _ => panic!("expected MainPanel::Chart"),
    }
}

// --- Placement rules: Category A variants → SIDEBAR_PINNED ---

#[test]
fn category_a_placement_rules() {
    let panels = [
        SidebarPanel::Watchlist,
        SidebarPanel::Alerts,
        SidebarPanel::ObjectTree,
        SidebarPanel::Signals,
        SidebarPanel::Connectors,
        SidebarPanel::Performance,
    ];
    for panel in &panels {
        assert_eq!(
            panel.placement_rules(),
            PlacementRules::SIDEBAR_PINNED,
            "{:?} should have SIDEBAR_PINNED rules",
            panel
        );
    }
}

#[test]
fn agents_placement_rules() {
    let panel = SidebarPanel::Agents(InstanceId::new());
    assert_eq!(panel.placement_rules(), PlacementRules::SIDEBAR_MULTI);
}

#[test]
fn chart_placement_rules() {
    let panel = MainPanel::Chart(1);
    assert_eq!(panel.placement_rules(), PlacementRules::MAIN_ONLY);
}

#[test]
fn order_entry_sidebar_placement_rules() {
    let panel = SidebarPanel::OrderEntry;
    assert_eq!(panel.placement_rules(), PlacementRules::MIGRATABLE_SINGLETON);
}

#[test]
fn order_entry_main_placement_rules() {
    let panel = MainPanel::OrderEntry;
    assert_eq!(panel.placement_rules(), PlacementRules::MIGRATABLE_SINGLETON);
}

// --- Conversion table tests ---

#[test]
fn sidebar_to_main_order_entry() {
    let sidebar = SidebarPanel::OrderEntry;
    let main = sidebar_to_main(&sidebar);
    assert!(matches!(main, Some(MainPanel::OrderEntry)));
}

#[test]
fn sidebar_to_main_watchlist_is_none() {
    let sidebar = SidebarPanel::Watchlist;
    let main = sidebar_to_main(&sidebar);
    assert!(main.is_none());
}

#[test]
fn main_to_sidebar_chart_is_none() {
    let main = MainPanel::Chart(1);
    let sidebar = main_to_sidebar(&main);
    assert!(sidebar.is_none());
}

#[test]
fn main_to_sidebar_positions() {
    let main = MainPanel::Positions;
    let sidebar = main_to_sidebar(&main);
    assert!(matches!(sidebar, Some(SidebarPanel::Positions)));
}
