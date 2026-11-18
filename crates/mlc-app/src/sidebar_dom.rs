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
    /// frame: the panels it covers, outside the states it does not.
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
            // Item 11: the whole panel area (header toolbar + body) is legacy-painted
            // into a slot over the DOM frame; verified at over==0 with the live freeze.
            RightSidebarPanel::SlotTrading
            | RightSidebarPanel::SlotDom
            | RightSidebarPanel::SlotTape
            | RightSidebarPanel::SlotL2
            | RightSidebarPanel::SlotBigTrades
            | RightSidebarPanel::SlotLiq => true,
            // Agents stays app-painted: its header CLI-picker (prompt/CLAUDE/gear)
            // does not reproduce in the panel-area slot path yet (item 11 remainder).
            _ => false,
        }
    }

    fn watchlist_on_dom(&self) -> bool {
        true
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

    /// DOM-host paint-slot entry for a typed slot panel body (item 11): binds the
    /// six app-data closures exactly as the `render_right_sidebar` call site does
    /// (mlc-app lib.rs ~9110-9260) and calls `render_slot_panel`. Hits go to the
    /// caller's (throwaway) coordinator so the app's layout pass stays the single
    /// source of live hit zones. Returns the content height.
    pub fn render_slot_panel_dom(
        &mut self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        rect: &mlc_core::LayoutRect,
        content_y: f64,
        content_width: f64,
        slot_idx: usize,
        theme: &mlc_app_layout::ui::toolbar_theme::ToolbarTheme,
        result: &mut mlc_app_sidebar_content::render::RightSidebarResult,
        coordinator: &mut uzor::input::InputCoordinator,
    ) -> f64 {
        // Pre-compute per-panel color + sync-flags maps BEFORE the &mut sidebar_state
        // borrow (E0502), mirroring the render_right_sidebar call site.
        let (color_map, sync_map): (
            std::collections::HashMap<u64, [f32; 4]>,
            std::collections::HashMap<u64, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot>,
        ) = {
            use mlc_core::ids::SyncMemberId;
            let bubbles = self.core.bubbles();
            let tags = self.core.tags();
            let mut colors = std::collections::HashMap::new();
            let mut flags = std::collections::HashMap::new();
            for (tag_id, tag) in tags.iter() {
                let bubble_default = bubbles.sync_flags(*tag_id).unwrap_or_default();
                for &member in &tag.members {
                    if let SyncMemberId::Panel(pid) = member {
                        if !tag.auto_created {
                            colors.insert(pid, tag.color);
                        }
                        flags.insert(pid, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot {
                            sync_symbol: tag.effective_sync_symbol(member, bubble_default.sync_symbol),
                            sync_crosshair: tag.effective_sync_crosshair(member, bubble_default.sync_crosshair),
                        });
                    }
                }
            }
            (colors, flags)
        };
        let panel_theme = crate::panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());
        let panels_store = &self.panels_store;
        mlc_app_sidebar_content::render::render_slot_panel(
            ctx, rect, content_y, content_width, slot_idx,
            &mut self.sidebar_state, theme, result, coordinator,
            &mut |item, r, c, coord, prefix| {
                crate::panels_render::render_free_item(panels_store, item, r.0, r.1, r.2, r.3, c, &panel_theme, coord, prefix);
            },
            &|item: &mlc_core::state::Panel| -> Option<(bool, f64, f64, bool)> {
                match item {
                    mlc_core::state::Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter, s.multi_mode)),
                    _ => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<[f32; 4]> { color_map.get(&item.panel_id().0).copied() },
            &|item: &mlc_core::state::Panel| -> Option<mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot> { sync_map.get(&item.panel_id().0).cloned() },
            &|item: &mlc_core::state::Panel| -> Option<(String, String, String, String)> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::LiqTape(id) => panels_store.liq_tape.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::TradingContainer(id) => panels_store.trading_container.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), s.timeframe.clone())),
                    _ => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<Vec<bool>> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| vec![s.column_config.show_bid_orders, s.column_config.show_sell_trades, s.column_config.show_buy_trades, s.column_config.show_ask_orders]),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![s.column_config.show_time, s.column_config.show_type, s.column_config.show_side, s.column_config.show_price, s.column_config.show_qty]),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![s.column_config.show_time, s.column_config.show_price, s.column_config.show_size]),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![s.column_config.show_time, s.column_config.show_side, s.column_config.show_price, s.column_config.show_size, s.column_config.show_notional]),
                    _ => None,
                }
            },
        )
    }

    /// DOM-host paint-slot entry for a whole slot panel area (header toolbar +
    /// body + its own scrollbar), item 11 step 2: same closure binding as
    /// [`Self::render_slot_panel_dom`] plus the watchlist params, calling
    /// `render_right_sidebar` so the legacy slot header is reproduced exactly.
    /// Hits go to the caller's throwaway coordinator (the app's layout pass keeps
    /// the live zones).
    pub fn render_right_sidebar_dom(
        &mut self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        rect: &mlc_core::LayoutRect,
        theme: &mlc_app_layout::ui::toolbar_theme::ToolbarTheme,
        coordinator: &mut uzor::input::InputCoordinator,
    ) -> mlc_app_sidebar_content::render::RightSidebarResult {
        let (color_map, sync_map): (
            std::collections::HashMap<u64, [f32; 4]>,
            std::collections::HashMap<u64, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot>,
        ) = {
            use mlc_core::ids::SyncMemberId;
            let bubbles = self.core.bubbles();
            let tags = self.core.tags();
            let mut colors = std::collections::HashMap::new();
            let mut flags = std::collections::HashMap::new();
            for (tag_id, tag) in tags.iter() {
                let bubble_default = bubbles.sync_flags(*tag_id).unwrap_or_default();
                for &member in &tag.members {
                    if let SyncMemberId::Panel(pid) = member {
                        if !tag.auto_created {
                            colors.insert(pid, tag.color);
                        }
                        flags.insert(pid, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot {
                            sync_symbol: tag.effective_sync_symbol(member, bubble_default.sync_symbol),
                            sync_crosshair: tag.effective_sync_crosshair(member, bubble_default.sync_crosshair),
                        });
                    }
                }
            }
            (colors, flags)
        };
        let wl_cfg = self.watchlist_ui.column_config(self.watchlist_ui.active_list_id);
        let wl_list = self.core.watchlists().find(self.watchlist_ui.active_list_id).cloned();
        let panel_theme = crate::panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());
        let panels_store = &self.panels_store;
        mlc_app_sidebar_content::render::render_right_sidebar(
            ctx, rect, &mut self.sidebar_state, theme, coordinator,
            &mut |item, r, c, coord, prefix| {
                crate::panels_render::render_free_item(panels_store, item, r.0, r.1, r.2, r.3, c, &panel_theme, coord, prefix);
            },
            &|item: &mlc_core::state::Panel| -> Option<(bool, f64, f64, bool)> {
                match item {
                    mlc_core::state::Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter, s.multi_mode)),
                    _ => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<[f32; 4]> { color_map.get(&item.panel_id().0).copied() },
            &|item: &mlc_core::state::Panel| -> Option<mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot> { sync_map.get(&item.panel_id().0).cloned() },
            &|item: &mlc_core::state::Panel| -> Option<(String, String, String, String)> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::LiqTape(id) => panels_store.liq_tape.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::TradingContainer(id) => panels_store.trading_container.get(id).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), s.timeframe.clone())),
                    _ => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<Vec<bool>> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| vec![s.column_config.show_bid_orders, s.column_config.show_sell_trades, s.column_config.show_buy_trades, s.column_config.show_ask_orders]),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![s.column_config.show_time, s.column_config.show_type, s.column_config.show_side, s.column_config.show_price, s.column_config.show_qty]),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![s.column_config.show_time, s.column_config.show_price, s.column_config.show_size]),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![s.column_config.show_time, s.column_config.show_side, s.column_config.show_price, s.column_config.show_size, s.column_config.show_notional]),
                    _ => None,
                }
            },
            &wl_cfg,
            &|addr: &mlc_core::InstrumentAddress| -> Option<String> {
                wl_list.as_ref().and_then(|l| l.get_color_flag(addr)).map(|s| s.to_string())
            },
        )
    }
}
