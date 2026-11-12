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
    /// frame: the watchlist, outside the states it does not cover (the column
    /// menu, the colour picker, a row drag, the market counters, a failing row).
    pub fn sidebar_on_dom(&self) -> bool {
        let s = &self.sidebar_state;
        if !(self.panel_app.dom_hosted.sidebar && s.is_right_open() && s.right_panel == RightSidebarPanel::Watchlist) {
            return false;
        }
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
