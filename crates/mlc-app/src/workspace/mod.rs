//! Dual-workspace docking type definitions.
//!
//! See docs/plans/dual-workspace-docking.md for the full architecture.
//!
//! Phase 0: types only, not wired to any existing code yet.

pub mod dockable;
pub mod main_panel;
pub mod sidebar_panel;

pub use dockable::{DockablePanel, PlacementRules};
pub use main_panel::{ChartId, MainPanel, main_to_sidebar, sidebar_to_main};
pub use sidebar_panel::SidebarPanel;

#[cfg(test)]
mod tests;
