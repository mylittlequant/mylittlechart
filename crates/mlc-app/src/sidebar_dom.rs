//! What a uzor-framework host needs to draw the right sidebar itself
//! (`ChartPanelApp::dom_hosted.sidebar`).

use crate::ChartApp;
use mlc_app_sidebar_content::state::RightSidebarPanel;
use mlc_app_sidebar_content::types::WatchlistColumnConfig;

/// The watchlist's per-list data the sidebar renderer gets beside `sidebar_state`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct WatchlistDomData {
    /// The active list's columns.
    pub columns: WatchlistColumnConfig,
    /// Each row's colour flag (`""` = none), in `sidebar_state.watchlist_items` order.
    pub flags: Vec<String>,
}

impl ChartApp {
    /// Whether a host that set `dom_hosted.sidebar` draws the right sidebar this
    /// frame: the panels it covers, outside the states it does not (the watchlist's
    /// column menu, colour picker, row drag, market counters and failing rows).
    pub fn sidebar_on_dom(&self) -> bool {
        let s = &self.sidebar_state;
        if !(self.panel_app.dom_hosted.sidebar && s.is_right_open()) {
            return false;
        }
        match s.right_panel {
            RightSidebarPanel::Watchlist => self.watchlist_on_dom(),
            RightSidebarPanel::Alerts | RightSidebarPanel::Events | RightSidebarPanel::ObjectTree | RightSidebarPanel::Screener => true,
            // The Performance tab (the Connectors tab stays the app's).
            RightSidebarPanel::Monitoring => s.monitoring_section_expanded(mlc_app_sidebar_content::state::MonitoringSection::Performance),
            // A group that scrolls (more than 8 rows shown) keeps the app's painting.
            RightSidebarPanel::Signals => s
                .indicator_signals
                .groups
                .iter()
                .all(|g| g.signals.len() <= 8 || s.collapsed_signal_groups.contains(&g.instance_id)),
            _ => false,
        }
    }

    fn watchlist_on_dom(&self) -> bool {
        let s = &self.sidebar_state;
        let cfg = self.watchlist_ui.column_config(self.watchlist_ui.active_list_id);
        !s.watchlist_config_dropdown_open
            && s.watchlist_color_picker_open.is_none()
            && s.watchlist_drag_index.is_none()
            && !(cfg.show_market_counters && !s.counter_rows.is_empty())
            && s.watchlist_items.iter().all(|i| i.status.is_none())
    }

    /// The active watchlist's columns and row flags.
    pub fn watchlist_dom_data(&self) -> WatchlistDomData {
        let columns = self.watchlist_ui.column_config(self.watchlist_ui.active_list_id);
        let lists = self.core.watchlists();
        let list = lists.find(self.watchlist_ui.active_list_id);
        let flags = self
            .sidebar_state
            .watchlist_items
            .iter()
            .map(|i| list.and_then(|l| l.get_color_flag(&i.address)).map(|s| s.to_string()).unwrap_or_default())
            .collect();
        WatchlistDomData { columns, flags }
    }
}
