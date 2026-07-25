//! Workspace focus (2026-07 workspace-panel arc).
//!
//! The Trading tab stops being a passive right-sidebar display and becomes
//! an equal workspace panel (owner directive) — some single-target UI
//! (symbol-search commit, the topbar's symbol/exchange/account derivation,
//! the tags&tabs pre-select) needs to know whether the user is currently
//! "in" a chart or "in" the Trading tab's container to pick the right
//! target. [`WorkspaceFocus`] is that one piece of state.

/// Which workspace surface currently has focus.
///
/// Read via [`crate::ChartApp::workspace_focus`], which self-heals a stale
/// `TradingPanel` back to `Chart` once the Trading tab is no longer the open
/// right-sidebar panel (closed or switched away) — so there is no dedicated
/// write site for "sidebar closed" / "sidebar switched to a different tab".
///
/// The two writes that DO exist are the two gestures that unambiguously
/// pick a target: a click landing inside the Trading tab's container body
/// (`panel_click.rs`'s `slot:{idx}:leaf:{id}:focus` handler) sets
/// `TradingPanel`; a click landing on the chart canvas
/// (`handle_canvas_click`, the `on_click` `ScaleCorner` branch, and
/// `on_right_click`) sets `Chart`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceFocus {
    Chart(mlc_core::ChartId),
    TradingPanel(mlc_core::state::PanelId),
}

impl WorkspaceFocus {
    /// `Some(pid)` iff focus is currently on the Trading tab's container.
    pub fn trading_panel(&self) -> Option<mlc_core::state::PanelId> {
        match self {
            WorkspaceFocus::TradingPanel(pid) => Some(*pid),
            WorkspaceFocus::Chart(_) => None,
        }
    }
}

impl crate::ChartApp {
    /// Current workspace focus, self-healing: a stored `TradingPanel` focus
    /// is honored only while the Trading tab is actually the open
    /// right-sidebar panel. Once it is closed or the sidebar switches to a
    /// different tab, this falls back to `Chart(active_container_id)`
    /// without requiring a dedicated write at every sidebar-close/switch
    /// call site (there are several, none of them a natural single hook).
    pub(crate) fn workspace_focus(&self) -> WorkspaceFocus {
        if let WorkspaceFocus::TradingPanel(pid) = self.workspace_focus {
            use mlc_app_sidebar_content::state::RightSidebarPanel;
            if self.sidebar_state.right_panel == RightSidebarPanel::SlotTrading {
                return WorkspaceFocus::TradingPanel(pid);
            }
        }
        match self.panel_app.panel_grid.active_container_id() {
            Some(cid) => WorkspaceFocus::Chart(cid),
            // No leaves at all (should not happen post-hydration) — keep
            // whatever was last stored rather than fabricating an id.
            None => self.workspace_focus,
        }
    }
}
