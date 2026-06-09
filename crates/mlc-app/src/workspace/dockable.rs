//! DockablePanel extension trait over uzor::panels::DockPanel.
//!
//! Panels in mylittlechart are split into two workspaces — sidebar and main.
//! Each panel declares rules about where it may live and how it behaves.

use uzor::panels::DockPanel;

/// Rules governing where a panel is allowed to be placed and its instance behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacementRules {
    /// Panel may live in the sidebar workspace.
    pub allow_sidebar: bool,
    /// Panel may live in the main workspace (center area, near charts).
    pub allow_main: bool,
    /// Panel may be detached into a uzor floating window.
    pub allow_floating: bool,
    /// Panel is bound to a dedicated rail button.
    /// If true: clicking the button focuses it, or respawns if missing.
    /// Typically implies singleton.
    pub sidebar_pinned: bool,
    /// At most one instance of this panel may exist across all workspaces.
    pub singleton: bool,
}

impl PlacementRules {
    /// A panel locked to sidebar only, pinned to a rail button, singleton.
    /// Used for Watchlist, Alerts, ObjectTree, Signals, Connectors, Performance.
    pub const SIDEBAR_PINNED: Self = Self {
        allow_sidebar: true,
        allow_main: false,
        allow_floating: false,
        sidebar_pinned: true,
        singleton: true,
    };

    /// A panel locked to sidebar, not pinned (rail button opens a spawner menu).
    /// Used for Agents (PTY terminals, Chat).
    pub const SIDEBAR_MULTI: Self = Self {
        allow_sidebar: true,
        allow_main: false,
        allow_floating: false,
        sidebar_pinned: false,
        singleton: false,
    };

    /// A panel locked to main area, multi-instance, may float.
    /// Used for Chart.
    pub const MAIN_ONLY: Self = Self {
        allow_sidebar: false,
        allow_main: true,
        allow_floating: true,
        sidebar_pinned: false,
        singleton: false,
    };

    /// A migratable singleton panel — may live in either workspace or float.
    /// Used for trading panels (OrderEntry, Positions, Dom, etc.).
    pub const MIGRATABLE_SINGLETON: Self = Self {
        allow_sidebar: true,
        allow_main: true,
        allow_floating: true,
        sidebar_pinned: false,
        singleton: true,
    };
}

/// Extension trait for panels that live in mylittlechart's docking workspaces.
/// Every concrete panel variant must declare its placement rules.
pub trait DockablePanel: DockPanel {
    fn placement_rules(&self) -> PlacementRules;
}
