//! Mega-router for click events: dispatches by widget_id to per-area
//! handlers (sidebar, watchlist, overlay, profile-manager, tags-tabs,
//! slot panels, agent leaves, etc.).
#[cfg(target_arch = "wasm32")]
use crate::agent as gate4agent;


use crate::{ChartApp, sglog};
use uzor::WidgetId;
use uzor::input::TextFieldConfig;
use mlc_core::localize_primitive_name;

/// Which store an `OrderEntryState` lives in — see
/// [`ChartApp::submit_order_entry_target`]. `pub(crate)` so
/// `text_input::dispatch`'s Enter/Escape handlers for the per-instance
/// `oe_field`/`rc_field` TextFieldStore ids can flow it through
/// `oe_target_for_slot_prefix`/`commit_oe_field` without naming variants.
#[derive(Copy, Clone)]
pub(crate) enum OrderEntryTarget {
    Standalone(mlc_core::state::PanelId),
    Container(mlc_core::state::PanelId),
}

/// Which store a `RiskCalculatorState` lives in — mirrors `OrderEntryTarget`.
#[derive(Copy, Clone)]
pub(crate) enum RiskCalculatorTarget {
    Standalone(mlc_core::state::PanelId),
    Container(mlc_core::state::PanelId),
}

impl ChartApp {
    /// Dispatch a click that landed on a registered widget.
    ///
    /// Handles toolbar clicks and dropdown item selections by forwarding to
    /// the toolbar state's handler methods.  Modal widget clicks are logged
    /// but not fully wired — modal input routing would require a handle_input()
    /// on ChartPanelApp which does not exist at this checkpoint.
    pub(crate) fn dispatch_panel_click(&mut self, widget_id: &str, x: f64, y: f64) {
        // === Profile Manager lock guard — block everything while it is shown ===
        // While `show_profile_manager` is true the ONLY interactive elements are
        // those inside the profile manager overlay.  All other UI is silently
        // swallowed so the user cannot reach the chart, toolbar, or settings until
        // they select a profile or dismiss the manager.
        if self.panel_app.user_settings_state.show_profile_manager {
            let allowed = widget_id.starts_with("profile_manager:")
                || widget_id.starts_with("user_settings:profile_mgr:")
                || widget_id.starts_with("user_settings:profile_delete:")
                || widget_id.starts_with("profile_mgr:device_")
                || widget_id.starts_with("profile_mgr:channel_")
                || widget_id == "user_settings:e2e_passphrase_input"
                || widget_id == "user_settings:profile_mgr:recovery_key_display";
            if !allowed {
                return;
            }
            // Dimmer click — dismiss profile manager if user has a live profile
            // and is NOT in skeleton mode (vault unlocked, not first run).
            if widget_id == "profile_manager:dimmer" {
                let in_skeleton = self.panel_app.user_settings_state.needs_vault_unlock;
                if !in_skeleton && !self.panel_app.user_settings_state.runtime_profile_id.is_empty() {
                    self.panel_app.user_settings_state.show_profile_manager = false;
                    self.panel_app.user_settings_state.profile_manager_page =
                        mlc_app_layout::ui::modal_settings::ProfileManagerPage::ProfileList;
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    {
                        let wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&wid);
                    }
                    eprintln!("[ChartApp] profile_manager: dimmer clicked, dismissing (live profile exists)");
                }
                return;
            }
            // Absorb other profile_manager: background clicks
            if widget_id.starts_with("profile_manager:") {
                return;
            }
        }

        // Standalone panel-settings surface (overlay gear, 2026-07-28) is
        // transient — any click outside it (and not on a gear that would
        // reopen it) dismisses it; the click then proceeds normally.
        if self.panel_settings_open.is_some()
            && !widget_id.starts_with("pset:")
            && !widget_id.ends_with(":gear")
        {
            self.panel_settings_open = None;
        }

        // === Panel-settings surface clicks ===
        if let Some(rest) = widget_id.strip_prefix("pset:") {
            match rest {
                "close" => {
                    self.panel_settings_open = None;
                }
                "bg" => {
                    // Absorb click — prevent fall-through.
                }
                r if r.starts_with("opt:open_as:") => {
                    // Universal "Open as" choice — persisted presentation
                    // preference (Dropdown = 0, Modal = 1).
                    let want_modal = r.ends_with(":1");
                    {
                        let mut user_mut = self.core.user_mut();
                        user_mut.profile_mut().ui.panel_gear_opens_modal = want_modal;
                    }
                    self.autosave_snapshot();
                }
                r if r.starts_with("opt:hist:") => {
                    use mlc_app_panels::trading::trading::history::HistoryDepth;
                    let idx: usize = r["opt:hist:".len()..].parse().unwrap_or(2);
                    let depth = HistoryDepth::OPTIONS.get(idx).copied().unwrap_or_default();
                    if let Some((panel, _)) = self.panel_settings_open.clone() {
                        match panel {
                            mlc_core::state::Panel::PositionManager(pid) => {
                                if let Some(s) = self.panels_store.position_manager.get_mut(&pid) {
                                    s.history_depth = depth;
                                }
                            }
                            mlc_core::state::Panel::TradeLog(pid) => {
                                if let Some(s) = self.panels_store.trade_log.get_mut(&pid) {
                                    s.history_depth = depth;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {
                    eprintln!("[ChartApp] unhandled pset click: {}", rest);
                }
            }
            return;
        }

        // === Slot spawn dropdown — click-away-to-dismiss ===
        // If a slot spawn dropdown is open and the clicked widget is not a
        // spawn row or the [+] button for that slot, close it immediately.
        if self.sidebar_state.slot_spawn_dropdown.is_some() {
            let is_spawn_related = widget_id.contains(":spawn:")
                || (widget_id.strip_prefix("slot:").and_then(|s| s.strip_suffix(":new")).is_some());
            if !is_spawn_related {
                self.sidebar_state.slot_spawn_dropdown = None;
                self.sidebar_data_dirty = true;
            }
        }

        // === Agent dropdowns (model/perm/sessions) — click-away-to-dismiss ===
        // If any agent popup is open and click is NOT on a popup-related widget, close all.
        {
            let any_open = self.sidebar_state.agent_model_dropdown.is_some()
                || self.sidebar_state.agent_perm_dropdown.is_some()
                || self.sidebar_state.agent_sessions_dropdown.is_some();
            if any_open {
                let is_popup_widget = widget_id.contains(":model")
                    || widget_id.contains(":perm")
                    || widget_id.contains(":sessions_toggle")
                    || widget_id.contains(":sessions_backdrop")
                    || widget_id.contains(":select_model:")
                    || widget_id.contains(":select_perm:")
                    || widget_id.contains(":select_session:")
                    || widget_id.contains(":model_backdrop")
                    || widget_id.contains(":perm_backdrop");
                if !is_popup_widget {
                    self.sidebar_state.agent_model_dropdown = None;
                    self.sidebar_state.agent_perm_dropdown = None;
                    self.sidebar_state.agent_sessions_dropdown = None;
                    self.sidebar_data_dirty = true;
                }
            }
        }

        // === Column config popup — click-away-to-dismiss ===
        if let Some((cfg_slot, cfg_leaf)) = self.sidebar_state.panel_col_config_open {
            let prefix = format!("slot:{}:leaf:{}:col_", cfg_slot, cfg_leaf);
            if !widget_id.starts_with(&prefix) {
                self.sidebar_state.panel_col_config_open = None;
                self.sidebar_data_dirty = true;
            }
        }

        // === Trading header inline symbol editor — click-away-to-dismiss
        // (trading-header-symbol-edit arc, 2026-07-25). Mirrors the column-
        // config block above: does NOT `return` — an outside click both
        // cancels the editor AND still reaches its real target (e.g. a
        // click on the chart canvas focuses the chart AND closes the
        // editor in the same click). Row/backdrop clicks on the editor's
        // own live-search popup are "related" and left alone; their own
        // handlers (below) commit or dismiss explicitly.
        if let Some(edit) = self.symbol_quick_edit.as_ref() {
            if let crate::SymbolQuickEditTarget::TradingPanel(pid)
            | crate::SymbolQuickEditTarget::SectionPanel(pid)
            | crate::SymbolQuickEditTarget::MultiDomRoster(pid)
            | crate::SymbolQuickEditTarget::MultiTapeRoster(pid) = edit.target
            {
                // The search icon is also a "related" entry point (not just
                // the tab body) — otherwise this block would close the
                // editor a beat before `open_trading_symbol_quick_edit`'s
                // own already-editing no-op guard gets to run, turning a
                // re-click on the icon into a query-resetting reopen
                // instead of a true no-op.
                let _ = pid;
                let related = widget_id == "tch:symbol_selector"
                    || widget_id.starts_with("dropdown:symbol_selector:")
                    || widget_id == "dropdown:__bg__";
                if !related {
                    self.close_symbol_quick_dropdown();
                }
            }
        }

        // === Right sidebar widgets ===
        if widget_id == "right_sidebar_close" {
            if let Some((_closing, _width)) = self.sidebar_state.close_right() {
                eprintln!("[ChartApp] Sidebar closed via close button");
                // Sidebar visibility is persisted in UserProfile; mirror the
                // Toggle* handlers in chart_out_events.rs so the next launch
                // restores the user's "closed" state.
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            return;
        }


        // === Watchlist panel header buttons ===

        if widget_id == "watchlist_open_modal" {
            self.standalone_panels.watchlist_modal.open();
            {
                let wl_id = WidgetId::from(crate::text_input::WATCHLIST_SEARCH);
                self.input_coordinator.borrow_mut().text_fields_mut().clear(&wl_id);
                self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&wl_id);
                self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::WATCHLIST_SEARCH);
            }
            eprintln!("[Sidebar] Watchlist expand button clicked — opening watchlist modal");
            return;
        }

        if widget_id == "watchlist_column_config" {
            // Toggle the column-config dropdown panel inside the sidebar.
            self.sidebar_state.watchlist_config_dropdown_open =
                !self.sidebar_state.watchlist_config_dropdown_open;
            eprintln!("[Sidebar] Watchlist column-config dropdown toggled: {}",
                self.sidebar_state.watchlist_config_dropdown_open);
            return;
        }

        if widget_id == "watchlist_sort_color" {
            // Cycle through 3 sort modes: 0 = no sort, 1 = red first, 2 = gray first.
            self.sidebar_state.watchlist_sort_mode =
                (self.sidebar_state.watchlist_sort_mode + 1) % 3;
            let mode = self.sidebar_state.watchlist_sort_mode;
            eprintln!("[Sidebar] watchlist_sort_mode -> {}", mode);
            let active_list_id = self.watchlist_ui.active_list_id;
            let mut wl = self.core.watchlists_mut();
            if let Some(list) = wl.find_mut(active_list_id) {
                let list_ui = self.watchlist_ui.list_mut(active_list_id);
                match mode {
                    0 => {
                        // Reset to original order from snapshot.
                        if let Some(snap) = list_ui.order_snapshot.take() {
                            list.ungrouped = snap;
                        }
                        eprintln!("[Sidebar] Watchlist sort reset to original order");
                    }
                    1 | 2 => {
                        // Save snapshot if not already saved (first sort from unsorted state).
                        if list_ui.order_snapshot.is_none() {
                            list_ui.order_snapshot = Some(list.ungrouped.clone());
                        }
                        // Always sort from the snapshot base so switching between modes
                        // 1 and 2 does not compound on an already-sorted order.
                        let base = list_ui.order_snapshot.as_ref().unwrap().clone();
                        let color_order: &[&str] = &[
                            "#ef5350", "#f59e0b", "#22c55e", "#3b82f6",
                            "#a855f7", "#ec4899", "#6b7280",
                        ];
                        let mut symbols = base;
                        symbols.sort_by(|a, b| {
                            let a_idx = list.get_color_flag(a)
                                .and_then(|f| color_order.iter().position(|c| *c == f))
                                .unwrap_or(99);
                            let b_idx = list.get_color_flag(b)
                                .and_then(|f| color_order.iter().position(|c| *c == f))
                                .unwrap_or(99);
                            if mode == 1 {
                                a_idx.cmp(&b_idx)
                            } else {
                                b_idx.cmp(&a_idx)
                            }
                        });
                        list.ungrouped = symbols;
                        let label = if mode == 1 { "red first" } else { "gray first" };
                        eprintln!("[Sidebar] Watchlist sorted: flagged first ({})", label);
                    }
                    _ => {}
                }
            }
            drop(wl);
            self.watchlist_actions.push(crate::WatchlistAction::SortCycle);
            return;
        }

        // Backdrop click inside the dropdown — just consume the event (keeps dropdown open).
        if widget_id == "watchlist_cfg_backdrop" {
            return;
        }

        // Column config checkbox toggles.
        if let Some(field) = widget_id.strip_prefix("watchlist_cfg:") {
            // Strip the "show_" prefix to match the action enum keys used by
            // about_to_wait() in main.rs (e.g. "show_volume" → "volume").
            let column_key = field.strip_prefix("show_").unwrap_or(field);
            eprintln!("[Sidebar] watchlist column toggled: {}", field);
            self.watchlist_actions.push(crate::WatchlistAction::ToggleColumnVisibility { column: column_key.to_string() });
            if column_key != "align_columns" {
                self.watchlist_actions.push(crate::WatchlistAction::ResetSeparatorOffsets);
            }
            self.watchlists_dirty = true;
            return;
        }

        // Any other click while the dropdown is open closes it.
        if self.sidebar_state.watchlist_config_dropdown_open {
            self.sidebar_state.watchlist_config_dropdown_open = false;
        }

        // Close color picker on any click that isn't on the picker or its flag.
        if self.sidebar_state.watchlist_color_picker_open.is_some()
            && !widget_id.starts_with("watchlist_color_")
            && !widget_id.starts_with("watchlist_flag_")
        {
            self.sidebar_state.watchlist_color_picker_open = None;
        }

        // === Watchlist panel clicks ===

        // Pattern: "watchlist_flag_{index}" — left-edge flag stripe click.
        // Toggles the color picker popup for that row.
        if widget_id.starts_with("watchlist_flag_") {
            if let Some(idx) = widget_id.strip_prefix("watchlist_flag_").and_then(|s| s.parse::<usize>().ok()) {
                // Toggle: close if same row already open, otherwise open.
                if self.sidebar_state.watchlist_color_picker_open.map(|(i, _, _)| i) == Some(idx) {
                    self.sidebar_state.watchlist_color_picker_open = None;
                } else if let Some(ref sidebar_result) = self.last_sidebar_result {
                    if let Some((_, row_rect)) = sidebar_result.watchlist_row_rects.get(idx) {
                        let popup_x = row_rect.x;
                        let popup_y = row_rect.y + row_rect.height;
                        self.sidebar_state.watchlist_color_picker_open = Some((idx, popup_x, popup_y));
                    }
                }
            }
            return;
        }

        // Pattern: "watchlist_color_{row}_{ci}" — swatch click in the color picker.
        if widget_id.starts_with("watchlist_color_") {
            if let Some(rest) = widget_id.strip_prefix("watchlist_color_") {
                let parts: Vec<&str> = rest.split('_').collect();
                if parts.len() == 2 {
                    if let (Ok(row_idx), Ok(color_idx)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                        let colors: &[&str] = &["#ef5350", "#f59e0b", "#22c55e", "#3b82f6", "#a855f7", "#ec4899", "#6b7280", ""];
                        if let Some(item) = self.sidebar_state.watchlist_items.get(row_idx) {
                            let address = item.address.clone();
                            let color_set: Option<String> = {
                                let active_list_id = self.watchlist_ui.active_list_id;
                                let mut wl = self.core.watchlists_mut();
                                if wl.find(active_list_id).is_some() {
                                    let color = colors.get(color_idx).copied().unwrap_or_default();
                                    wl.set_color_flag(active_list_id, &address, color);
                                    Some(color.to_string())
                                } else {
                                    None
                                }
                            };
                            if let Some(color) = color_set {
                                let color_opt = if color.is_empty() { None } else { Some(color.clone()) };
                                self.watchlist_actions.push(crate::WatchlistAction::SetColorFlag {
                                    symbol: address.symbol.clone(),
                                    exchange: address.exchange.clone(),
                                    account_type: address.account_type.as_label().to_string(),
                                    color: color_opt,
                                });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[Sidebar] Color flag set: {} = {:?}", address.routing_key(), color);
                            }
                        }
                        self.sidebar_state.watchlist_color_picker_open = None;
                    }
                }
            }
            return;
        }

        // Pattern: "watchlist_delete_{index}" — delete button on a row.
        // Must be checked BEFORE the generic "watchlist_" prefix so it doesn't
        // fall through to the row-click handler.
        if widget_id.starts_with("watchlist_delete_") {
            if let Some(idx) = widget_id.strip_prefix("watchlist_delete_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(item) = self.sidebar_state.watchlist_items.get(idx) {
                    let symbol = item.symbol.clone();
                    let exchange = item.exchange.clone();
                    let account_type = item.account_type.clone();
                    let address = item.address.clone();
                    // Remove symbol from snapshot (if active) before removing from list.
                    {
                        let active_list_id = self.watchlist_ui.active_list_id;
                        if let Some(ref mut snap) = self.watchlist_ui.list_mut(active_list_id).order_snapshot {
                            snap.retain(|s| *s != address);
                        }
                        self.core.watchlists_mut().remove_symbol(active_list_id, &address);
                    }
                    self.watchlist_actions.push(crate::WatchlistAction::Remove { symbol: symbol.clone(), exchange: exchange.clone(), account_type: account_type.clone() });
                    self.watchlist_actions.push(crate::WatchlistAction::ClearOrderSnapshot);
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[Sidebar] Watchlist delete: {}:{}:{} ({})", symbol, exchange, account_type, idx);
                }
            }
            return;
        }

        // Pattern: "watchlist_{index}" — row click (switch symbol).
        if widget_id.starts_with("watchlist_") {
            if let Some(idx) = widget_id.strip_prefix("watchlist_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(item) = self.sidebar_state.watchlist_items.get(idx) {
                    let symbol = item.symbol.clone();
                    let item_exchange = item.exchange.clone();
                    let item_account_type = item.account_type.clone();
                    eprintln!("[Sidebar] Watchlist click: {} @ {} ({})", symbol, item_exchange, idx);

                    // Resolve ExchangeId from the item's exchange string.
                    let resolved_exchange = self.core.exchange_symbols()
                        .find_by_label(&item_exchange)
                        .unwrap_or(self.active_exchange);

                    // S2.7.1e: read prev instrument fields via branch_meta (B1).
                    let wl_active_cid = self.panel_app.panel_grid.active_container_id();
                    let prev_symbol: Option<String> = wl_active_cid
                        .and_then(|cid| self.symbol_for_chart(cid))
                        .map(String::from);
                    let prev_exchange: Option<String> = wl_active_cid
                        .and_then(|cid| self.exchange_for_chart(cid))
                        .map(String::from);
                    let prev_account_type: Option<String> = wl_active_cid
                        .and_then(|cid| self.account_type_for_chart(cid))
                        .map(|at| at.to_string());
                    if prev_symbol.as_deref() != Some(&symbol) || prev_exchange.as_deref() != Some(&item_exchange) || prev_account_type.as_deref() != Some(&item_account_type) {
                        // Capture old trade-stream identity BEFORE mutating window state.
                        // Per-container venue, NOT the app-global `active_exchange`
                        // (2026-07-15 wrong-venue bug: unsubscribing the OLD stream
                        // at the global venue leaked the container's real stream and
                        // decremented a foreign forwarder's refcount).
                        let old_trade_exchange = wl_active_cid
                            .map(|cid| self.exchange_id_for_chart(cid))
                            .unwrap_or(self.active_exchange);
                        let old_trade_symbol: String = wl_active_cid
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let old_trade_at = wl_active_cid
                            .and_then(|cid| self.account_type_for_chart(cid))
                            .map(mlc_backend_data_bridge::account_type_to_dig3)
                            .unwrap_or(digdigdig3::AccountType::Spot);
                        let timeframe = wl_active_cid
                            .and_then(|cid| self.timeframe_for_chart(cid))
                            .unwrap_or_default();
                        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                        // Phase 4 B: capture chart_id BEFORE the mutating block.
                        let active_chart_id = self.panel_app.panel_grid.active_container_id();
                        let old_sym_pc = active_chart_id
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let old_exch_pc = active_chart_id
                            .and_then(|cid| self.exchange_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let old_at_pc = active_chart_id
                            .and_then(|cid| self.account_type_for_chart(cid))
                            .map(|at| at.to_string())
                            .unwrap_or_else(|| "S".to_string());
                        let active_bid_pc = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                            if let Some(bid) = active_bid_pc {
                                let restore_snaps = window.take_drawings_snapshot_for_symbol(&symbol, &item_exchange, &item_account_type);
                                let new_address = mlc_core::instrument::InstrumentAddress::new(
                                    item_exchange.clone(),
                                    symbol.clone(),
                                    item_account_type.parse().unwrap_or_default(),
                                );
                                let result = self.core.drawings_mut().swap_symbol(
                                    bid, &new_address, restore_snaps.as_deref(),
                                );
                                let old_key = format!("{}:{}:{}", old_sym_pc, old_exch_pc, old_at_pc);
                                if result.old_primitives.is_empty() {
                                    window.symbol_drawings.remove(&old_key);
                                } else {
                                    window.symbol_drawings.insert(old_key, result.old_primitives);
                                }
                            }
                            window.begin_fresh_load();
                        }
                        if let Some(chart_id) = active_chart_id {
                            if let Some(bid) = self.bubble_for_chart(chart_id) {
                                self.core.bubbles_mut().set_bid_instrument_full(
                                    bid,
                                    symbol.clone(),
                                    item_exchange.clone(),
                                    item_account_type.clone(),
                                    timeframe.clone(),
                                );
                            }
                        }
                        self.active_exchange = resolved_exchange;
                        // Unsubscribe only the old symbol's trade stream, leaving other
                        // windows' streams intact.
                        if !old_trade_symbol.is_empty() {
                            self.core.unsubscribe_trades(old_trade_exchange, &old_trade_symbol, old_trade_at);
                        }
                        let eid_str = resolved_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (watchlist sidebar click)", eid_str);
                        } else {
                            let item_at = crate::account_type_from_label(&item_account_type);
                            self.core.ensure_connector(resolved_exchange);
                            // force=true — watchlist-click is an explicit
                            // user symbol switch; non-force is silently
                            // dedup-L2-skipped when this instrument's kline
                            // subscriber is still alive (see the ApplySymbol
                            // handler's matching comment).
                            self.core.request_kline(resolved_exchange, &symbol, &timeframe, item_at, None, Some(self.core.user().profile().domain.bar_count as usize), true);
                        }
                        // Propagate the new symbol+exchange+account_type to all other windows
                        // in the same sync group so the full instrument key is consistent.
                        if let Some(leaf) = active_leaf {
                            self.propagate_symbol_to_sync_group(leaf, &symbol, &item_exchange, &item_account_type);
                        }
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                }
            }
            return;
        }

        // === Connector panel clicks ===
        // Pattern: "connector_row:{exchange_id}" — toggle expand/collapse
        if widget_id.starts_with("connector_row:") {
            if let Some(exchange_id) = widget_id.strip_prefix("connector_row:") {
                let expanded = self.sidebar_state.connector_expanded.entry(exchange_id.to_string()).or_insert(false);
                *expanded = !*expanded;
                eprintln!("[Sidebar] Connector row toggled: {} -> expanded={}", exchange_id, *expanded);
            }
            return;
        }

        // Pattern: "connector_toggle:{exchange_id}" — toggle enabled/disabled dot
        if widget_id.starts_with("connector_toggle:") {
            if let Some(exchange_id_str) = widget_id.strip_prefix("connector_toggle:") {
                let enabled = self.sidebar_state.connector_enabled
                    .entry(exchange_id_str.to_string()).or_insert(true);
                *enabled = !*enabled;
                let now_enabled = *enabled;
                self.connector_actions.push(crate::ConnectorAction::ToggleEnabled { exchange_id: exchange_id_str.to_string() });

                if let Some(eid) = digdigdig3::ExchangeId::from_str(exchange_id_str) {
                    if now_enabled {
                        self.core.bridge_handle().enable_connector(eid);
                        eprintln!("[Sidebar] Connector enabled: {}", exchange_id_str);
                    } else {
                        self.core.bridge_handle().disable_connector(eid);
                        eprintln!("[Sidebar] Connector disabled: {}", exchange_id_str);
                    }
                } else {
                    eprintln!("[Sidebar] Connector toggle: {} -> enabled={}", exchange_id_str, now_enabled);
                }
                self.persist_profile();
            }
            return;
        }

        // Pattern: "connector_metrics:{exchange_id}" — toggle metrics section visibility
        if widget_id.starts_with("connector_metrics:") {
            if let Some(exchange_id) = widget_id.strip_prefix("connector_metrics:") {
                let visible = self.sidebar_state.connector_metrics_visible
                    .entry(exchange_id.to_string()).or_insert(false);
                *visible = !*visible;
                eprintln!("[Sidebar] Connector metrics toggled: {} -> visible={}", exchange_id, *visible);
            }
            return;
        }

        // Pattern: "connector_group:{group_label}" — toggle group collapse state
        if widget_id.starts_with("connector_group:") {
            if let Some(group_label) = widget_id.strip_prefix("connector_group:") {
                let collapsed = self.sidebar_state.connector_group_collapsed
                    .entry(group_label.to_string())
                    .or_insert(false);
                *collapsed = !*collapsed;
                eprintln!("[Sidebar] Connector group toggled: {} -> collapsed={}", group_label, *collapsed);
            }
            return;
        }

        // Pattern: "monitoring_section:{section_key}" — toggle the merged
        // Monitoring panel's top-level CONNECTORS/PERFORMANCE section collapse
        // (runtime-only, not persisted — see `SidebarState::monitoring_sections_expanded`).
        if widget_id.starts_with("monitoring_section:") {
            if let Some(section_key) = widget_id.strip_prefix("monitoring_section:") {
                use mlc_app_sidebar_content::state::MonitoringSection;
                let section = match section_key {
                    "connectors" => Some(MonitoringSection::Connectors),
                    "performance" => Some(MonitoringSection::Performance),
                    _ => None,
                };
                if let Some(section) = section {
                    self.sidebar_state.toggle_monitoring_section(section);
                    eprintln!(
                        "[Sidebar] Monitoring section toggled: {} -> expanded={}",
                        section_key,
                        self.sidebar_state.monitoring_section_expanded(section)
                    );
                }
            }
            return;
        }

        // === Performance panel control clicks — cycle through values ===
        if widget_id == "perf:backend" {
            use mlc_app_sidebar_content::state::RenderBackend;
            let current = &self.sidebar_state.performance_data.render_backend;
            let all = RenderBackend::all();
            let idx = all.iter().position(|b| b == current).unwrap_or(0);
            let next = all[(idx + 1) % all.len()];
            eprintln!("[ChartApp] perf: backend -> {}", next.label());
            self.perf_actions.push(crate::PerfAction::SetBackend(next.label().to_string()));
            return;
        }
        if widget_id == "perf:fps_limit" {
            // Cycle: 30 → 60 → 120 → 200 → 0 (unlimited) → 30
            let current = self.sidebar_state.performance_data.fps_limit;
            let next = match current {
                30 => 60,
                60 => 120,
                120 => 200,
                200 => 0,
                _ => 30,
            };
            self.perf_actions.push(crate::PerfAction::SetFpsLimit(next));
            eprintln!("[ChartApp] perf: FPS limit -> {}", if next == 0 { "unlimited".to_string() } else { format!("{}", next) });
            return;
        }
        if widget_id == "perf:msaa" {
            // Cycle: 0 (off) → 8 → 16 → 0 (vello has no Msaa4)
            let current = self.sidebar_state.performance_data.msaa_samples;
            let next = match current {
                0 => 8,
                8 => 16,
                _ => 0,
            };
            self.perf_actions.push(crate::PerfAction::SetMsaa(next));
            eprintln!("[ChartApp] perf: MSAA -> {}", if next == 0 { "off".to_string() } else { format!("{}x", next) });
            return;
        }
        if widget_id == "perf:recalc_mode" {
            // Cycle: PerFrame → PerBar → PerTick → PerFrame
            let current = self.sidebar_state.performance_data.recalc_mode.clone();
            let next = match current.as_str() {
                "PerFrame" => "PerBar",
                "PerBar" => "PerTick",
                _ => "PerFrame",
            };
            self.perf_actions.push(crate::PerfAction::SetRecalcMode(next.to_string()));
            eprintln!("[ChartApp] perf: recalc mode -> {}", next);
            return;
        }
        if widget_id == "perf:log_toggle" {
            self.perf_actions.push(crate::PerfAction::TogglePerfLog);
            return;
        }

        // === Agent panel control clicks ===

        // --- [PTY] mode toggle ---
        if widget_id == "agent:mode:pty" {
            self.sidebar_state.agent_spawn_mode = gate4agent::InstanceMode::Pty;
            self.sidebar_data_dirty = true;
            return;
        }

        // --- [Chat] mode toggle ---
        if widget_id == "agent:mode:chat" {
            self.sidebar_state.agent_spawn_mode = gate4agent::InstanceMode::Chat;
            self.sidebar_data_dirty = true;
            return;
        }

        // --- Backend spawn: agent:spawn:{claude|codex|gemini|opencode} ---
        // (Stage B: rows of the header backend dropdown + agents modal.)
        if let Some(cli_str) = widget_id.strip_prefix("agent:spawn:") {
            // Remember the pick for the dropdown button label.
            self.sidebar_state.agent_selected_backend = cli_str.to_string();
            use gate4agent::AgentCli;
            use mlc_app_sidebar_content::agents_dock::{AgentLeafDescriptor, AgentPaneLeaf};
            let cli = match cli_str {
                "claude"   => AgentCli::Claude,
                "codex"    => AgentCli::Codex,
                "gemini"   => AgentCli::Gemini,
                "opencode" => AgentCli::OpenCode,
                _ => return,
            };
            let mode = self.sidebar_state.agent_spawn_mode;
            let workdir = self.agent.cli_workdir(cli);
            let _ = std::fs::create_dir_all(&workdir);
            match self.agent.create_instance(cli, mode, workdir.clone()) {
                Ok(instance_id) => {
                    let desc = AgentLeafDescriptor {
                        instance_id, cli, mode, workdir, chat_session_id: None,
                    };
                    use mlc_app_sidebar_content::state::AgentSpawnLayout;
                    let leaf_id = match self.sidebar_state.agent_spawn_layout {
                        AgentSpawnLayout::Replace => {
                            if let Some(focus) = self.sidebar_state.focused_agent_leaf {
                                // Replace the focused leaf in-place: update descriptor and clear per-leaf state.
                                // The old instance session is orphaned (stop_instance is async, called on shutdown).
                                let new_leaf = AgentPaneLeaf { instance_id, cli, mode };
                                if let Some(leaf) = self.sidebar_state.agent_docking.inner_mut().tree_mut().leaf_mut(focus) {
                                    if let Some(slot) = leaf.panels.first_mut() {
                                        *slot = new_leaf;
                                    }
                                }
                                self.sidebar_state.agent_leaves.insert(focus, desc);
                                self.sidebar_state.agent_pty_selections.remove(&focus);
                                self.sidebar_state.agent_pty_scrolls.remove(&focus);
                                self.sidebar_state.agent_chat_scrolls.remove(&focus);
                                self.sidebar_state.agent_input_buffers.remove(&focus);
                                self.sidebar_state.agent_leaf_snapshots.remove(&focus);
                                focus
                            } else {
                                // No focused leaf — add normally.
                                let leaf = AgentPaneLeaf { instance_id, cli, mode };
                                let id = self.sidebar_state.agent_docking.inner_mut().tree_mut().add_leaf(leaf);
                                self.sidebar_state.agent_leaves.insert(id, desc);
                                id
                            }
                        }
                        AgentSpawnLayout::SplitH | AgentSpawnLayout::SplitV => {
                            let split_kind = if self.sidebar_state.agent_spawn_layout == AgentSpawnLayout::SplitH {
                                uzor::panels::SplitKind::SplitRight
                            } else {
                                uzor::panels::SplitKind::SplitBottom
                            };
                            if let Some(focus) = self.sidebar_state.focused_agent_leaf {
                                // Tree already has leaves — split the focused one using the selected direction.
                                let rw = self.sidebar_state.right_sidebar_width as f32;
                                let rh = self.height as f32;
                                let new_ids = self.sidebar_state.agent_docking.inner_mut().tree_mut()
                                    .split_leaf_with_children(focus, split_kind, rw, rh);
                                if new_ids.len() >= 2 {
                                    let original_id = new_ids[0];
                                    let sibling_id  = new_ids[1];
                                    // Remap the old focus descriptor to the retained original slot.
                                    if let Some(old_desc) = self.sidebar_state.agent_leaves.remove(&focus) {
                                        self.sidebar_state.agent_leaves.insert(original_id, old_desc);
                                    }
                                    // Transfer per-leaf state maps from old focus id to new original_id.
                                    if let Some(v) = self.sidebar_state.agent_pty_selections.remove(&focus) {
                                        self.sidebar_state.agent_pty_selections.insert(original_id, v);
                                    }
                                    if let Some(v) = self.sidebar_state.agent_pty_scrolls.remove(&focus) {
                                        self.sidebar_state.agent_pty_scrolls.insert(original_id, v);
                                    }
                                    if let Some(v) = self.sidebar_state.agent_chat_scrolls.remove(&focus) {
                                        self.sidebar_state.agent_chat_scrolls.insert(original_id, v);
                                    }
                                    if let Some(v) = self.sidebar_state.agent_input_buffers.remove(&focus) {
                                        self.sidebar_state.agent_input_buffers.insert(original_id, v);
                                    }
                                    if let Some(v) = self.sidebar_state.agent_leaf_snapshots.remove(&focus) {
                                        self.sidebar_state.agent_leaf_snapshots.insert(original_id, v);
                                    }
                                    // The new leaf goes into the sibling slot.
                                    self.sidebar_state.agent_leaves.insert(sibling_id, desc);
                                    sibling_id
                                } else {
                                    // split_leaf returned unexpected result — fall back to add_leaf.
                                    let leaf = AgentPaneLeaf { instance_id, cli, mode };
                                    let id = self.sidebar_state.agent_docking.inner_mut().tree_mut().add_leaf(leaf);
                                    self.sidebar_state.agent_leaves.insert(id, desc);
                                    id
                                }
                            } else {
                                // Empty tree — add the first leaf normally.
                                let leaf = AgentPaneLeaf { instance_id, cli, mode };
                                let id = self.sidebar_state.agent_docking.inner_mut().tree_mut().add_leaf(leaf);
                                self.sidebar_state.agent_leaves.insert(id, desc);
                                id
                            }
                        }
                    };
                    self.sidebar_state.focused_agent_leaf = Some(leaf_id);
                    self.sidebar_state.agent_docking.inner_mut().set_active_leaf(leaf_id);
                    eprintln!("[ChartApp] agent:spawn:{} mode={:?} — leaf {:?}", cli_str, mode, leaf_id);
                }
                Err(e) => eprintln!("[ChartApp] agent:spawn:{} error: {}", cli_str, e),
            }
            self.sidebar_data_dirty = true;
            self.profile_dirty = true;
            return;
        }

        // --- [H] / [V] / [R] spawn layout toggles ---
        if widget_id == "agent:split:h" {
            self.sidebar_state.agent_spawn_layout = mlc_app_sidebar_content::state::AgentSpawnLayout::SplitH;
            self.sidebar_data_dirty = true;
            return;
        }
        if widget_id == "agent:split:v" {
            self.sidebar_state.agent_spawn_layout = mlc_app_sidebar_content::state::AgentSpawnLayout::SplitV;
            self.sidebar_data_dirty = true;
            return;
        }
        if widget_id == "agent:split:replace" {
            self.sidebar_state.agent_spawn_layout = mlc_app_sidebar_content::state::AgentSpawnLayout::Replace;
            self.sidebar_data_dirty = true;
            return;
        }

        // --- [⊞/⊟] expand/collapse toggle ---
        if widget_id == "agent:expand_toggle" {
            let any_hidden = self.sidebar_state.agent_leaves.keys().any(|&lid| {
                self.sidebar_state.agent_docking.inner().tree().leaf(lid).map_or(false, |l| l.hidden)
            });
            if any_hidden {
                // Collapse mode: show all leaves.
                let all_ids: Vec<uzor::panels::LeafId> = self
                    .sidebar_state
                    .agent_leaves
                    .keys()
                    .copied()
                    .collect();
                for lid in all_ids {
                    self.sidebar_state.agent_docking.inner_mut().tree_mut().show_leaf(lid);
                }
            } else if let Some(focus) = self.sidebar_state.focused_agent_leaf {
                // Expand mode: hide all leaves except the focused one.
                let all_ids: Vec<uzor::panels::LeafId> = self
                    .sidebar_state
                    .agent_leaves
                    .keys()
                    .copied()
                    .collect();
                for lid in all_ids {
                    if lid != focus {
                        self.sidebar_state.agent_docking.inner_mut().tree_mut().hide_leaf(lid);
                    }
                }
            }
            self.sidebar_data_dirty = true;
            return;
        }

        // --- [↺] reset sizes ---
        if widget_id == "agent:reset_sizes" {
            self.sidebar_state.agent_docking.inner_mut().tree_mut().reset_proportions();
            self.sidebar_data_dirty = true;
            return;
        }

        // --- [Split H] / [Split V] buttons ---
        if widget_id == "agent:split_h" || widget_id == "agent:split_v" {
            if let Some(focus) = self.sidebar_state.focused_agent_leaf {
                use uzor::panels::SplitKind;
                use mlc_app_sidebar_content::agents_dock::AgentLeafDescriptor;
                let kind = if widget_id == "agent:split_h" { SplitKind::SplitRight } else { SplitKind::SplitBottom };
                let rw = self.sidebar_state.right_sidebar_width as f32;
                let rh = self.height as f32;
                // split_leaf creates 2 new leaf nodes replacing the old one.
                // new_ids[0] inherits the original leaf's position; new_ids[1] is the sibling.
                let new_ids = self.sidebar_state.agent_docking.inner_mut().tree_mut()
                    .split_leaf_with_children(focus, kind, rw, rh);
                if new_ids.len() >= 2 {
                    let original_id = new_ids[0];
                    let sibling_id  = new_ids[1];
                    // Move the original leaf descriptor to the new id.
                    if let Some(desc) = self.sidebar_state.agent_leaves.remove(&focus) {
                        self.sidebar_state.agent_leaves.insert(original_id, desc);
                    }
                    // Create a new instance for the sibling.
                    let cli = gate4agent::AgentCli::Claude;
                    let workdir = self.agent.cli_workdir(cli);
                    let _ = std::fs::create_dir_all(&workdir);
                    match self.agent.create_instance(cli, gate4agent::InstanceMode::Pty, workdir.clone()) {
                        Ok(instance_id) => {
                            let desc = AgentLeafDescriptor {
                                instance_id, cli, mode: gate4agent::InstanceMode::Pty,
                                workdir, chat_session_id: None,
                            };
                            self.sidebar_state.agent_leaves.insert(sibling_id, desc);
                            self.sidebar_state.focused_agent_leaf = Some(sibling_id);
                            self.sidebar_state.agent_docking.inner_mut().set_active_leaf(sibling_id);
                            eprintln!("[ChartApp] {} — original={:?} sibling={:?}", widget_id, original_id, sibling_id);
                        }
                        Err(e) => eprintln!("[ChartApp] {} create_instance error: {}", widget_id, e),
                    }
                    // Also transfer per-leaf state maps for the original.
                    if let Some(v) = self.sidebar_state.agent_pty_selections.remove(&focus) {
                        self.sidebar_state.agent_pty_selections.insert(original_id, v);
                    }
                    if let Some(v) = self.sidebar_state.agent_pty_scrolls.remove(&focus) {
                        self.sidebar_state.agent_pty_scrolls.insert(original_id, v);
                    }
                    if let Some(v) = self.sidebar_state.agent_chat_scrolls.remove(&focus) {
                        self.sidebar_state.agent_chat_scrolls.insert(original_id, v);
                    }
                    if let Some(v) = self.sidebar_state.agent_input_buffers.remove(&focus) {
                        self.sidebar_state.agent_input_buffers.insert(original_id, v);
                    }
                    if let Some(v) = self.sidebar_state.agent_leaf_snapshots.remove(&focus) {
                        self.sidebar_state.agent_leaf_snapshots.insert(original_id, v);
                    }
                }
            }
            self.sidebar_data_dirty = true;
            self.profile_dirty = true;
            return;
        }

        // --- [×] close focused pane button ---
        if widget_id == "agent:close_pane" {
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                if let Some(desc) = self.sidebar_state.agent_leaves.remove(&leaf_id) {
                    let id = desc.instance_id;
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(rt) = self.core.native_rt() {
                        let _ = rt.block_on(self.agent.stop_instance(id));
                    }
                }
                self.sidebar_state.agent_docking.inner_mut().tree_mut().remove_leaf(leaf_id);
                self.sidebar_state.agent_pty_selections.remove(&leaf_id);
                self.sidebar_state.agent_pty_scrolls.remove(&leaf_id);
                self.sidebar_state.agent_chat_scrolls.remove(&leaf_id);
                self.sidebar_state.agent_input_buffers.remove(&leaf_id);
                self.sidebar_state.agent_input_cursors.remove(&leaf_id);
                self.sidebar_state.agent_input_selections.remove(&leaf_id);
                self.sidebar_state.agent_leaf_snapshots.remove(&leaf_id);
                // Focus the next available leaf.
                let next = self.sidebar_state.agent_docking.inner().panel_rects().keys().next().copied();
                self.sidebar_state.focused_agent_leaf = next;
                if let Some(next_id) = next {
                    self.sidebar_state.agent_docking.inner_mut().set_active_leaf(next_id);
                    // Sync the chat text-field buffer to the newly focused leaf.
                    if self.sidebar_state.agent_leaves.get(&next_id)
                        .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                        .unwrap_or(false)
                    {
                        let buf = self.sidebar_state.agent_input_buffers
                            .get(&next_id).map(|s| s.as_str()).unwrap_or_default();
                        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, buf);
                    }
                }
                self.sidebar_data_dirty = true;
                self.profile_dirty = true;
            }
            return;
        }

        // --- Per-leaf: agent:leaf:{id}:focus ---
        if let Some(rest) = widget_id.strip_prefix("agent:leaf:") {
            if let Some(id_str) = rest.strip_suffix(":focus") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    self.sidebar_state.focused_agent_leaf = Some(leaf_id);
                    self.sidebar_state.agent_docking.inner_mut().set_active_leaf(leaf_id);
                    // Sync the chat text-field buffer to the newly focused leaf so
                    // keystrokes go to the correct leaf's buffer.
                    if self.sidebar_state.agent_leaves.get(&leaf_id)
                        .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                        .unwrap_or(false)
                    {
                        let buf = self.sidebar_state.agent_input_buffers
                            .get(&leaf_id).map(|s| s.as_str()).unwrap_or_default();
                        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, buf);
                    }
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:focus_content (click inside pane content) ---
            if let Some(id_str) = rest.strip_suffix(":focus_content") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    self.sidebar_state.focused_agent_leaf = Some(leaf_id);
                    self.sidebar_state.agent_docking.inner_mut().set_active_leaf(leaf_id);
                    // Focus the appropriate input field for this leaf type and sync
                    // the chat text-field buffer to prevent text leaking between leaves.
                    match self.sidebar_state.agent_leaves.get(&leaf_id).map(|d| d.mode) {
                        Some(gate4agent::InstanceMode::Pty) => {
                            self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_PTY);
                            self.agent_pty_hover_focused = true;
                            let is_empty = self.sidebar_state.agent_pty_selections.get(&leaf_id)
                                .map(|s| s.is_empty())
                                .unwrap_or(true);
                            if is_empty {
                                self.sidebar_state.agent_pty_selections.remove(&leaf_id);
                            }
                        }
                        Some(gate4agent::InstanceMode::Chat) => {
                            let buf = self.sidebar_state.agent_input_buffers
                                .get(&leaf_id).map(|s| s.as_str()).unwrap_or_default();
                            let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
                            self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, buf);
                            self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_CHAT);
                        }
                        None => {}
                    }
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:close ---
            if let Some(id_str) = rest.strip_suffix(":close") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    if let Some(desc) = self.sidebar_state.agent_leaves.remove(&leaf_id) {
                        let id = desc.instance_id;
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(rt) = self.core.native_rt() {
                            let _ = rt.block_on(self.agent.stop_instance(id));
                        }
                    }
                    self.sidebar_state.agent_docking.inner_mut().tree_mut().remove_leaf(leaf_id);
                    self.sidebar_state.agent_pty_selections.remove(&leaf_id);
                    self.sidebar_state.agent_pty_scrolls.remove(&leaf_id);
                    self.sidebar_state.agent_chat_scrolls.remove(&leaf_id);
                    self.sidebar_state.agent_input_buffers.remove(&leaf_id);
                    self.sidebar_state.agent_input_cursors.remove(&leaf_id);
                    self.sidebar_state.agent_input_selections.remove(&leaf_id);
                    self.sidebar_state.agent_leaf_snapshots.remove(&leaf_id);
                    if self.sidebar_state.focused_agent_leaf == Some(leaf_id) {
                        let next = self.sidebar_state.agent_docking.inner().panel_rects().keys().next().copied();
                        self.sidebar_state.focused_agent_leaf = next;
                        if let Some(next_id) = next {
                            self.sidebar_state.agent_docking.inner_mut().set_active_leaf(next_id);
                            // Sync the chat text-field buffer to the newly focused leaf.
                            if self.sidebar_state.agent_leaves.get(&next_id)
                                .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                                .unwrap_or(false)
                            {
                                let buf = self.sidebar_state.agent_input_buffers
                                    .get(&next_id).map(|s| s.as_str()).unwrap_or_default();
                                let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
                                self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, buf);
                            }
                        }
                    }
                    self.sidebar_data_dirty = true;
                    self.profile_dirty = true;
                    self.autosave_snapshot();
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:new_session (clear chat, start fresh) ---
            if let Some(id_str) = rest.strip_suffix(":new_session") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id) {
                        let iid = desc.instance_id;
                        self.agent.clear_chat_instance(iid);
                        self.sidebar_state.agent_active_session_id.insert(leaf_id, None);
                        self.sidebar_state.agent_leaf_snapshots.remove(&leaf_id);
                        self.sidebar_state.agent_chat_scrolls.remove(&leaf_id);
                    }
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:model (toggle model dropdown) ---
            if let Some(id_str) = rest.strip_suffix(":model") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    if self.sidebar_state.agent_model_dropdown == Some(leaf_id) {
                        self.sidebar_state.agent_model_dropdown = None;
                    } else {
                        self.sidebar_state.agent_sessions_dropdown = None;
                        self.sidebar_state.agent_perm_dropdown = None;
                        self.sidebar_state.agent_model_dropdown = Some(leaf_id);
                    }
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:perm (toggle permission dropdown) ---
            if let Some(id_str) = rest.strip_suffix(":perm") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    if self.sidebar_state.agent_perm_dropdown == Some(leaf_id) {
                        self.sidebar_state.agent_perm_dropdown = None;
                    } else {
                        self.sidebar_state.agent_sessions_dropdown = None;
                        self.sidebar_state.agent_model_dropdown = None;
                        self.sidebar_state.agent_perm_dropdown = Some(leaf_id);
                    }
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:model_backdrop (close model dropdown) ---
            if let Some(id_str) = rest.strip_suffix(":model_backdrop") {
                if let Ok(_raw) = id_str.parse::<u64>() {
                    self.sidebar_state.agent_model_dropdown = None;
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:perm_backdrop (close perm dropdown) ---
            if let Some(id_str) = rest.strip_suffix(":perm_backdrop") {
                if let Ok(_raw) = id_str.parse::<u64>() {
                    self.sidebar_state.agent_perm_dropdown = None;
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:select_model:{model_id} ---
            if let Some(pos) = rest.find(":select_model:") {
                let id_str = &rest[..pos];
                let model_id = &rest[pos + ":select_model:".len()..];
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    if model_id == "__default__" {
                        self.sidebar_state.agent_selected_model.remove(&leaf_id);
                    } else {
                        self.sidebar_state.agent_selected_model.insert(leaf_id, model_id.to_string());
                    }
                    self.sidebar_state.agent_model_dropdown = None;
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:select_perm:{perm_id} ---
            if let Some(pos) = rest.find(":select_perm:") {
                let id_str = &rest[..pos];
                let perm_id = &rest[pos + ":select_perm:".len()..];
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    if perm_id == "__default__" {
                        self.sidebar_state.agent_selected_perm.remove(&leaf_id);
                    } else {
                        self.sidebar_state.agent_selected_perm.insert(leaf_id, perm_id.to_string());
                    }
                    self.sidebar_state.agent_perm_dropdown = None;
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:sessions_toggle (open/close sessions dropdown) ---
            if let Some(id_str) = rest.strip_suffix(":sessions_toggle") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    if self.sidebar_state.agent_sessions_dropdown == Some(leaf_id) {
                        self.sidebar_state.agent_sessions_dropdown = None;
                    } else {
                        self.sidebar_state.agent_model_dropdown = None;
                        self.sidebar_state.agent_perm_dropdown = None;
                        if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id) {
                            let iid = desc.instance_id;
                            let sessions = self.agent.list_past_sessions_instance(iid);
                            self.sidebar_state.agent_past_sessions.insert(leaf_id, sessions);
                        }
                        self.sidebar_state.agent_sessions_dropdown = Some(leaf_id);
                    }
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:sessions_backdrop (close dropdown on outside click) ---
            if let Some(id_str) = rest.strip_suffix(":sessions_backdrop") {
                if let Ok(_raw) = id_str.parse::<u64>() {
                    self.sidebar_state.agent_sessions_dropdown = None;
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:load_session:{index} ---
            if let Some(pos) = rest.find(":load_session:") {
                let id_str = &rest[..pos];
                let idx_str = &rest[pos + ":load_session:".len()..];
                if let (Ok(raw), Ok(idx)) = (id_str.parse::<u64>(), idx_str.parse::<usize>()) {
                    let leaf_id = uzor::panels::LeafId(raw);
                    let (iid, session_id_opt) = {
                        let desc = self.sidebar_state.agent_leaves.get(&leaf_id);
                        let iid = desc.map(|d| d.instance_id);
                        let session_id_opt = self.sidebar_state.agent_past_sessions
                            .get(&leaf_id)
                            .and_then(|sessions| sessions.get(idx))
                            .map(|s| s.id.clone());
                        (iid, session_id_opt)
                    };
                    if let (Some(iid), Some(session_id)) = (iid, session_id_opt) {
                        self.agent.load_history_instance(iid, &session_id);
                        self.sidebar_state.agent_active_session_id.insert(leaf_id, Some(session_id));
                        self.sidebar_state.agent_chat_scrolls.remove(&leaf_id);
                    }
                    self.sidebar_state.agent_sessions_dropdown = None;
                    self.sidebar_data_dirty = true;
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:start (spawn on demand) ---
            if let Some(id_str) = rest.strip_suffix(":start") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    let desc = self.sidebar_state.agent_leaves.get(&leaf_id).cloned();
                    if let Some(desc) = desc {
                        let id = desc.instance_id;
                        match desc.mode {
                            gate4agent::InstanceMode::Pty => {
                                use gate4agent::{SessionConfig, CliTool};
                                let tool = match desc.cli {
                                    gate4agent::AgentCli::Claude   => CliTool::ClaudeCode,
                                    gate4agent::AgentCli::Codex    => CliTool::Codex,
                                    gate4agent::AgentCli::Gemini   => CliTool::Gemini,
                                    gate4agent::AgentCli::OpenCode => CliTool::OpenCode,
                                };
                                let config = SessionConfig {
                                    tool,
                                    working_dir: desc.workdir.clone(),
                                    ..SessionConfig::default()
                                };
                                #[cfg(not(target_arch = "wasm32"))]
                                if let Some(rt) = self.core.native_rt() {
                                    match rt.block_on(self.agent.start_pty_instance(id, config)) {
                                        Ok(()) => {
                                            eprintln!("[ChartApp] agent:leaf:{:?}:start PTY started", leaf_id);
                                            self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_PTY);
                                            self.agent_pty_hover_focused = true;
                                        }
                                        Err(e) => eprintln!("[ChartApp] start PTY error: {}", e),
                                    }
                                }
                            }
                            gate4agent::InstanceMode::Chat => {
                                // Chat starts lazily on first send — focus input field.
                                // Sync buffer first to avoid leaking previous leaf's text.
                                let buf = self.sidebar_state.agent_input_buffers
                                    .get(&leaf_id).map(|s| s.as_str()).unwrap_or_default();
                                let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
                                self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, buf);
                                self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&chat_id);
                                self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_CHAT);
                                self.agent.load_latest_history_instance(id);
                            }
                        }
                        self.sidebar_state.focused_agent_leaf = Some(leaf_id);
                        self.sidebar_data_dirty = true;
                    }
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:input (chat input focus) ---
            if let Some(id_str) = rest.strip_suffix(":input") {
                if let Ok(_raw) = id_str.parse::<u64>() {
                    let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
                    if !self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
                        self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&chat_id);
                        self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_CHAT);
                    }
                    self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                    eprintln!("[ChartApp] agent leaf chat input focused");
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:send ---
            if let Some(id_str) = rest.strip_suffix(":send") {
                if let Ok(raw) = id_str.parse::<u64>() {
                    let leaf_id = uzor::panels::LeafId(raw);
                    let desc = self.sidebar_state.agent_leaves.get(&leaf_id).cloned();
                    if let Some(desc) = desc {
                        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
                        let from_field = self.input_coordinator.borrow().text_fields().text(&chat_id).to_string();
                        let from_buffer = self.sidebar_state.agent_input_buffers
                            .get(&leaf_id).cloned().unwrap_or_default();
                        let text = if !from_field.is_empty() { from_field } else { from_buffer };
                        eprintln!("[gate4agent::chat] send button leaf={:?} len={}", leaf_id, text.len());
                        if !text.is_empty() {
                            let id = desc.instance_id;
                            #[cfg(not(target_arch = "wasm32"))]
                            if let Some(rt) = self.core.native_rt() {
                                match rt.block_on(self.agent.send_chat_instance(id, &text)) {
                                    Ok(()) => {
                                        self.sidebar_state.agent_input_buffers.remove(&leaf_id);
                                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, "");
                                        eprintln!("[gate4agent::chat] send button OK");
                                    }
                                    Err(e) => eprintln!("[gate4agent::chat] send button error: {}", e),
                                }
                            }
                        }
                    }
                }
                return;
            }

            // --- Per-leaf: agent:leaf:{id}:past:{i} (load past session) ---
            if let Some((id_part, idx_str)) = rest.split_once(":past:") {
                if let (Ok(raw), Ok(idx)) = (id_part.parse::<u64>(), idx_str.parse::<usize>()) {
                    let leaf_id = uzor::panels::LeafId(raw);
                    let desc = self.sidebar_state.agent_leaves.get(&leaf_id).cloned();
                    if let Some(desc) = desc {
                        let id = desc.instance_id;
                        let sessions = self.agent.list_past_sessions_instance(id);
                        if let Some(meta) = sessions.get(idx).cloned() {
                            if self.agent.load_history_instance(id, &meta.id) {
                                self.sidebar_data_dirty = true;
                            }
                        }
                    }
                }
                return;
            }
        }

        // === Slot panel control clicks ===

        // --- slot:{idx}:new — toggle spawn dropdown for the slot ---
        if let Some(idx_str) = widget_id.strip_prefix("slot:").and_then(|s| s.strip_suffix(":new")) {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if idx < mlc_app_sidebar_content::free_slot::SLOT_COUNT {
                    if self.sidebar_state.slot_spawn_dropdown == Some(idx) {
                        self.sidebar_state.slot_spawn_dropdown = None;
                    } else {
                        self.sidebar_state.slot_spawn_dropdown = Some(idx);
                    }
                    self.sidebar_data_dirty = true;
                }
            }
            return;
        }

        // --- slot:{idx}:spawn:{kind_str} — spawn the selected panel variant ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, kind_str)) = rest.split_once(":spawn:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    self.spawn_panel_in_slot(idx, kind_str);
                }
                return;
            }
        }

        // --- slot:{idx}:split:h / :split:v / :split:replace — spawn layout toggle ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, action)) = rest.split_once(':') {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if idx < mlc_app_sidebar_content::free_slot::SLOT_COUNT {
                        match action {
                            "split:h" => {
                                self.sidebar_state.slot_spawn_layout =
                                    mlc_app_sidebar_content::state::AgentSpawnLayout::SplitH;
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "split:v" => {
                                self.sidebar_state.slot_spawn_layout =
                                    mlc_app_sidebar_content::state::AgentSpawnLayout::SplitV;
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "split:replace" => {
                                self.sidebar_state.slot_spawn_layout =
                                    mlc_app_sidebar_content::state::AgentSpawnLayout::Replace;
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            // Toggle: left half sets Tagged, right half sets Untagged.
                            "source:toggle" => {
                                self.sidebar_state.slot_source_mode =
                                    mlc_app_sidebar_content::state::SlotSourceMode::Tagged;
                                // If a panel in this slot is focused, update its source too.
                                if let Some((focused_idx, focused_leaf)) = self.sidebar_state.focused_free_leaf {
                                    if focused_idx == idx {
                                        let item_opt = self.sidebar_state.slot_dockings[idx]
                                            .inner()
                                            .tree()
                                            .leaf(focused_leaf)
                                            .and_then(|l| l.active_panel().cloned());
                                        if let Some(item) = item_opt {
                                            use mlc_app_panels::trading::SymbolSource;
                                            use mlc_core::state::Panel;
                                            let src = SymbolSource::HyperFocus;
                                            match &item {
                                                Panel::OrderEntry(id) => { if let Some(s) = self.panels_store.order_entry.get_mut(id) { s.source = src; } }
                                                Panel::TradingContainer(id) => { if let Some(s) = self.panels_store.trading_container.get_mut(id) { s.source = src; } }
                                                _ => {}
                                            }
                                            self.autosave_snapshot();
                                        }
                                    }
                                }
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "source:toggle_u" => {
                                self.sidebar_state.slot_source_mode =
                                    mlc_app_sidebar_content::state::SlotSourceMode::Untagged;
                                // If a panel in this slot is focused, update its source too.
                                if let Some((focused_idx, focused_leaf)) = self.sidebar_state.focused_free_leaf {
                                    if focused_idx == idx {
                                        let item_opt = self.sidebar_state.slot_dockings[idx]
                                            .inner()
                                            .tree()
                                            .leaf(focused_leaf)
                                            .and_then(|l| l.active_panel().cloned());
                                        if let Some(item) = item_opt {
                                            use mlc_app_panels::trading::SymbolSource;
                                            use mlc_core::state::Panel;
                                            // S2.7.1e: read instrument fields via branch_meta (B1).
                                            let toggle_u_cid = self.panel_app.panel_grid.active_container_id();
                                            let symbol: String = toggle_u_cid
                                                .and_then(|cid| self.symbol_for_chart(cid))
                                                .map(String::from)
                                                .unwrap_or_default();
                                            let exchange: String = toggle_u_cid
                                                .and_then(|cid| self.exchange_for_chart(cid))
                                                .map(String::from)
                                                .unwrap_or_default();
                                            let account_type: String = toggle_u_cid
                                                .and_then(|cid| self.account_type_for_chart(cid))
                                                .map(|at| at.to_string())
                                                .unwrap_or_default();
                                            let src = SymbolSource::Fixed { symbol, exchange, account_type };
                                            match &item {
                                                Panel::OrderEntry(id) => { if let Some(s) = self.panels_store.order_entry.get_mut(id) { s.source = src; } }
                                                Panel::TradingContainer(id) => { if let Some(s) = self.panels_store.trading_container.get_mut(id) { s.source = src; } }
                                                _ => {}
                                            }
                                            self.autosave_snapshot();
                                        }
                                    }
                                }
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "expand_toggle" => {
                                let all_leaf_ids: Vec<uzor::panels::LeafId> = self
                                    .sidebar_state.slot_dockings[idx]
                                    .inner()
                                    .panel_rects()
                                    .keys()
                                    .copied()
                                    .collect();
                                let any_hidden = all_leaf_ids.iter().any(|&lid| {
                                    self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(lid)
                                        .map_or(false, |l| l.hidden)
                                });
                                if any_hidden {
                                    // Show all leaves (collapse back to grid).
                                    for lid in all_leaf_ids {
                                        self.sidebar_state.slot_dockings[idx]
                                            .inner_mut()
                                            .tree_mut()
                                            .show_leaf(lid);
                                    }
                                } else if let Some((focused_idx, focus)) =
                                    self.sidebar_state.focused_free_leaf
                                {
                                    if focused_idx == idx {
                                        // Hide all leaves except the focused one.
                                        for lid in all_leaf_ids {
                                            if lid != focus {
                                                self.sidebar_state.slot_dockings[idx]
                                                    .inner_mut()
                                                    .tree_mut()
                                                    .hide_leaf(lid);
                                            }
                                        }
                                    }
                                }
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "reset_sizes" => {
                                self.sidebar_state.slot_dockings[idx]
                                    .inner_mut()
                                    .tree_mut()
                                    .reset_proportions();
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "close_pane" => {
                                if let Some((focused_idx, leaf_id)) =
                                    self.sidebar_state.focused_free_leaf
                                {
                                    if focused_idx == idx {
                                        // Retrieve the Panel before removing so we can
                                        // clean up the panels store.
                                        let item_opt = self.sidebar_state.slot_dockings[idx]
                                            .inner()
                                            .tree()
                                            .leaf(leaf_id)
                                            .and_then(|l| l.panels.get(l.active_tab).cloned());
                                        self.sidebar_state.slot_dockings[idx]
                                            .inner_mut()
                                            .tree_mut()
                                            .remove_leaf(leaf_id);
                                        if let Some(item) = item_opt {
                                            // Deregister from TagManager before removing state.
                                            let member = mlc_core::ids::SyncMemberId::Panel(item.panel_id().0);
                                            self.remove_panel_completely(member);
                                            // Unsubscribe panels from shared data streams
                                            // before dropping their state.
                                            if let mlc_core::state::Panel::Dom(pid) = &item {
                                                if let Some(state) = self.panels_store.dom.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.core.unsubscribe_orderbook(eid, &state.symbol, at);
                                                            self.core.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let mlc_core::state::Panel::BigTrades(pid) = &item {
                                                if let Some(state) = self.panels_store.big_trades.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.core.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let mlc_core::state::Panel::VolumeProfile(pid) = &item {
                                                if let Some(state) = self.panels_store.volume_profile.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.core.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let mlc_core::state::Panel::Footprint(pid) = &item {
                                                if let Some(state) = self.panels_store.footprint.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.core.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let mlc_core::state::Panel::L2Tape(pid) = &item {
                                                if let Some(state) = self.panels_store.l2_tape.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.core.unsubscribe_orderbook(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let mlc_core::state::Panel::TradeTape(pid) = &item {
                                                if let Some(state) = self.panels_store.trade_tape.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.core.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let mlc_core::state::Panel::LiquidityHeatmap(pid) = &item {
                                                if let Some(state) = self.panels_store.liquidity_heatmap.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.core.unsubscribe_orderbook(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            self.panels_store.remove(&item);
                                        }
                                        self.sidebar_state.focused_free_leaf = None;
                                        eprintln!(
                                            "[ChartApp] slot:{}:close_pane — removed leaf {:?}",
                                            idx, leaf_id
                                        );
                                        self.sidebar_data_dirty = true;
                                        self.autosave_snapshot();
                                    }
                                }
                                return;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:focus — set focused free leaf ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            // Try to parse "slot:{idx}:leaf:{leaf_id}:focus"
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(leaf_id_str) = leaf_rest.strip_suffix(":focus") {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            let leaf_id = uzor::panels::LeafId(raw);
                            self.sidebar_state.focused_free_leaf = Some((idx, leaf_id));
                            // Sync slot_source_mode to reflect the focused panel's current source.
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.active_panel().cloned());
                            if let Some(item) = item_opt {
                                use mlc_app_panels::trading::SymbolSource;
                                use mlc_core::state::Panel;
                                use mlc_app_sidebar_content::state::SlotSourceMode;
                                use mlc_core::ids::SyncMemberId;
                                // For market-data panels: mode reflects the panel's actual
                                // group state (auto_created = Untagged, else Tagged).
                                // For account-bound panels: mode reflects their SymbolSource.
                                let is_market_data = matches!(
                                    &item,
                                    Panel::Dom(_) | Panel::Footprint(_)
                                    | Panel::VolumeProfile(_) | Panel::LiquidityHeatmap(_)
                                    | Panel::BigTrades(_) | Panel::L2Tape(_)
                                    | Panel::TradeTape(_)
                                );
                                if is_market_data {
                                    let member = SyncMemberId::Panel(item.panel_id().0);
                                    let is_untagged = {
                                        let tags = self.core.tags();
                                        tags.tag_for_member(member)
                                            .and_then(|gid| tags.get(gid).map(|t| t.auto_created))
                                            .unwrap_or(true)
                                    };
                                    self.sidebar_state.slot_source_mode = if is_untagged {
                                        SlotSourceMode::Untagged
                                    } else {
                                        SlotSourceMode::Tagged
                                    };
                                } else {
                                    let source = match &item {
                                        Panel::OrderEntry(id) => self.panels_store.order_entry.get(id).map(|s| s.source.clone()),
                                        Panel::TradingContainer(id) => self.panels_store.trading_container.get(id).map(|s| s.source.clone()),
                                        _ => None,
                                    };
                                    if let Some(src) = source {
                                        self.sidebar_state.slot_source_mode = match src {
                                            SymbolSource::HyperFocus => SlotSourceMode::Tagged,
                                            SymbolSource::Fixed { .. } => SlotSourceMode::Untagged,
                                            SymbolSource::BoundToChart { .. } => SlotSourceMode::Tagged,
                                        };
                                    }
                                }
                            }
                            self.sidebar_data_dirty = true;
                        }
                        return;
                    }

                    // --- slot:{idx}:leaf:{leaf_id}:close — remove leaf + clean up state ---
                    if let Some(leaf_id_str) = leaf_rest.strip_suffix(":close") {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            let leaf_id = uzor::panels::LeafId(raw);
                            // Retrieve the Panel before removing so we can clean up the store.
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.panels.get(l.active_tab).cloned());
                            self.sidebar_state.slot_dockings[idx].inner_mut().tree_mut().remove_leaf(leaf_id);
                            if let Some(item) = item_opt {
                                // Deregister from TagManager before removing state.
                                let member = mlc_core::ids::SyncMemberId::Panel(item.panel_id().0);
                                self.remove_panel_completely(member);
                                // Unsubscribe panels from shared data streams before
                                // dropping their state.
                                if let mlc_core::state::Panel::Dom(pid) = &item {
                                    if let Some(state) = self.panels_store.dom.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.core.unsubscribe_orderbook(eid, &state.symbol, at);
                                                self.core.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let mlc_core::state::Panel::BigTrades(pid) = &item {
                                    if let Some(state) = self.panels_store.big_trades.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.core.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let mlc_core::state::Panel::VolumeProfile(pid) = &item {
                                    if let Some(state) = self.panels_store.volume_profile.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.core.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let mlc_core::state::Panel::Footprint(pid) = &item {
                                    if let Some(state) = self.panels_store.footprint.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.core.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let mlc_core::state::Panel::L2Tape(pid) = &item {
                                    if let Some(state) = self.panels_store.l2_tape.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.core.unsubscribe_orderbook(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let mlc_core::state::Panel::TradeTape(pid) = &item {
                                    if let Some(state) = self.panels_store.trade_tape.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.core.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let mlc_core::state::Panel::LiquidityHeatmap(pid) = &item {
                                    if let Some(state) = self.panels_store.liquidity_heatmap.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.core.unsubscribe_orderbook(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                self.panels_store.remove(&item);
                            }
                            if self.sidebar_state.focused_free_leaf == Some((idx, leaf_id)) {
                                self.sidebar_state.focused_free_leaf = None;
                            }
                            eprintln!("[ChartApp] slot:{}:leaf:{}:close", idx, raw);
                            self.sidebar_data_dirty = true;
                            self.autosave_snapshot();
                        }
                        return;
                    }

                    // --- slot:{idx}:leaf:{leaf_id}:split_h / :split_v — add sibling leaf ---
                    let is_split_h = leaf_rest.strip_suffix(":split_h");
                    let is_split_v = leaf_rest.strip_suffix(":split_v");
                    if let Some(leaf_id_str) = is_split_h.or(is_split_v) {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            let src_leaf = uzor::panels::LeafId(raw);
                            // Only act when the split target is the focused leaf.
                            if self.sidebar_state.focused_free_leaf == Some((idx, src_leaf)) {
                                // Clone the same panel type with fresh PanelId + copied config.
                                let source_item = self.sidebar_state.slot_dockings[idx]
                                    .inner()
                                    .tree()
                                    .leaf(src_leaf)
                                    .and_then(|l| l.active_panel().cloned());
                                let new_item = source_item
                                    .as_ref()
                                    .and_then(|item| self.panels_store.clone_item(item));
                                let new_item = match new_item {
                                    Some(item) => item,
                                    None => return, // source leaf missing or state gone
                                };
                                // Inherit tag-membership from source so split copies share
                                // the same sync group (color marker, symbol, sync state).
                                // Without this the new PanelId has no group → no color tag,
                                // doesn't follow active chart, leaks defaults.
                                if let Some(src_item) = source_item.as_ref() {
                                    let src_member = mlc_core::ids::SyncMemberId::Panel(src_item.panel_id().0);
                                    let new_member = mlc_core::ids::SyncMemberId::Panel(new_item.panel_id().0);
                                    let src_gid = self.group_for_member(src_member);
                                    let src_was_synced = self.core.tags().is_synced(src_member);
                                    if let Some(gid) = src_gid {
                                        if src_was_synced {
                                            self.set_panel_synced(new_member, gid);
                                        } else {
                                            self.set_panel_auto(new_member, gid);
                                        }
                                    }
                                }
                                let new_id = self.sidebar_state.slot_dockings[idx]
                                    .inner_mut()
                                    .tree_mut()
                                    .add_leaf_near(new_item, src_leaf);
                                self.sidebar_state.focused_free_leaf = Some((idx, new_id));
                                eprintln!("[ChartApp] slot:{}:leaf:{}:{} → new leaf {:?}",
                                    idx, raw,
                                    if is_split_h.is_some() { "split_h" } else { "split_v" },
                                    new_id);
                                self.sidebar_data_dirty = true;
                                self.autosave_snapshot();
                            }
                        }
                        return;
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:am_toggle — toggle DOM auto_center ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(leaf_id_str) = leaf_rest.strip_suffix(":am_toggle") {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            let leaf_id = uzor::panels::LeafId(raw);
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.active_panel().cloned());
                            if let Some(mlc_core::state::Panel::Dom(pid)) = item_opt {
                                if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                    state.auto_center = !state.auto_center;
                                    if state.auto_center && state.market_price > 0.0 {
                                        state.center_price = state.market_price;
                                    }
                                    self.sidebar_data_dirty = true;
                                }
                            }
                        }
                        return;
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:mx_toggle — toggle DOM multi-exchange mode ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(leaf_id_str) = leaf_rest.strip_suffix(":mx_toggle") {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            let leaf_id = uzor::panels::LeafId(raw);
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.active_panel().cloned());
                            if let Some(mlc_core::state::Panel::Dom(pid)) = item_opt {
                                let sub_info = self.panels_store.dom.get_mut(&pid).map(|state| {
                                    state.set_multi_mode(!state.multi_mode);
                                    (state.multi_mode, state.multi.symbol.clone(), state.multi.config.exchanges.clone())
                                });
                                if let Some((on, sym, exchanges)) = sub_info {
                                    if on {
                                        self.subscribe_multi_roster(&sym, &exchanges);
                                    }
                                    self.sidebar_data_dirty = true;
                                    self.autosave_snapshot();
                                }
                            }
                        }
                        return;
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:vol_filter — cycle DOM volume filter presets ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(leaf_id_str) = leaf_rest.strip_suffix(":vol_filter") {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            let leaf_id = uzor::panels::LeafId(raw);
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.active_panel().cloned());
                            if let Some(mlc_core::state::Panel::Dom(pid)) = item_opt {
                                if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                    let next = match state.min_volume_filter {
                                        v if v < 0.5  => 1.0,
                                        v if v < 1.5  => 5.0,
                                        v if v < 5.5  => 10.0,
                                        v if v < 10.5 => 25.0,
                                        v if v < 25.5 => 50.0,
                                        _             => 0.0,
                                    };
                                    state.min_volume_filter = next;
                                    eprintln!("[ChartApp] slot:{}:leaf:{}:vol_filter → {}", idx, raw, next);
                                    self.sidebar_data_dirty = true;
                                }
                            }
                        }
                        return;
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:col_config — toggle column config popup ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(leaf_id_str) = leaf_rest.strip_suffix(":col_config") {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            if self.sidebar_state.panel_col_config_open == Some((idx, raw)) {
                                self.sidebar_state.panel_col_config_open = None;
                            } else {
                                self.sidebar_state.panel_col_config_open = Some((idx, raw));
                            }
                            self.sidebar_data_dirty = true;
                        }
                        return;
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:col_toggle:{col_idx} — toggle column visibility ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(toggle_pos) = leaf_rest.find(":col_toggle:") {
                        let leaf_id_str = &leaf_rest[..toggle_pos];
                        let col_idx_str = &leaf_rest[toggle_pos + ":col_toggle:".len()..];
                        if let (Ok(raw), Ok(col_idx)) = (leaf_id_str.parse::<u64>(), col_idx_str.parse::<usize>()) {
                            let leaf_id = uzor::panels::LeafId(raw);
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.active_panel().cloned());

                            if let Some(item) = item_opt {
                                match item {
                                    mlc_core::state::Panel::Dom(pid) => {
                                        if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                            match col_idx {
                                                0 => state.column_config.show_bid_orders = !state.column_config.show_bid_orders,
                                                1 => state.column_config.show_sell_trades = !state.column_config.show_sell_trades,
                                                2 => state.column_config.show_buy_trades = !state.column_config.show_buy_trades,
                                                3 => state.column_config.show_ask_orders = !state.column_config.show_ask_orders,
                                                _ => {}
                                            }
                                        }
                                    }
                                    mlc_core::state::Panel::L2Tape(pid) => {
                                        if let Some(state) = self.panels_store.l2_tape.get_mut(&pid) {
                                            match col_idx {
                                                0 => state.column_config.show_time = !state.column_config.show_time,
                                                1 => state.column_config.show_type = !state.column_config.show_type,
                                                2 => state.column_config.show_side = !state.column_config.show_side,
                                                3 => state.column_config.show_price = !state.column_config.show_price,
                                                4 => state.column_config.show_qty = !state.column_config.show_qty,
                                                _ => {}
                                            }
                                        }
                                    }
                                    mlc_core::state::Panel::TradeTape(pid) => {
                                        if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                                            match col_idx {
                                                0 => state.column_config.show_time = !state.column_config.show_time,
                                                1 => state.column_config.show_price = !state.column_config.show_price,
                                                2 => state.column_config.show_size = !state.column_config.show_size,
                                                _ => {}
                                            }
                                        }
                                    }
                                    mlc_core::state::Panel::BigTrades(pid) => {
                                        if let Some(state) = self.panels_store.big_trades.get_mut(&pid) {
                                            match col_idx {
                                                0 => state.column_config.show_time = !state.column_config.show_time,
                                                1 => state.column_config.show_side = !state.column_config.show_side,
                                                2 => state.column_config.show_price = !state.column_config.show_price,
                                                3 => state.column_config.show_size = !state.column_config.show_size,
                                                4 => state.column_config.show_notional = !state.column_config.show_notional,
                                                _ => {}
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                                self.sidebar_data_dirty = true;
                            }
                        }
                        return;
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:tick_size — cycle DOM tick_size ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if let Some(leaf_id_str) = leaf_rest.strip_suffix(":tick_size") {
                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                            let leaf_id = uzor::panels::LeafId(raw);
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.active_panel().cloned());
                            if let Some(mlc_core::state::Panel::Dom(pid)) = item_opt {
                                if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                    const TICKS: &[f64] = &[0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0, 100.0];
                                    let cur = state.tick_size;
                                    let next = TICKS.iter()
                                        .find(|&&t| t > cur * 1.001)
                                        .copied()
                                        .unwrap_or(TICKS[0]);
                                    state.tick_size = next;
                                    state.volume_by_price.clear();
                                    self.sidebar_data_dirty = true;
                                }
                            }
                        }
                        return;
                    }
                }
            }
        }

        // --- slot:{idx}:leaf:{leaf_id}:source_cycle — cycle SymbolSource ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, leaf_rest)) = rest.split_once(":leaf:") {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if idx < self.sidebar_state.slot_dockings.len() {
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":source_cycle") {
                            if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                let leaf_id = uzor::panels::LeafId(raw);
                                // Resolve the active Panel for this leaf.
                                let item_opt = self.sidebar_state.slot_dockings[idx]
                                    .inner()
                                    .tree()
                                    .leaf(leaf_id)
                                    .and_then(|l| l.active_panel().cloned());
                                if let Some(item) = item_opt {
                                    // Get current source and compute next state.
                                    // HyperFocus → Fixed (pin to active chart symbol)
                                    // Fixed → HyperFocus
                                    // BoundToChart → HyperFocus
                                    use mlc_app_panels::trading::SymbolSource;
                                    use mlc_core::state::Panel;

                                    // Read the current source from account-bound panel states.
                                    // Market-data panels (dom, footprint, etc.) no longer carry
                                    // SymbolSource — they are managed via TagManager.
                                    let current_source = match &item {
                                        Panel::Dom(_)
                                        | Panel::Footprint(_)
                                        | Panel::VolumeProfile(_)
                                        | Panel::LiquidityHeatmap(_)
                                        | Panel::BigTrades(_)
                                        | Panel::L2Tape(_)
                                        | Panel::TradeTape(_) => None,
                                        Panel::OrderEntry(id) => self.panels_store.order_entry.get(id).map(|s| s.source.clone()),
                                        Panel::TradingContainer(id) => self.panels_store.trading_container.get(id).map(|s| s.source.clone()),
                                        Panel::PositionManager(_) | Panel::TradeLog(_) | Panel::RiskCalculator(_) => None,
                                        // Chart / IndicatorPanel are sync-group members like other
                                        // market-data panels — no independent SymbolSource field.  Source-
                                        // cycle button is a no-op for them.
                                        Panel::Chart(_) | Panel::IndicatorPanel(_) => None,
                                    };

                                    // S2.7.1e: read instrument fields via branch_meta (B1).
                                    let source_cycle_cid = self.panel_app.panel_grid.active_container_id();
                                    let new_source = match current_source {
                                        Some(SymbolSource::HyperFocus) => {
                                            // Pin to current active chart symbol/exchange/account_type.
                                            let symbol: String = source_cycle_cid
                                                .and_then(|cid| self.symbol_for_chart(cid))
                                                .map(String::from)
                                                .unwrap_or_default();
                                            let exchange: String = source_cycle_cid
                                                .and_then(|cid| self.exchange_for_chart(cid))
                                                .map(String::from)
                                                .unwrap_or_default();
                                            let account_type: String = source_cycle_cid
                                                .and_then(|cid| self.account_type_for_chart(cid))
                                                .map(|at| at.to_string())
                                                .unwrap_or_default();
                                            Some(SymbolSource::Fixed { symbol, exchange, account_type })
                                        }
                                        Some(SymbolSource::Fixed { .. }) | Some(SymbolSource::BoundToChart { .. }) => {
                                            Some(SymbolSource::HyperFocus)
                                        }
                                        None => None,
                                    };

                                    if let Some(src) = new_source {
                                        match &item {
                                            Panel::OrderEntry(id) => { if let Some(s) = self.panels_store.order_entry.get_mut(id) { s.source = src; } }
                                            Panel::TradingContainer(id) => { if let Some(s) = self.panels_store.trading_container.get_mut(id) { s.source = src; } }
                                            _ => {}
                                        }
                                        eprintln!("[ChartApp] slot:{}:leaf:{}:source_cycle", idx, raw);
                                        self.sidebar_data_dirty = true;
                                        self.autosave_snapshot();
                                    }
                                }
                            }
                            return;
                        }

                        // --- slot:{idx}:leaf:{leaf_id}:dom_zoom_in ---
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":dom_zoom_in") {
                            if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                let leaf_id = uzor::panels::LeafId(raw);
                                let item_opt = self.sidebar_state.slot_dockings[idx]
                                    .inner()
                                    .tree()
                                    .leaf(leaf_id)
                                    .and_then(|l| l.active_panel().cloned());
                                if let Some(mlc_core::state::Panel::Dom(pid)) = item_opt {
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        state.levels_displayed = (state.levels_displayed + 5).min(100);
                                        eprintln!("[ChartApp] slot:{}:leaf:{}:dom_zoom_in → {}", idx, raw, state.levels_displayed);
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                            }
                            return;
                        }

                        // --- slot:{idx}:leaf:{leaf_id}:dom_zoom_out ---
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":dom_zoom_out") {
                            if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                let leaf_id = uzor::panels::LeafId(raw);
                                let item_opt = self.sidebar_state.slot_dockings[idx]
                                    .inner()
                                    .tree()
                                    .leaf(leaf_id)
                                    .and_then(|l| l.active_panel().cloned());
                                if let Some(mlc_core::state::Panel::Dom(pid)) = item_opt {
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        state.levels_displayed = state.levels_displayed.saturating_sub(5).max(5);
                                        eprintln!("[ChartApp] slot:{}:leaf:{}:dom_zoom_out → {}", idx, raw, state.levels_displayed);
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                            }
                            return;
                        }

                        // --- slot:{idx}:leaf:{leaf_id}:dom_center ---
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":dom_center") {
                            if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                let leaf_id = uzor::panels::LeafId(raw);
                                let item_opt = self.sidebar_state.slot_dockings[idx]
                                    .inner()
                                    .tree()
                                    .leaf(leaf_id)
                                    .and_then(|l| l.active_panel().cloned());
                                if let Some(mlc_core::state::Panel::Dom(pid)) = item_opt {
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        state.center_price = state.market_price;
                                        eprintln!("[ChartApp] slot:{}:leaf:{}:dom_center → {:.2}", idx, raw, state.center_price);
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                            }
                            return;
                        }

                        // --- slot:{idx}:leaf:{leaf_id}:oe:* — OrderEntry interactive widgets ---
                        // Widget IDs registered by OrderEntryState::render follow the pattern
                        // "{slot_prefix}:oe:{local}" where slot_prefix = "slot:{idx}:leaf:{leaf_id}".
                        // Split leaf_rest on the first ':' to get (leaf_id_str, local_id).
                        // local_id will be e.g. "oe:buy", "oe:tab:0", "oe:submit", etc.
                        // A TradingContainer's hosted OrderEntry namespaces itself the exact
                        // same way (the container forwards its own slot_prefix unchanged), so
                        // it matches this same widget-id shape — only the target store differs.
                        if let Some((leaf_id_str, local_id)) = leaf_rest.split_once(':') {
                            if local_id.starts_with("oe:") {
                                // Gauge chip — toggles the Risk Calculator
                                // modal (overlay hosting, owner 2026-07-29);
                                // never reaches OrderEntry's own handler.
                                if local_id == "oe:rc_toggle" {
                                    if self.rc_modal.is_open {
                                        self.rc_modal.close();
                                    } else {
                                        self.rc_modal.open();
                                    }
                                    return;
                                }
                                if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use mlc_app_panels::panel_trait::TradingPanel;
                                    let slot_prefix = format!("slot:{}:leaf:{}", idx, raw);
                                    match item_opt {
                                        Some(mlc_core::state::Panel::OrderEntry(pid)) => {
                                            let handled = self.panels_store.order_entry.get_mut(&pid)
                                                .map(|state| state.handle_click(local_id, x, y))
                                                .unwrap_or(false);
                                            if handled {
                                                self.sidebar_data_dirty = true;
                                                if let Some(field) = Self::oe_field_for_local_id(local_id) {
                                                    self.start_oe_field_edit(OrderEntryTarget::Standalone(pid), &slot_prefix, field);
                                                }
                                                if local_id == "oe:submit" {
                                                    self.submit_order_entry(pid, &slot_prefix);
                                                }
                                            }
                                        }
                                        Some(mlc_core::state::Panel::TradingContainer(pid)) => {
                                            let handled = self.panels_store.trading_container.get_mut(&pid)
                                                .map(|state| state.handle_click(local_id, x, y))
                                                .unwrap_or(false);
                                            if handled {
                                                self.sidebar_data_dirty = true;
                                                if let Some(field) = Self::oe_field_for_local_id(local_id) {
                                                    self.start_oe_field_edit(OrderEntryTarget::Container(pid), &slot_prefix, field);
                                                }
                                                if local_id == "oe:submit" {
                                                    self.submit_order_entry_in_container(pid, &slot_prefix);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                return;
                            }

                            // --- bstrip: — bottom-strip tab switch (W3, 2026-07-25) ---
                            // Fully local — TradingContainerState::handle_click already
                            // does the whole job (flips `active_tab`), no app-layer
                            // follow-up needed. Only ever hosted inside a container.
                            if local_id.starts_with("bstrip:") {
                                if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use mlc_app_panels::panel_trait::TradingPanel;
                                    if let Some(mlc_core::state::Panel::TradingContainer(pid)) = item_opt {
                                        let handled = self.panels_store.trading_container.get_mut(&pid)
                                            .map(|state| state.handle_click(local_id, x, y))
                                            .unwrap_or(false);
                                        if handled {
                                            self.sidebar_data_dirty = true;
                                        }
                                    }
                                }
                                return;
                            }

                            // --- tcfree: — Free-mode docking chrome (owner spec
                            // 2026-07-29): overlay ✕ chips, tab-chip activate and
                            // tab-chip ✕. TradingContainerState::handle_click owns
                            // the whole family; spawn lives on the header dropdown.
                            if local_id.starts_with("tcfree:") {
                                // Per-panel gear chip — opens the container's
                                // COMMON settings modal (same contract as the
                                // tape/metrics strip gears).
                                if local_id.starts_with("tcfree:gear:") {
                                    self.tc_modal.open();
                                    return;
                                }
                                if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use mlc_app_panels::panel_trait::TradingPanel;
                                    if let Some(mlc_core::state::Panel::TradingContainer(pid)) = item_opt {
                                        let handled = self.panels_store.trading_container.get_mut(&pid)
                                            .map(|state| state.handle_click(local_id, x, y))
                                            .unwrap_or(false);
                                        if handled {
                                            self.sidebar_data_dirty = true;
                                            // Close/tab clicks restructure the tree —
                                            // persist the fresh snapshot. Body-focus
                                            // clicks are RAM-only — no write.
                                            if !local_id.starts_with("tcfree:body:") {
                                                self.autosave_snapshot();
                                            }
                                        }
                                    }
                                }
                                return;
                            }

                            // --- orders: — OrdersLog cancel / cancel-all clicks
                            // (W3, 2026-07-25). Only ever hosted inside a container
                            // (no standalone OrdersLog panel kind) — the click sets a
                            // pending flag on `OrdersLogState`, then
                            // `process_orders_log_pending_cancels` issues the actual
                            // `TradingMut::cancel_order` call(s).
                            if local_id.starts_with("orders:") {
                                if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use mlc_app_panels::panel_trait::TradingPanel;
                                    if let Some(mlc_core::state::Panel::TradingContainer(pid)) = item_opt {
                                        let handled = self.panels_store.trading_container.get_mut(&pid)
                                            .map(|state| state.handle_click(local_id, x, y))
                                            .unwrap_or(false);
                                        if handled {
                                            self.sidebar_data_dirty = true;
                                            self.process_orders_log_pending_cancels(pid);
                                        }
                                    }
                                }
                                return;
                            }

                            // --- Strip gears (owner spec 2026-07-28): the
                            // tape / metrics hover-gear opens the container's
                            // COMMON settings modal at SETUP.
                            if local_id == "tape:gear" || local_id == "metrics:gear" {
                                self.tc_modal.open();
                                self.tc_modal.section =
                                    mlc_app_layout::layout::modals::TcModalSection::Setup;
                                return;
                            }

                            // --- risk_calculator: — RiskCalculator editable-field
                            // clicks + the "Apply Size" button (W5, 2026-07-25).
                            // Standalone RiskCalculator panels have no OrderEntry
                            // sibling to apply into — Apply computes but has nowhere
                            // to write, matching this panel's pre-arc unwired state.
                            if local_id.starts_with("risk_calculator:") {
                                if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use mlc_app_panels::panel_trait::TradingPanel;
                                    let slot_prefix = format!("slot:{}:leaf:{}", idx, raw);
                                    match item_opt {
                                        Some(mlc_core::state::Panel::RiskCalculator(pid)) => {
                                            let handled = self.panels_store.risk_calculator.get_mut(&pid)
                                                .map(|state| state.handle_click(local_id, x, y))
                                                .unwrap_or(false);
                                            if handled {
                                                self.sidebar_data_dirty = true;
                                                if let Some(field) = Self::rc_field_for_local_id(local_id) {
                                                    self.start_rc_field_edit(RiskCalculatorTarget::Standalone(pid), &slot_prefix, field);
                                                } else if local_id == "risk_calculator:apply" {
                                                    self.commit_rc_field(RiskCalculatorTarget::Standalone(pid), &Self::rc_field_widget_id(&slot_prefix));
                                                    if let Some(state) = self.panels_store.risk_calculator.get_mut(&pid) {
                                                        if state.inputs_ready() { state.apply_requested = true; }
                                                    }
                                                }
                                                // Standalone has no OrderEntry sibling to apply
                                                // into — Apply computes but has nowhere to write
                                                // (pre-existing behavior, preserved verbatim).
                                                if let Some(state) = self.panels_store.risk_calculator.get_mut(&pid) {
                                                    state.apply_requested = false;
                                                }
                                            }
                                        }
                                        Some(mlc_core::state::Panel::TradingContainer(pid)) => {
                                            let handled = self.panels_store.trading_container.get_mut(&pid)
                                                .map(|state| state.handle_click(local_id, x, y))
                                                .unwrap_or(false);
                                            if handled {
                                                self.sidebar_data_dirty = true;
                                                if let Some(field) = Self::rc_field_for_local_id(local_id) {
                                                    self.start_rc_field_edit(RiskCalculatorTarget::Container(pid), &slot_prefix, field);
                                                } else if local_id == "risk_calculator:apply" {
                                                    self.commit_rc_field(RiskCalculatorTarget::Container(pid), &Self::rc_field_widget_id(&slot_prefix));
                                                    if let Some(state) = self.panels_store.trading_container.get_mut(&pid) {
                                                        if state.risk_calculator.inputs_ready() { state.risk_calculator.apply_requested = true; }
                                                    }
                                                    self.apply_risk_calculator_size_in_container(pid);
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                return;
                            }

                            // Multi-DOM has no in-panel clickable chrome — its
                            // config routes through the section-header gear
                            // (canonical dom_modal:* ids handled below).
                        }

                        // --- slot:{idx}:leaf:{leaf_id}:focus_content — click inside panel body ---
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":focus_content") {
                            if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                let leaf_id = uzor::panels::LeafId(raw);
                                self.sidebar_state.focused_free_leaf = Some((idx, leaf_id));
                                // Sync slot_source_mode to reflect the focused panel's current source.
                                let item_opt = self.sidebar_state.slot_dockings[idx]
                                    .inner()
                                    .tree()
                                    .leaf(leaf_id)
                                    .and_then(|l| l.active_panel().cloned());
                                // Workspace focus (2026-07 workspace-panel arc, Stage 3): a
                                // click landing inside the Trading tab's container body is
                                // one of the two gestures that set `WorkspaceFocus`
                                // explicitly (the other is a chart-canvas click,
                                // `handle_canvas_click`). This is the ONLY reachable body
                                // click path for the Trading slot — `is_trading_slot`
                                // rendering never registers the sibling `:focus` (no
                                // `_content`) widget, only this one. Every other slot kind
                                // (Dom/footprint/tape/...) leaves workspace focus alone —
                                // they have no workspace-panel concept of their own.
                                if let Some(mlc_core::state::Panel::TradingContainer(pid)) = &item_opt {
                                    self.workspace_focus = crate::focus::WorkspaceFocus::TradingPanel(*pid);
                                }
                                if let Some(item) = item_opt {
                                    use mlc_app_panels::trading::SymbolSource;
                                    use mlc_core::state::Panel;
                                    use mlc_app_sidebar_content::state::SlotSourceMode;
                                    use mlc_core::ids::SyncMemberId;
                                    let is_market_data = matches!(
                                        &item,
                                        Panel::Dom(_) | Panel::Footprint(_)
                                        | Panel::VolumeProfile(_) | Panel::LiquidityHeatmap(_)
                                        | Panel::BigTrades(_) | Panel::L2Tape(_)
                                        | Panel::TradeTape(_)
                                    );
                                    if is_market_data {
                                        let member = SyncMemberId::Panel(item.panel_id().0);
                                        let is_untagged = {
                                            let tags = self.core.tags();
                                            tags.tag_for_member(member)
                                                .and_then(|gid| tags.get(gid).map(|t| t.auto_created))
                                                .unwrap_or(true)
                                        };
                                        self.sidebar_state.slot_source_mode = if is_untagged {
                                            SlotSourceMode::Untagged
                                        } else {
                                            SlotSourceMode::Tagged
                                        };
                                    } else {
                                        let source = match &item {
                                            Panel::OrderEntry(id) => self.panels_store.order_entry.get(id).map(|s| s.source.clone()),
                                            Panel::TradingContainer(id) => self.panels_store.trading_container.get(id).map(|s| s.source.clone()),
                                            _ => None,
                                        };
                                        if let Some(src) = source {
                                            self.sidebar_state.slot_source_mode = match src {
                                                SymbolSource::HyperFocus => SlotSourceMode::Tagged,
                                                SymbolSource::Fixed { .. } => SlotSourceMode::Untagged,
                                                SymbolSource::BoundToChart { .. } => SlotSourceMode::Tagged,
                                            };
                                        }
                                    }
                                }
                                self.sidebar_data_dirty = true;
                            }
                            return;
                        }
                    }
                }
            }
        }

        // === Alert panel clicks ===
        // Pattern: "alert_delete_{id}"
        if widget_id.starts_with("alert_delete_") {
            if let Some(id) = widget_id.strip_prefix("alert_delete_").and_then(|s| s.parse::<u64>().ok()) {
                let (snapshot, bid_opt) = match self.core.alerts().find_owner(id) {
                    Some((bid, a)) => (Some(a), Some(bid)),
                    None => (None, None),
                };
                if let Some(bid) = bid_opt {
                    self.core.alerts_mut().remove(bid, id);
                }
                if let Some(snap) = snapshot {
                    self.push_undo_command(mlc_core::Command::DeleteAlert { snapshot: snap });
                }
                eprintln!("[Sidebar] Alert deleted: {}", id);
                self.sidebar_data_dirty = true;
                self.autosave_snapshot();
            }
            return;
        }
        // Pattern: "alert_add_button" — open alert settings modal for new alert
        if widget_id == "alert_add_button" {
            let price = self.panel_app.panel_grid.active_container()
                .and_then(|_w| None::<&mlc_core::Bar>)
                .map(|b| b.close)
                .unwrap_or(0.0);
            // S2.7.1e: read symbol via branch_meta (B1).
            let symbol: String = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.symbol_for_chart(cid))
                .map(String::from)
                .unwrap_or_else(|| "BTCUSD".to_string());
            let source = mlc_core::alert::AlertSource::Price { symbol: symbol.clone() };
            self.panel_app.alert_settings_state.open_new(source, &symbol, price);
            self.panel_app.alert_settings_state.pin_initial_position(
                self.content_rect.width, self.content_rect.height,
            );
            eprintln!("[Sidebar] Alert settings modal opened (new, price={:.2})", price);
            return;
        }
        // Pattern: "alert_{id}" (row click — navigate to alert's window, then open edit modal)
        if let Some(id) = widget_id.strip_prefix("alert_").and_then(|s| s.parse::<u64>().ok()) {
            let alert_data = self.core.alerts().find_owner(id).map(|(_, a)| a);
            if let Some(alert) = alert_data {
                let alert_id = alert.id;
                let window_id_hint = alert.window_id_hint;
                // Navigate to the window that created this alert, if known.
                if let Some(wid) = window_id_hint {
                    let chart_id = mlc_core::ChartId(wid);
                    if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_container_id(chart_id) {
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        eprintln!("[Sidebar] Alert {}: navigated to window {} (leaf {:?})", alert_id, wid, leaf_id);
                    }
                }
                // Open the edit modal.
                self.panel_app.alert_settings_state.open_edit(&alert);
                self.panel_app.alert_settings_state.pin_initial_position(
                    self.content_rect.width, self.content_rect.height,
                );
                eprintln!("[Sidebar] Alert edit opened: id={}", alert_id);
            }
            return;
        }

        // === Object tree panel clicks ===
        // Widget IDs use namespaced format: "{section_tag}_{prefix}_{action}_{id}"
        // where section_tag is one of: grp, win, mem, flt (flat/untagged).
        // Legacy (non-namespaced) format "drw_{action}_{id}" is also matched for
        // backwards compatibility with in-flight autosave data.
        //
        // All handlers extract the trailing numeric ID via rsplit('_').next() so
        // they work regardless of how many namespace prefixes are prepended.
        //
        // Buttons: *_drw_delete_{id}, *_drw_settings_{id}, *_drw_vis_{id}, etc.
        // Row:     *_drw_{id}  (where suffix after last underscore is a pure number)

        // --- Drawing delete ---
        if widget_id.contains("_drw_delete_") || widget_id.starts_with("drw_delete_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                // 1. Try active bubble first (handles Active + WindowOtherKey).
                let active_bid_del = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                let removed_from_dm = active_bid_del
                    .map(|bid| self.core.drawings_mut().delete(bid, id))
                    .unwrap_or(false);

                if removed_from_dm {
                    self.sync_drawing_back_to_group();
                    self.autosave_snapshot();
                    self.sidebar_data_dirty = true;
                    eprintln!("[Sidebar] Drawing deleted from drawing_manager: {}", id);
                    return;
                }

                // 2. Try auto_bubble.primitives (WindowStash).
                let active_chart_id = self.panel_app.panel_grid.active_container().map(|w| w.id);
                let removed_from_auto_bubble = if let Some(cid) = active_chart_id {
                    let auto_bid = self.panel_app.panel_grid.containers()
                        .get(&cid)
                        .map(|w| w.auto_tag_id);
                    if let Some(bid) = auto_bid {
                        self.core.drawings_mut().delete(bid, id)
                    } else {
                        false
                    }
                } else {
                    false
                };

                if removed_from_auto_bubble {
                    self.autosave_snapshot();
                    self.sidebar_data_dirty = true;
                    eprintln!("[Sidebar] Drawing deleted from auto_bubble.primitives: {}", id);
                    return;
                }

                // 3. Try group.primitives (GroupOtherKey).
                let group_id = self.panel_app.panel_grid
                    .active_container_id()
                    .and_then(|cid| self.bubble_for_chart(cid));
                if let Some(gid) = group_id {
                    let deleted = self.core.drawings_mut().delete(gid, id);
                    if deleted {
                        self.autosave_snapshot();
                        self.sidebar_data_dirty = true;
                        eprintln!("[Sidebar] Drawing deleted from group.primitives: {}", id);
                        return;
                    }
                }

                eprintln!("[Sidebar] drw_delete_{}: not found in any store", id);
            }
            return;
        }
        // --- Indicator delete ---
        if widget_id.contains("_ind_delete_") || widget_id.starts_with("ind_delete_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                // Try live indicator_manager first (Active + WindowOtherKey).
                if self.core.indicators().exists(id) {
                    self.delete_indicator_instance(id);
                    return;
                }

                // Fall through: try group.indicator_configs (GroupIndicatorOtherKey).
                let group_id = self.panel_app.panel_grid
                    .active_container_id()
                    .and_then(|cid| self.bubble_for_chart(cid));
                if let Some(gid) = group_id {
                    let deleted = self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                        let before = group.indicator_configs.len();
                        group.indicator_configs.retain(|cfg| cfg.id != id);
                        group.indicator_configs.len() < before
                    }).unwrap_or(false);
                    if deleted {
                        self.autosave_snapshot();
                        self.sidebar_data_dirty = true;
                        eprintln!("[Sidebar] Group indicator config deleted: {}", id);
                    }
                }
            }
            return;
        }
        // --- Drawing settings ---
        if widget_id.contains("_drw_settings_") || widget_id.starts_with("drw_settings_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                let active_bid_settings = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if let Some(bid) = active_bid_settings {
                    if self.core.drawings().get_data(bid, id).is_some() {
                        self.panel_app.primitive_settings_state.open(id);
                        eprintln!("[Sidebar] Primitive settings opened: {}", id);
                    }
                }
            }
            return;
        }
        // --- Indicator settings ---
        if widget_id.contains("_ind_settings_") || widget_id.starts_with("ind_settings_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                self.panel_app.indicator_settings_state.open(id);
                eprintln!("[Sidebar] Indicator settings opened: {}", id);
            }
            return;
        }
        // --- Drawing visibility ---
        if widget_id.contains("_drw_vis_") || widget_id.starts_with("drw_vis_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                let active_bid_vis = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if let Some(bid) = active_bid_vis {
                    self.core.drawings_mut().toggle_visibility(bid, id);
                }
                self.sidebar_data_dirty = true;
                eprintln!("[Sidebar] Drawing visibility toggled: {}", id);
            }
            return;
        }
        // --- Indicator visibility ---
        if widget_id.contains("_ind_vis_") || widget_id.starts_with("ind_vis_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                self.core.indicators_mut().toggle_visibility(id);
                self.sidebar_data_dirty = true;
                eprintln!("[Sidebar] Indicator visibility toggled: {}", id);
            }
            return;
        }
        // --- Drawing lock ---
        if widget_id.contains("_drw_lock_") || widget_id.starts_with("drw_lock_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                let active_bid_lock = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if let Some(bid) = active_bid_lock {
                    self.core.drawings_mut().toggle_lock_primitive(bid, id);
                }
                self.sidebar_data_dirty = true;
                eprintln!("[Sidebar] Drawing lock toggled: {}", id);
            }
            return;
        }
        // --- Indicator lock (no-op for now, indicators don't have lock) ---
        if widget_id.contains("_ind_lock_") || widget_id.starts_with("ind_lock_") {
            return;
        }

        // === Compare overlay object tree handlers ===
        // Widget IDs use the "{section}_{key}_cmp_*" prefix layout that the
        // sidebar renderer produces (see `mlc-app-sidebar-content/src/render.rs`
        // around line 2205 — `widget_prefix = "{section_tag}_{key_slug}"`).
        // Mirror what `_drw_delete_` / `_ind_delete_` do: accept both the
        // prefixed contains-form AND the bare starts_with form so legacy ids
        // (if any sidebar path still emits them) still route.

        // --- Compare delete ---
        if widget_id.contains("_cmp_delete_") || widget_id.starts_with("cmp_delete_") {
            if let Some(idx) = widget_id.rsplit("cmp_delete_").next()
                .and_then(|s| s.parse::<usize>().ok())
            {
                let removed = if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.compare_overlay.series.get(idx).cloned()
                        .and_then(|snap| {
                            window.compare_overlay.remove_series(idx).map(|sym| (sym, snap))
                        })
                } else {
                    None
                };
                if let Some((symbol, snapshot)) = removed {
                    self.push_undo_command(mlc_core::Command::RemoveCompareSeries {
                        symbol: symbol.clone(),
                        series: snapshot.without_bars(),
                    });
                    eprintln!("[Sidebar] Compare series removed: {} (idx={})", symbol, idx);
                }
            }
            return;
        }
        // --- Compare visibility toggle ---
        if widget_id.contains("_cmp_vis_") || widget_id.starts_with("cmp_vis_") {
            if let Some(idx) = widget_id.rsplit("cmp_vis_").next()
                .and_then(|s| s.parse::<usize>().ok())
            {
                let (symbol_for_undo, prev_vis, new_vis) = if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    let prev = window.compare_overlay.series.get(idx).map(|s| (s.symbol.clone(), s.visible));
                    let new_vis = window.compare_overlay.toggle_visibility(idx);
                    match prev {
                        Some((sym, pv)) => (Some(sym), pv, new_vis),
                        None => (None, false, new_vis),
                    }
                } else {
                    (None, false, false)
                };
                if let Some(symbol) = symbol_for_undo {
                    self.push_undo_command(mlc_core::Command::SetCompareSeriesVisibility {
                        symbol: symbol.clone(),
                        visible: new_vis,
                        previous: prev_vis,
                    });
                    eprintln!("[Sidebar] Compare visibility toggled: {} idx={} -> visible={}", symbol, idx, new_vis);
                }
            }
            return;
        }
        // --- Compare settings / alert / lock — intentionally inert ---
        // Compare series do not carry per-series settings, alerts or lock state
        // the way drawings / indicators do (the line is a derived overlay on
        // top of the host chart's bars, not a first-class object).  The
        // sidebar renderer should grey these icons out for compare rows; we
        // still swallow the click here so no other handler picks it up.
        if widget_id.contains("_cmp_settings_") || widget_id.starts_with("cmp_settings_")
            || widget_id.contains("_cmp_alert_") || widget_id.starts_with("cmp_alert_")
            || widget_id.contains("_cmp_lock_") || widget_id.starts_with("cmp_lock_")
        {
            return;
        }
        // --- Compare row click (row selection) ---
        {
            let cmp_id = widget_id
                .strip_prefix("cmp_")
                .and_then(|s| s.parse::<u64>().ok());
            if let Some(id) = cmp_id {
                if self.sidebar_state.object_tree_items.iter().any(|item| {
                    item.id == id && item.category == mlc_core::ObjectCategory::Compare
                }) {
                    for item in &mut self.sidebar_state.object_tree_items {
                        item.selected = item.id == id && item.category == mlc_core::ObjectCategory::Compare;
                    }
                    eprintln!("[Sidebar] Compare series selected: idx={}", id);
                }
                return;
            }
        }

        // --- Drawing alert button ---
        if widget_id.contains("_drw_alert_") || widget_id.starts_with("drw_alert_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                let mut price = self.panel_app.panel_grid.active_container()
                    .and_then(|_w| None::<&mlc_core::Bar>)
                    .map(|b| b.close)
                    .unwrap_or(0.0);
                let mut source_name = format!("Drawing {}", id);
                let active_bid_alert = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if let Some(bid) = active_bid_alert {
                    let guard = self.core.drawings();
                    if let Some(prim) = guard.primitives(bid).iter().find(|p| p.data().id == id) {
                        source_name = localize_primitive_name(prim.type_id(), prim.display_name());
                        let pts = prim.points();
                        if !pts.is_empty() {
                            price = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
                        }
                    }
                }
                // S2.7.1e: read symbol via branch_meta (B1).
                let symbol: String = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_else(|| "BTCUSD".to_string());
                let source = mlc_core::alert::AlertSource::Drawing { primitive_id: id, label: source_name.clone() };
                self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                self.panel_app.alert_settings_state.pin_initial_position(
                    self.content_rect.width, self.content_rect.height,
                );
                eprintln!("[Sidebar] Alert settings opened for drawing {}", id);
            }
            return;
        }
        // --- Indicator alert button ---
        if widget_id.contains("_ind_alert_") || widget_id.starts_with("ind_alert_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                let price = self.panel_app.panel_grid.active_container()
                    .and_then(|_w| None::<&mlc_core::Bar>)
                    .map(|b| b.close)
                    .unwrap_or(0.0);
                let source_name = self.core.indicators().name_of(id)
                    .unwrap_or_else(|| format!("Indicator {}", id));
                // S2.7.1e: read symbol via branch_meta (B1).
                let symbol: String = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_else(|| "BTCUSD".to_string());
                let source = mlc_core::alert::AlertSource::Signal {
                    indicator_id: id,
                    label: source_name.clone(),
                    direction_filter: mlc_core::alert::SignalDirection::Any,
                    bar_state: mlc_core::alert::SignalBarState::Forming,
                    kind_filter: None,
                };
                self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                self.panel_app.alert_settings_state.pin_initial_position(
                    self.content_rect.width, self.content_rect.height,
                );
                eprintln!("[Sidebar] Alert settings opened for indicator {}", id);
            }
            return;
        }
        // --- Alert bell on drawing primitive — opens the existing alert edit modal ---
        // Widget ID: "alert_bell_drw_{primitive_id}"
        if widget_id.starts_with("alert_bell_drw_") {
            if let Some(prim_id) = widget_id.strip_prefix("alert_bell_drw_").and_then(|s| s.parse::<u64>().ok()) {
                let alert = self.core.alerts().items_all().into_iter().find(|a| matches!(
                    &a.source,
                    mlc_core::alert::AlertSource::Drawing { primitive_id, .. } if *primitive_id == prim_id
                ) && a.status == mlc_core::alert::AlertStatus::Active);
                if let Some(alert) = alert {
                    self.panel_app.alert_settings_state.open_edit(&alert);
                    self.panel_app.alert_settings_state.pin_initial_position(
                        self.content_rect.width, self.content_rect.height,
                    );
                    eprintln!("[ChartApp] Bell click: opened edit modal for drawing alert id={}", alert.id);
                }
            }
            return;
        }
        // --- Alert bell on indicator — opens the existing alert edit modal ---
        // Widget ID: "alert_bell_ind_{indicator_id}"
        if widget_id.starts_with("alert_bell_ind_") {
            if let Some(ind_id) = widget_id.strip_prefix("alert_bell_ind_").and_then(|s| s.parse::<u64>().ok()) {
                let alert = self.core.alerts().items_all().into_iter().find(|a| matches!(
                    &a.source,
                    mlc_core::alert::AlertSource::Signal { indicator_id, .. } if *indicator_id == ind_id
                ) && a.status == mlc_core::alert::AlertStatus::Active);
                if let Some(alert) = alert {
                    self.panel_app.alert_settings_state.open_edit(&alert);
                    self.panel_app.alert_settings_state.pin_initial_position(
                        self.content_rect.width, self.content_rect.height,
                    );
                    eprintln!("[ChartApp] Bell click: opened edit modal for signal alert id={}", alert.id);
                }
            }
            return;
        }
        // --- Row selection ({section_tag}_drw_{id} or {section_tag}_ind_{id}) ---
        // Also matches legacy bare "drw_{id}" / "ind_{id}" for backwards compatibility.
        // The widget ID contains "_drw_" or "_ind_" (or starts with one of those), and
        // the trailing component after the last underscore is a pure numeric ID.
        // We guard with contains("_drw_") / contains("_ind_") plus a starts_with check
        // to avoid catching "ind_overlay:toggle" and similar unrelated widget IDs.
        {
            let is_drw_row = widget_id.contains("_drw_") || widget_id.starts_with("drw_");
            let is_ind_row = widget_id.contains("_ind_") || widget_id.starts_with("ind_");
            // Extra guard: the widget_id must not contain ':' (rules out "ind_overlay:toggle").
            let row_id = if (is_drw_row || is_ind_row) && !widget_id.contains(':') {
                widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok())
            } else {
                None
            };
            if let Some(id) = row_id {
                if self.sidebar_state.object_tree_items.iter().any(|item| item.id == id) {
                    for item in &mut self.sidebar_state.object_tree_items {
                        item.selected = item.id == id;
                    }
                    eprintln!("[Sidebar] Object selected: {}", id);

                    // Click-to-center: if a drawing row was clicked, the active
                    // leaf does NOT center itself directly — it raises a
                    // "center this viewport" intent on the sync TAG, which then
                    // propagates to every sync-viewport peer (including the
                    // initiator). Centering is X (time) only + force Auto-Y on
                    // each member: Auto re-fits each peer's own price range to
                    // the now-visible X window, which resolves the
                    // different-instrument / different-scale-mode cases without
                    // copying a price band that would be meaningless on a peer
                    // (and without the old Manual-mode force that broke peers).
                    if is_drw_row {
                        let active_bid_center = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                        let center_bars: Vec<mlc_core::Bar> = self.active_chart_bars();
                        // Mid timestamp from the primitive's anchor points. Price
                        // anchors (the `.1`) are no longer used for framing — Auto
                        // handles Y — so sentinel (0.0) prices are irrelevant here.
                        let mid_bar = active_bid_center
                            .and_then(|bid| {
                                let guard = self.core.drawings();
                                guard.primitives(bid).iter()
                                    .find(|p| p.data().id == id)
                                    .and_then(|prim| {
                                        let pts = prim.points();
                                        if pts.is_empty() {
                                            return None;
                                        }
                                        let avg_ts = pts.iter().map(|p| p.0).sum::<i64>() / pts.len() as i64;
                                        self.panel_app.panel_grid.active_container()
                                            .map(|_w| mlc_core::timestamp_ms_to_bar_f64(&center_bars[..], avg_ts))
                                    })
                            });
                        if let Some(mid_bar) = mid_bar {
                            use mlc_core::chart::types::price_scale::ScaleMode;
                            // Audit D8: X anchor above is always kline-ts (drawings
                            // are ts-anchored regardless of chart_type); the Y
                            // autoscale must key off whatever THIS window renders.
                            let center_series = self.active_chart_active_series();
                            // Center the active leaf on X and switch it to Auto-Y.
                            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                window.center_on_bar_auto(mid_bar, center_series.as_active_series());
                            }
                            // Raise the center intent on the tag: propagate X +
                            // Auto-Y to every sync-viewport peer. The peer applies
                            // the X anchor in its own coordinates (time→bar) and
                            // Auto re-fits its own Y. No scale-mode is forced to
                            // Manual; no price band is copied across instruments.
                            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                                let (vs, bsp) = self.panel_app.panel_grid.active_container()
                                    .map(|w| (w.viewport().view_start, w.viewport().bar_spacing))
                                    .unwrap_or((0.0, 8.0));
                                self.propagate_viewport_to_sync_group(active_leaf, vs, bsp, Some(ScaleMode::Auto));
                                // Persist Auto into the sync group's stored mode too.
                                // propagate_* only writes the live windows; the
                                // DataBubble.scale_mode is the authority that any
                                // later group-join / symbol-switch / Y-zoom re-applies
                                // to members. Without updating it, the bubble still
                                // holds Manual and the next such event reverts the
                                // whole group back to M — which is exactly the
                                // "center switches to M not A" report.
                                if let Some(gid) = self.panel_app.panel_grid
                                    .container_id_for_leaf(active_leaf)
                                    .and_then(|cid| self.bubble_for_chart(cid))
                                {
                                    self.core.bubbles_mut().set_scale_mode(gid, ScaleMode::Auto);
                                }
                            }
                        }
                    }
                }
                return;
            }
        }

        // === Signal panel clicks ===
        // Pattern: "signal_{instance_id}_{bar_index}" (individual signal row — must come before signal_group_)
        if widget_id.starts_with("signal_") && !widget_id.starts_with("signal_group_") {
            let parts: Vec<&str> = widget_id.strip_prefix("signal_").unwrap_or_default().splitn(2, '_').collect();
            if parts.len() == 2 {
                if let Ok(bar_index) = parts[1].parse::<i64>() {
                    let signal_series = self.active_chart_active_series();
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.center_on_bar(bar_index as f64, signal_series.as_active_series());
                    }
                    eprintln!("[Sidebar] Signal clicked: center on bar {}", bar_index);
                }
            }
            return;
        }
        // Pattern: "signal_group_{id}"
        if widget_id.starts_with("signal_group_") {
            if let Some(id) = widget_id.strip_prefix("signal_group_").and_then(|s| s.parse::<u64>().ok()) {
                if self.sidebar_state.collapsed_signal_groups.contains(&id) {
                    self.sidebar_state.collapsed_signal_groups.remove(&id);
                } else {
                    self.sidebar_state.collapsed_signal_groups.insert(id);
                }
                eprintln!("[Sidebar] Signal group toggled: {}", id);
            }
            return;
        }

        // === Toolbar widgets ===
        if widget_id.starts_with("toolbar:") || widget_id.starts_with("dtb:") || widget_id.starts_with("csb:") || widget_id.starts_with("btb:") || widget_id.starts_with("rtb:") || widget_id.starts_with("ilb:") || widget_id.starts_with("tch:") {
            let item_id = widget_id
                .strip_prefix("toolbar:")
                .or_else(|| widget_id.strip_prefix("dtb:"))
                .or_else(|| widget_id.strip_prefix("csb:"))
                .or_else(|| widget_id.strip_prefix("btb:"))
                .or_else(|| widget_id.strip_prefix("rtb:"))
                .or_else(|| widget_id.strip_prefix("ilb:"))
                .or_else(|| widget_id.strip_prefix("tch:"))
                .unwrap_or(widget_id)
                .to_string();

            // ── Trading container embedded header toolbar (`tch:` instance,
            // owner spec 2026-07-25). Same item ids as the main topbar,
            // dispatch target = the container. Mutual dropdown exclusion:
            // touching one instance closes the other's open dropdown.
            let from_tch = widget_id.starts_with("tch:");
            if from_tch {
                self.panel_app.toolbar_state.open_dropdown_id = None;
                let tc_pid = self.panels_store.trading_container.keys().next().copied();
                // Data-section header (global-docking Stage B): same tch
                // instance, dispatch target = the section's panel instead
                // of the container.
                let section_item = {
                    use mlc_app_sidebar_content::state::RightSidebarPanel as RSP;
                    let rp = self.sidebar_state.right_panel;
                    if matches!(rp, RSP::SlotDom | RSP::SlotTape | RSP::SlotL2 | RSP::SlotBigTrades) {
                        self.section_panel_for(rp)
                    } else {
                        None
                    }
                };
                match item_id.as_str() {
                    "agent_mode" => {
                        // Agents mode toggle-click: PTY <-> Chat.
                        self.trading_header_tb.open_dropdown_id = None;
                        self.sidebar_state.agent_spawn_mode =
                            if self.sidebar_state.agent_spawn_mode == gate4agent::InstanceMode::Pty {
                                gate4agent::InstanceMode::Chat
                            } else {
                                gate4agent::InstanceMode::Pty
                            };
                        self.sidebar_data_dirty = true;
                        return;
                    }
                    "agent_backend" => {
                        // Backend dropdown — shared composer machinery.
                        if self.trading_header_tb.open_dropdown_id.as_deref() == Some("agent_backend") {
                            self.trading_header_tb.open_dropdown_id = None;
                        } else {
                            self.trading_header_tb.open_dropdown_id = Some("agent_backend".to_string());
                        }
                        return;
                    }
                    "sec_gear" => {
                        // Section gear → the section's settings modal
                        // (DOM: dual-mode modal; TT/L2/BT: columns/thresholds;
                        // Agents: agents settings modal).
                        self.trading_header_tb.open_dropdown_id = None;
                        let is_agents = self.sidebar_state.right_panel
                            == mlc_app_sidebar_content::state::RightSidebarPanel::Agents;
                        if section_item.is_some() || is_agents {
                            self.dom_modal.open();
                        }
                        return;
                    }
                    "sec_mx" | "sec_am" | "sec_filter" | "sec_tick" | "sec_imb" => {
                        self.trading_header_tb.open_dropdown_id = None;
                        if let Some(mlc_core::state::Panel::Dom(pid)) = section_item {
                            match item_id.as_str() {
                                "sec_mx" => {
                                    let sub_info = self.panels_store.dom.get_mut(&pid).map(|state| {
                                        state.set_multi_mode(!state.multi_mode);
                                        (state.multi_mode, state.multi.symbol.clone(), state.multi.config.exchanges.clone())
                                    });
                                    if let Some((on, sym, exchanges)) = sub_info {
                                        if on {
                                            self.subscribe_multi_roster(&sym, &exchanges);
                                        }
                                        self.autosave_snapshot();
                                    }
                                }
                                "sec_am" => {
                                    // L/F toggle: Locked recenters on mid and
                                    // follows it; Free keeps the scrolled view.
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        if state.multi_mode {
                                            let to = !state.multi.locked;
                                            state.multi.set_locked(to);
                                        } else {
                                            state.auto_center = !state.auto_center;
                                            if state.auto_center && state.market_price > 0.0 {
                                                state.center_price = state.market_price;
                                            }
                                        }
                                    }
                                }
                                "sec_imb" => {
                                    // Imb strip toggle (multi only) — plain
                                    // chip label = on, struck = off.
                                    let mut changed = false;
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        if state.multi_mode {
                                            state.multi.config.show_imbalance = !state.multi.config.show_imbalance;
                                            changed = true;
                                        }
                                    }
                                    if changed {
                                        self.autosave_snapshot();
                                    }
                                }
                                "sec_filter" => {
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        if state.multi_mode {
                                            // Multi: minimum level qty to draw.
                                            state.multi.min_level_qty = match state.multi.min_level_qty {
                                                v if v < 0.05 => 0.1,
                                                v if v < 0.15 => 0.5,
                                                v if v < 0.55 => 1.0,
                                                v if v < 1.5 => 5.0,
                                                v if v < 5.5 => 10.0,
                                                _ => 0.0,
                                            };
                                        } else {
                                            state.min_volume_filter = match state.min_volume_filter {
                                                v if v < 0.5 => 1.0,
                                                v if v < 1.5 => 5.0,
                                                v if v < 5.5 => 10.0,
                                                v if v < 10.5 => 25.0,
                                                v if v < 25.5 => 50.0,
                                                _ => 0.0,
                                            };
                                        }
                                    }
                                }
                                _ => {
                                    // sec_tick — single: cycle tick size;
                                    // multi: cycle the bucket GRID.
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        if state.multi_mode {
                                            state.multi.cycle_grid_size();
                                        } else {
                                            const TICKS: &[f64] = &[0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0, 100.0];
                                            let cur = state.tick_size;
                                            let next = TICKS.iter()
                                                .find(|&&t| t > cur * 1.001)
                                                .copied()
                                                .unwrap_or(TICKS[0]);
                                            state.tick_size = next;
                                            state.volume_by_price.clear();
                                        }
                                    }
                                }
                            }
                            self.sidebar_data_dirty = true;
                        }
                        return;
                    }
                    "__chevron_left" | "__chevron_right" => {
                        let forward = item_id == "__chevron_right";
                        let ms = self.last_trading_header_result.as_ref()
                            .map(|r| r.top_max_scroll)
                            .unwrap_or(0.0);
                        self.trading_header_tb.handle_chevron_click("top", forward, ms);
                        return;
                    }
                    "symbol_selector" => {
                        if let Some(item) = &section_item {
                            self.open_section_symbol_quick_edit(item);
                            self.trading_header_tb.open_dropdown_id = Some("symbol_selector".to_string());
                        } else if let Some(pid) = tc_pid {
                            self.open_trading_symbol_quick_edit(pid);
                            // The results list is the composer's own
                            // `symbol_selector` dropdown on THIS instance.
                            self.trading_header_tb.open_dropdown_id = Some("symbol_selector".to_string());
                        }
                        return;
                    }
                    "timeframe_selector" => {
                        if self.trading_header_tb.open_dropdown_id.as_deref() == Some("timeframe_selector") {
                            self.trading_header_tb.open_dropdown_id = None;
                        } else {
                            self.trading_header_tb.open_dropdown_id = Some("timeframe_selector".to_string());
                        }
                        return;
                    }
                    "tc_color_tag" => {
                        self.trading_header_tb.open_dropdown_id = None;
                        let target_pid = section_item.as_ref().map(|i| i.panel_id()).or(tc_pid);
                        if let Some(pid) = target_pid {
                            let anchor = self.last_trading_header_result.as_ref()
                                .and_then(|r| r.top_toolbar.item_rects.iter().find(|(id, _)| id == "tc_color_tag").map(|(_, rect)| *rect));
                            if let Some(rect) = anchor {
                                self.panel_app.sync_color_grid.open_for_panel(
                                    pid.0,
                                    rect.x,
                                    rect.y + rect.height,
                                    self.width as f64,
                                    self.height as f64,
                                );
                            }
                        }
                        return;
                    }
                    "tc_center" => {
                        if self.trading_header_tb.open_dropdown_id.as_deref() == Some("tc_center") {
                            self.trading_header_tb.open_dropdown_id = None;
                        } else {
                            self.trading_header_tb.open_dropdown_id = Some("tc_center".to_string());
                        }
                        return;
                    }
                    "tc_spawn" => {
                        // Free-mode "+ Panel" spawn dropdown — same
                        // toggle-open contract as tc_center; the picked row
                        // routes through the `dropdown_id == "tc_spawn"` arm.
                        if self.trading_header_tb.open_dropdown_id.as_deref() == Some("tc_spawn") {
                            self.trading_header_tb.open_dropdown_id = None;
                        } else {
                            self.trading_header_tb.open_dropdown_id = Some("tc_spawn".to_string());
                        }
                        return;
                    }
                    "tc_center_swap" => {
                        // Fast swap (owner spec 2026-07-28): the icon half
                        // of the split control toggles DOM<->Chart in one
                        // click; the labeled half keeps the dropdown.
                        use mlc_app_panels::trading::trading::trading_container::CenterKind;
                        self.trading_header_tb.open_dropdown_id = None;
                        if let Some(pid) = tc_pid {
                            if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                                tc.center_kind = match tc.center_kind {
                                    CenterKind::Dom => CenterKind::Chart,
                                    CenterKind::Chart => CenterKind::Dom,
                                };
                            }
                            self.autosave_snapshot();
                        }
                        return;
                    }
                    "tc_gear" => {
                        // Trading Container modal (owner spec 2026-07-28) —
                        // card MAP of the hosted panels + setup section.
                        self.trading_header_tb.open_dropdown_id = None;
                        self.tc_modal.open();
                        return;
                    }
                    "chart_type_selector" => {
                        // Chart-mode control (embedded center chart) — same
                        // toggle-open contract as timeframe_selector above.
                        if self.trading_header_tb.open_dropdown_id.as_deref() == Some("chart_type_selector") {
                            self.trading_header_tb.open_dropdown_id = None;
                        } else {
                            self.trading_header_tb.open_dropdown_id = Some("chart_type_selector".to_string());
                        }
                        return;
                    }
                    "indicators" => {
                        // Focus the trading panel FIRST — the indicator
                        // search modal's commit path routes by
                        // `workspace_focus`, so the picked indicator lands
                        // on the embedded chart's bubble.
                        self.trading_header_tb.open_dropdown_id = None;
                        if let Some(pid) = tc_pid {
                            self.workspace_focus = crate::focus::WorkspaceFocus::TradingPanel(pid);
                        }
                        self.process_chart_out_event(mlc_core::events::ChartOutEvent::ToggleIndicators);
                        return;
                    }
                    _ => {}
                }
                // Pinned-favorites quick-pick buttons on the header instance
                // set the CONTAINER timeframe, never the active chart's.
                if let Some(value_id) = item_id.strip_prefix(mlc_core::chart_type::GRANULARITY_QUICK_PICK_PREFIX) {
                    if let Some(target) = self.resolve_granularity_target(value_id) {
                        if let (Some(pid), mlc_core::chart_type::GranularityTarget::Tf { name })
                            | (Some(pid), mlc_core::chart_type::GranularityTarget::Interval { name }) = (tc_pid, target)
                        {
                            self.set_trading_container_timeframe(pid, &name);
                        }
                    }
                    return;
                }
                // Anything else on the header instance is a no-op — never
                // fall through to the chart-window toolbar handler below.
                return;
            }
            // Main-toolbar interaction closes the header instance's dropdown.
            self.trading_header_tb.open_dropdown_id = None;

            // Chevron scroll buttons — handle before generic toolbar dispatch.
            if item_id == "__chevron_left" || item_id == "__chevron_right" {
                let forward = item_id == "__chevron_right";
                // Map widget prefix to toolbar name and fetch max_scroll from last result.
                let (toolbar_name, max_scroll) = if widget_id.starts_with("csb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.top_max_scroll).unwrap_or(0.0);
                    ("top", ms)
                } else if widget_id.starts_with("btb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.bottom_max_scroll).unwrap_or(0.0);
                    ("bottom", ms)
                } else if widget_id.starts_with("dtb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.left_max_scroll).unwrap_or(0.0);
                    ("left", ms)
                } else if widget_id.starts_with("rtb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.right_max_scroll).unwrap_or(0.0);
                    ("right", ms)
                } else {
                    return;
                };
                eprintln!("[ChartApp] chevron click: toolbar={} forward={} max_scroll={}", toolbar_name, forward, max_scroll);
                self.panel_app.toolbar_state.handle_chevron_click(toolbar_name, forward, max_scroll);
                return;
            }

            // Clock button — toggle clock popup
            if item_id == "clock" {
                self.panel_app.clock_popup_state.toggle(0.0, 0.0);
                return;
            }

            // Expand button — toggle expand mode in the split grid.
            if item_id == "expand" {
                self.panel_app.panel_grid.toggle_expand();
                eprintln!("[ChartApp] Expand button clicked, expanded={}", self.panel_app.panel_grid.is_expanded());
                return;
            }

            // chart_settings button opens/closes the chart settings modal directly.
            if item_id == "chart_settings" {
                self.panel_app.chart_settings_state.toggle();
                eprintln!("[ChartApp] chart_settings modal toggled: {}", self.panel_app.chart_settings_state.is_open);
                return;
            }

            // Undo/Redo — intercept before handle_toolbar_click_with_chart() which
            // returns Consumed without executing the command.  Command history lives
            // on ChartContainer, so we extract the command here and apply it.
            if item_id == "undo" {
                self.perform_undo();
                return;
            }
            if item_id == "redo" {
                self.perform_redo();
                return;
            }
            if item_id == "screenshot" {
                self.request_screenshot();
                return;
            }

            // Pinned-favorites quick-pick buttons on the top toolbar (user-
            // granularity-templates-inner arc, кусок 3 — TradingView
            // "pinned intervals" style; extended per-family scoping arc
            // 2026-07-16 for footprint bucket intervals). Reuses the SAME
            // `tf_{name}` / `{GRANULARITY_INTERVAL_ID_PREFIX}{name}` /
            // `{RESOLUTION_PARAM_ITEM_PREFIX}{value}` id encoding the
            // dropdown rows use — resolve back to a target and dispatch
            // `ChangeTimeframe`/`UpdateChartTypeParam` directly, no dropdown
            // involved. `Interval` dispatches `ChangeTimeframe` too (the
            // SAME `is_interval_resolution` branch inside its handler routes
            // it through `apply_footprint_interval`, not a plain kline TF
            // switch — see that handler's doc comment).
            if let Some(value_id) = item_id.strip_prefix(mlc_core::chart_type::GRANULARITY_QUICK_PICK_PREFIX) {
                if let Some(target) = self.resolve_granularity_target(value_id) {
                    match target {
                        mlc_core::chart_type::GranularityTarget::Tf { name } => {
                            self.process_chart_out_event(mlc_core::events::ChartOutEvent::ChangeTimeframe {
                                timeframe_id: format!("tf_{}", name),
                            });
                        }
                        mlc_core::chart_type::GranularityTarget::Param { value, .. } => {
                            self.process_chart_out_event(mlc_core::events::ChartOutEvent::UpdateChartTypeParam { value });
                        }
                        mlc_core::chart_type::GranularityTarget::Interval { name } => {
                            self.process_chart_out_event(mlc_core::events::ChartOutEvent::ChangeTimeframe {
                                timeframe_id: format!("tf_{}", name),
                            });
                        }
                    }
                }
                return;
            }

            // Close inline dropdowns when clicking any toolbar button that is not
            // an inline dropdown item or the dropdown menu triggers themselves.
            if !item_id.starts_with("inline:style_option:")
                && !item_id.starts_with("inline:width_option:")
                && item_id != "inline:style_menu"
                && item_id != "inline:width_menu"
                && item_id != "inline_dropdown:__bg__"
            {
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            // Bar background absorber — gap click between inline toolbar buttons.
            // Absorb the click so the primitive stays selected; do nothing else.
            if item_id == "__bg__" {
                return;
            }

            // inline:* actions come from the inline primitive toolbar embedded in
            // the control strip.  They must be intercepted here because
            // handle_toolbar_click_with_chart() does not know about them.
            if item_id.starts_with("inline:") || item_id == "inline_dropdown:__bg__" {
                self.handle_inline_action(&item_id);
                return;
            }

            // Split borrows: toolbar_state and panel_grid are separate fields of panel_app,
            // and panels_store is a separate field of self.
            let active_bid_tb = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            let window_ptr: *mut _ = self.panel_app.panel_grid.active_container_mut()
                .map(|w| w as *mut _)
                .unwrap_or(std::ptr::null_mut::<crate::chart_container::ChartContainer>());

            if !window_ptr.is_null() {
                // SAFETY: toolbar_state does not access panel_grid.
                let window = unsafe { &mut *window_ptr };
                if let Some(bid) = active_bid_tb {
                    let mut guard = self.core.drawings_mut();
                    let mut session = crate::input::drawing_session::DrawingSessionView::new(&mut guard, bid);
                    let events = self.panel_app.toolbar_state.handle_toolbar_click_with_chart(
                        &item_id,
                        &mut window.crosshair,
                        &mut session,
                    );
                    drop(guard);
                    for event in events {
                        self.process_chart_out_event(event);
                    }
                }
            }
            return;
        }

        // === Dropdown items ===
        if let Some(rest) = widget_id.strip_prefix("dropdown:") {
            // Which toolbar instance owns the currently open dropdown —
            // the trading container's embedded header (`tch:`) or the main
            // topbar. Only one can be open at a time (mutual exclusion is
            // enforced at open time), so ownership is unambiguous.
            let trading_owns_dropdown = self.trading_header_tb.open_dropdown_id.is_some();

            // Background click — close the dropdown without selecting any item.
            if rest == "__bg__" && trading_owns_dropdown {
                self.trading_header_tb.open_dropdown_id = None;
                if self.granularity_edit.take().is_some() {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                }
                self.granularity_drag_pending = None;
                self.granularity_drag = None;
                return;
            }
            if rest == "__bg__" {
                self.panel_app.toolbar_state.open_dropdown_id = None;
                self.panel_app.toolbar_state.open_dropdown_position = None;
                // Dropdown closing abandons any in-progress inline
                // granularity edit (user-granularity-templates-inner arc,
                // кусок 2) — no persisted partial state, no lingering focus
                // on a field the user can no longer see.
                if self.granularity_edit.take().is_some() {
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                }
                // Dropdown closing also cancels any pending/active row
                // drag-reorder (owner spec 2026-07-16 items 2+3).
                self.granularity_drag_pending = None;
                self.granularity_drag = None;
                return;
            }

            // Format: "dropdown:{dropdown_id}:{item_id}"
            let mut parts = rest.splitn(2, ':');
            if let (Some(dropdown_id), Some(item_id)) = (parts.next(), parts.next()) {
                // Noop items (disabled items, headers) use the "__noop__:" prefix.
                // The click is already consumed by reaching here (it didn't hit __bg__),
                // so we just return — the dropdown stays open with no action fired.
                if item_id.starts_with("__noop__:") {
                    return;
                }

                // Trading-header-owned dropdown: TF selection targets the
                // CONTAINER, never the active chart. Favorites star rows fall
                // through to the shared handler (they mutate the global
                // favorites store — identical semantics for both instances);
                // every other chart-coupled row is swallowed.
                if trading_owns_dropdown {
                    if dropdown_id == "timeframe_selector" {
                        if let Some(tf_name) = item_id.strip_prefix("tf_") {
                            let tc_pid = self.panels_store.trading_container.keys().next().copied();
                            if let Some(pid) = tc_pid {
                                let tf_name = tf_name.to_string();
                                self.set_trading_container_timeframe(pid, &tf_name);
                            }
                            self.trading_header_tb.open_dropdown_id = None;
                            return;
                        }
                        if !(item_id.starts_with("gran_star:") || item_id == "gran_pin:toggle") {
                            self.trading_header_tb.open_dropdown_id = None;
                            return;
                        }
                        // gran_star:* / gran_pin:toggle fall through to the
                        // shared favorites/pin handlers (global stores,
                        // identical semantics for both toolbar instances).
                    } else if dropdown_id == "tc_center" {
                        if let Some(kind_id) = item_id.strip_prefix("center:") {
                            if let Some(kind) = mlc_app_panels::trading::trading::trading_container::CenterKind::from_wire_id(kind_id) {
                                let tc_pid = self.panels_store.trading_container.keys().next().copied();
                                if let Some(pid) = tc_pid {
                                    if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                                        tc.center_kind = kind;
                                    }
                                    self.autosave_snapshot();
                                }
                            }
                        }
                        self.trading_header_tb.open_dropdown_id = None;
                        return;
                    } else if dropdown_id == "agent_backend" {
                        // Agents backend pick — spawn replaces the pane.
                        self.trading_header_tb.open_dropdown_id = None;
                        if let Some(cli) = item_id.strip_prefix("spawn:") {
                            let wid = format!("agent:spawn:{}", cli);
                            self.dispatch_panel_click(&wid, x, y);
                        }
                        return;
                    } else if dropdown_id == "tc_spawn" {
                        // Free-mode "+ Panel" spawn (owner spec 2026-07-29):
                        // one leaf per kind — an existing kind is activated,
                        // a new one splits the largest leaf.
                        if let Some(wire) = item_id.strip_prefix("spawn:") {
                            if let Some(kind) = mlc_app_panels::trading::trading::trading_container::TcLeaf::from_wire_id(wire) {
                                let tc_pid = self.panels_store.trading_container.keys().next().copied();
                                if let Some(pid) = tc_pid {
                                    if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                                        tc.spawn_free_leaf(kind);
                                    }
                                    self.sidebar_data_dirty = true;
                                    self.autosave_snapshot();
                                }
                            }
                        }
                        self.trading_header_tb.open_dropdown_id = None;
                        return;
                    } else if dropdown_id == "chart_type_selector" {
                        // Chart-mode control: run the FULL chart-type switch
                        // pipeline against the embedded center chart (the
                        // exact machinery `ChangeChartType` runs for the
                        // active grid chart — extracted as
                        // `apply_chart_type_switch`, 2026-07-26).
                        let aux_cid = self.panels_store.trading_container.keys().next().copied()
                            .and_then(|pid| self.trading_center_charts.get(&pid).copied());
                        if let Some(cid) = aux_cid {
                            self.apply_chart_type_switch(Some(cid), item_id);
                            self.sidebar_data_dirty = true;
                        }
                        self.trading_header_tb.open_dropdown_id = None;
                        return;
                    } else if dropdown_id == "symbol_selector" {
                        // Quicksearch rows fall through to the shared
                        // symbol_selector arm below — `handle_symbol_quick_click`
                        // is target-aware and commits to the container.
                    } else {
                        self.trading_header_tb.open_dropdown_id = None;
                        return;
                    }
                }

                // Delete actions — routed here (instead of the generic
                // `handle_dropdown_select_with_chart` fallback below) so both
                // items record undo state via `Command::DeletePrimitive` /
                // `Command::DeleteAllPrimitives` instead of the toolbar-state
                // `delete_selected`/`delete_all` calls, which mutate the
                // drawing manager directly with no undo history.
                if dropdown_id == "delete_tools" {
                    let active_bid_del = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    if let Some(bid) = active_bid_del {
                        match item_id {
                            "delete_selected" => {
                                let selected = self.core.drawings().selected(bid);
                                if let Some(prim_id) = selected {
                                    self.delete_primitive_with_undo(bid, prim_id);
                                }
                            }
                            "delete_all" => {
                                self.delete_all_primitives_with_undo(bid);
                            }
                            _ => {}
                        }
                    }
                    return;
                }

                // symbol_selector inline fast-search dropdown rows
                // (symbol-search-compare-inner arc, Wave 2, кусок 4 — owner
                // spec 2026-07-16). Checked BEFORE the granularity_edit
                // guard below (that guard only applies to the
                // `timeframe_selector` dropdown's OWN inline editor —
                // `symbol_quick_edit` is a separate, independent state) and
                // before the generic `handle_dropdown_select_with_chart`
                // fallback, mirroring exactly how granularity's own
                // trailing-icon prefixes are checked first.
                if dropdown_id == "symbol_selector" {
                    self.handle_symbol_quick_click(item_id);
                    return;
                }

                // Inline granularity editor is ACTIVE (owner rule 2026-07-16):
                // while the text input lives, item clicks inside the dropdown
                // must not select/apply anything — the user commits with
                // Enter or abandons via Escape / background-click (the
                // `__bg__` arm above cancels the edit). Consume the click,
                // keep the dropdown open, keep editing.
                if self.granularity_edit.is_some() {
                    return;
                }

                // Inline-input "__input__:{real_id}" hit zones were retired —
                // dropdown rows for range_bar / volume_bar / footprint now fire
                // the regular ChangeChartType action with the panel's current
                // param/interval (or default). Param editing happens via the
                // toolbar-level field after the chart type is committed.

                // FAVORITES-section row click (user-granularity-templates-
                // inner arc, кусок 2 — defect fix 2026-07-16). These rows
                // carry the `GRANULARITY_FAV_ID_PREFIX` uniqueness wrapper
                // (distinct dropdown-item id so hover doesn't leak into the
                // row's normal-position twin — see that const's doc
                // comment). Strip it here, BEFORE every other prefix check
                // below, so a FAVORITES row's body click dispatches through
                // the EXACT same path as its normal-position twin
                // (`handle_dropdown_select_with_chart` at the bottom of this
                // block sees the real `tf_{name}` / `reso_param:{value}` id).
                let item_id: &str = item_id.strip_prefix(mlc_core::chart_type::GRANULARITY_FAV_ID_PREFIX)
                    .unwrap_or(item_id);

                // Granularity trailing-icon clicks (star/edit/delete —
                // user-granularity-templates-inner arc, кусок 2). Checked
                // BEFORE the "+ Add" row and the normal dispatch path below
                // (mirrors кусок 1's custom-row-first ordering). These do
                // NOT close the dropdown — the user should see the list
                // update in place (star fill flip, row removed on delete).
                if let Some(target_id) = item_id.strip_prefix(mlc_core::chart_type::GRANULARITY_STAR_ACTION_PREFIX) {
                    if let Some(target) = self.resolve_granularity_target(target_id) {
                        self.process_chart_out_event(
                            mlc_core::events::ChartOutEvent::ToggleGranularityStar { target }
                        );
                    }
                    return;
                }
                if let Some(target_id) = item_id.strip_prefix(mlc_core::chart_type::GRANULARITY_DELETE_ACTION_PREFIX) {
                    if let Some(target) = self.resolve_granularity_target(target_id) {
                        self.process_chart_out_event(
                            mlc_core::events::ChartOutEvent::DeleteGranularityCustom { target }
                        );
                    }
                    return;
                }
                if let Some(target_id) = item_id.strip_prefix(mlc_core::chart_type::GRANULARITY_EDIT_ACTION_PREFIX) {
                    if let Some(target) = self.resolve_granularity_target(target_id) {
                        self.begin_granularity_edit(target);
                    }
                    return;
                }

                // Pinned-favorites-row toggle on the "FAVORITES" header
                // (кусок 3). Same "stay open, update in place" contract as
                // the star/edit/delete icons above.
                if item_id == mlc_core::chart_type::GRANULARITY_PIN_TOGGLE_ACTION_ID {
                    self.process_chart_out_event(mlc_core::events::ChartOutEvent::ToggleGranularityPin);
                    return;
                }

                // Granularity "+ Add" rows (кусок 2 — replaces кусок 1's
                // invisible "Custom…" input row): the WHOLE row's only
                // purpose is starting inline creation (no separate row-click
                // action to conflict with — this is why the whole-row hit
                // zone is safe here, unlike the 2026-06 chart-type-row
                // inline-input revert, where the input zone overlapped a
                // row that ALSO had its own click action). Sets
                // `granularity_edit` and focuses the dedicated field with an
                // empty prefill instead of dispatching through
                // `handle_dropdown_select_with_chart`.
                if item_id == mlc_core::chart_type::TF_FAVORITE_CUSTOM_INPUT_ITEM_ID {
                    self.granularity_edit = Some(crate::GranularityEdit { kind: crate::GranularityEditKind::AddTf });
                    let field_id = uzor::WidgetId::from(crate::text_input::TF_FAVORITE_INPUT);
                    let mut coord = self.input_coordinator.borrow_mut();
                    let tf = coord.text_fields_mut();
                    tf.set_text(&field_id, "");
                    tf.begin_edit(&field_id);
                    tf.focus(crate::text_input::TF_FAVORITE_INPUT);
                    return;
                }
                if item_id == mlc_core::chart_type::PARAM_FAVORITE_CUSTOM_INPUT_ITEM_ID {
                    self.granularity_edit = Some(crate::GranularityEdit { kind: crate::GranularityEditKind::AddParam });
                    let field_id = uzor::WidgetId::from(crate::text_input::PARAM_FAVORITE_INPUT);
                    let mut coord = self.input_coordinator.borrow_mut();
                    let tf = coord.text_fields_mut();
                    tf.set_text(&field_id, "");
                    tf.begin_edit(&field_id);
                    tf.focus(crate::text_input::PARAM_FAVORITE_INPUT);
                    return;
                }
                if item_id == mlc_core::chart_type::INTERVAL_FAVORITE_CUSTOM_INPUT_ITEM_ID {
                    self.granularity_edit = Some(crate::GranularityEdit { kind: crate::GranularityEditKind::AddInterval });
                    let field_id = uzor::WidgetId::from(crate::text_input::TF_FAVORITE_INPUT);
                    let mut coord = self.input_coordinator.borrow_mut();
                    let tf = coord.text_fields_mut();
                    tf.set_text(&field_id, "");
                    tf.begin_edit(&field_id);
                    tf.focus(crate::text_input::TF_FAVORITE_INPUT);
                    return;
                }

                let dropdown_id = dropdown_id.to_string();
                let item_id = item_id.to_string();

                let active_bid_dd = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                let window_ptr: *mut _ = self.panel_app.panel_grid.active_container_mut()
                    .map(|w| w as *mut _)
                    .unwrap_or(std::ptr::null_mut::<crate::chart_container::ChartContainer>());

                if !window_ptr.is_null() {
                    // SAFETY: toolbar_state does not access panel_grid.
                    let window = unsafe { &mut *window_ptr };
                    if let Some(bid) = active_bid_dd {
                        let mut guard = self.core.drawings_mut();
                        let mut session = crate::input::drawing_session::DrawingSessionView::new(&mut guard, bid);
                        let autosave = self.panel_app.autosave_enabled;
                        let events = self.panel_app.toolbar_state.handle_dropdown_select_with_chart(
                            &dropdown_id,
                            &item_id,
                            &mut window.crosshair,
                            &mut session,
                            autosave,
                        );
                        drop(guard);
                        for event in events {
                            self.process_chart_out_event(event);
                        }
                    }
                }
            }
            return;
        }

        // === Modal widget clicks — route to dedicated handlers ===
        if let Some(rest) = widget_id.strip_prefix("chart_settings:") {
            self.handle_chart_settings_click(rest, x, y);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("prim_settings:") {
            self.handle_prim_settings_click(rest, x, y);
            return;
        }
        // ── Template name overlay modal for primitive settings ────────────────
        if let Some(rest) = widget_id.strip_prefix("prim_tmpl:") {
            self.handle_prim_tmpl_modal_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("ind_settings:") {
            self.handle_ind_settings_click(rest, x, y);
            return;
        }
        // ── Template name overlay modal for indicator settings ────────────────
        if let Some(rest) = widget_id.strip_prefix("ind_tmpl:") {
            self.handle_ind_tmpl_modal_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("alert_set:") {
            self.handle_alert_settings_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("ind_overlay:") {
            self.handle_ind_overlay_click(rest, x, y);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("cmp_overlay:") {
            self.handle_cmp_overlay_click(rest);
            return;
        }
        // ── Template name overlay modal for compare settings ──────────────────
        if let Some(rest) = widget_id.strip_prefix("cmp_tmpl:") {
            self.handle_cmp_tmpl_modal_click(rest);
            return;
        }
        // ── Template name overlay modal for chart settings ────────────────────
        if let Some(rest) = widget_id.strip_prefix("chart_tmpl:") {
            self.handle_chart_tmpl_modal_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("cmp_settings:") {
            self.handle_cmp_settings_click(rest, x, y);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("overlay_settings:") {
            match rest {
                "close" => {
                    self.panel_app.close_overlay_settings();
                    eprintln!("[ChartApp] overlay_settings closed");
                }
                "modal_bg" => {
                    // No-op — absorb click to prevent fall-through.
                }
                tab_id if tab_id.starts_with("tab:") => {
                    use mlc_app_layout::ui::modal_settings::OverlayPanelTreeTab;
                    let tab = match &tab_id["tab:".len()..] {
                        "tree_view"  => OverlayPanelTreeTab::TreeView,
                        "eliminate"  => OverlayPanelTreeTab::Eliminate,
                        "hidden"     => OverlayPanelTreeTab::Hidden,
                        "minimap"    => OverlayPanelTreeTab::Minimap,
                        _            => return,
                    };
                    self.panel_app.overlay_settings_state.set_tab(tab);
                }
                id if id.starts_with("select:") => {
                    if let Ok(node_id) = id["select:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(node_id);
                        let leaf_id = mlc_core::LeafId(node_id);
                        if self.panel_app.panel_grid.docking().tree().leaf(leaf_id).is_some() {
                            self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        }
                    }
                }
                id if id.starts_with("minimap_leaf:") => {
                    if let Ok(leaf_id_val) = id["minimap_leaf:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(leaf_id_val);
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                id if id.starts_with("eliminate:") => {
                    if let Ok(leaf_id_val) = id["eliminate:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        let removed = self.panel_app.panel_grid.close_leaf(leaf_id);
                        if removed {
                            // If eliminated leaf was the target, clear it
                            if self.panel_app.overlay_settings_state.target_leaf_id == Some(leaf_id) {
                                self.panel_app.overlay_settings_state.target_leaf_id = None;
                                self.panel_app.overlay_settings_state.selected_node_id = None;
                            }
                            eprintln!("[OverlaySettings] Eliminated leaf {}", leaf_id_val);
                            // Persist the removal — every other mutating branch
                            // autosaves; this one did not, so eliminated panels
                            // came back on reload.
                            self.autosave_snapshot();
                        }
                    }
                }
                id if id.starts_with("hide:") => {
                    if let Ok(leaf_id_val) = id["hide:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        let hidden = self.panel_app.panel_grid.docking_mut().tree_mut().hide_leaf(leaf_id);
                        if hidden {
                            eprintln!("[OverlaySettings] Hidden leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("show:") || id.starts_with("restore:") => {
                    let prefix = if id.starts_with("show:") { "show:" } else { "restore:" };
                    if let Ok(leaf_id_val) = id[prefix.len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.docking_mut().tree_mut().show_leaf(leaf_id);
                        eprintln!("[OverlaySettings] Shown leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("expand:") => {
                    if let Ok(leaf_id_val) = id["expand:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        // Set as active leaf before toggling expand
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.panel_app.panel_grid.toggle_expand();
                        eprintln!("[OverlaySettings] Toggled expand for leaf {}", leaf_id_val);
                    }
                }
                _ => {
                    eprintln!("[ChartApp] unhandled overlay_settings click: {}", rest);
                }
            }
            return;
        }

        // === Risk Calculator modal chrome (overlay hosting, 2026-07-29) ===
        if widget_id == "rc_modal:close" {
            self.rc_modal.close();
            return;
        }
        if widget_id == "rc_modal:modal_bg" || widget_id == "rc_modal:header" {
            return; // consume — clicks on the modal chrome stay there
        }

        // === DOM settings modal (Stage B, 2026-07-30) ===
        if let Some(rest) = widget_id.strip_prefix("dom_modal:") {
            if rest == "close" {
                self.dom_modal.close();
                return;
            }
            if rest == "modal_bg" || rest == "header" {
                return; // consume — chrome
            }
            use mlc_app_sidebar_content::state::RightSidebarPanel as RSP;
            let rp = self.sidebar_state.right_panel;
            // Agents modal rows (Stage B agents reform).
            if rp == RSP::Agents {
                if rest == "agent_mode:pty" {
                    self.sidebar_state.agent_spawn_mode = gate4agent::InstanceMode::Pty;
                    self.sidebar_data_dirty = true;
                } else if rest == "agent_mode:chat" {
                    self.sidebar_state.agent_spawn_mode = gate4agent::InstanceMode::Chat;
                    self.sidebar_data_dirty = true;
                } else if let Some(cli) = rest.strip_prefix("agent_spawn:") {
                    self.dom_modal.close();
                    let wid = format!("agent:spawn:{}", cli);
                    self.dispatch_panel_click(&wid, x, y);
                }
                return;
            }
            let section = if matches!(rp, RSP::SlotDom | RSP::SlotTape | RSP::SlotL2 | RSP::SlotBigTrades) {
                self.section_panel_for(rp)
            } else {
                None
            };
            // Non-DOM sections: mode switch + columns + Big-Trades thresholds.
            match &section {
                Some(mlc_core::state::Panel::TradeTape(pid)) => {
                    if rest == "mode:single" || rest == "mode:multi" {
                        let want = rest == "mode:multi";
                        let subs = self.panels_store.trade_tape.get_mut(pid).map(|s| {
                            s.set_multi_mode(want);
                            (s.multi_mode, s.multi.symbol.clone(), s.multi.entries.clone())
                        });
                        if let Some((on, sym, entries)) = subs {
                            if on {
                                self.subscribe_multi_tape_roster(&sym, &entries, false);
                            }
                            self.autosave_snapshot();
                        }
                        self.sidebar_data_dirty = true;
                        return;
                    }
                    let mut persist = false;
                    if let Some(state) = self.panels_store.trade_tape.get_mut(pid) {
                        if let Some(i) = rest.strip_prefix("col:").and_then(|s| s.parse::<usize>().ok()) {
                            match i {
                                0 => state.column_config.show_time = !state.column_config.show_time,
                                1 => state.column_config.show_price = !state.column_config.show_price,
                                2 => state.column_config.show_size = !state.column_config.show_size,
                                _ => state.column_config.show_notional = !state.column_config.show_notional,
                            }
                        } else if rest == "skin" {
                            // Manual pick pins the panel skin (theme-skin
                            // default until the first cycle).
                            use mlc_app_panels::trading::order_flow::trade_tape::TapeSkin;
                            let cur = state.skin.unwrap_or(TapeSkin::Classic);
                            state.skin = Some(cur.next());
                        } else if rest == "tt_flag" {
                            // Big-print flag threshold cycle (ref 17).
                            const FLAGS: &[f64] = &[10_000.0, 50_000.0, 100_000.0, 500_000.0, 1_000_000.0];
                            state.flag_notional = match state.flag_notional {
                                None => Some(FLAGS[0]),
                                Some(cur) => FLAGS.iter().find(|&&v| v > cur * 1.001).copied(),
                            };
                        } else if rest == "imb" {
                            state.show_imbalance = !state.show_imbalance;
                            persist = true;
                        } else if rest == "imb_win" {
                            const WINDOWS: &[u8] = &[1, 2, 5, 10];
                            let cur = state.imb_window_min;
                            state.imb_window_min = WINDOWS
                                .iter()
                                .find(|&&m| m > cur)
                                .copied()
                                .unwrap_or(WINDOWS[0]);
                            persist = true;
                        }
                        self.sidebar_data_dirty = true;
                    }
                    if persist {
                        self.autosave_snapshot();
                    }
                    return;
                }
                Some(mlc_core::state::Panel::L2Tape(pid)) => {
                    if rest == "mode:single" || rest == "mode:multi" {
                        let want = rest == "mode:multi";
                        let subs = self.panels_store.l2_tape.get_mut(pid).map(|s| {
                            s.set_multi_mode(want);
                            (s.multi_mode, s.multi.symbol.clone(), s.multi.entries.clone())
                        });
                        if let Some((on, sym, entries)) = subs {
                            if on {
                                self.subscribe_multi_tape_roster(&sym, &entries, true);
                            }
                            self.autosave_snapshot();
                        }
                        self.sidebar_data_dirty = true;
                        return;
                    }
                    let mut persist = false;
                    if let Some(state) = self.panels_store.l2_tape.get_mut(pid) {
                        if let Some(i) = rest.strip_prefix("col:").and_then(|s| s.parse::<usize>().ok()) {
                            match i {
                                0 => state.column_config.show_time = !state.column_config.show_time,
                                1 => state.column_config.show_type = !state.column_config.show_type,
                                2 => state.column_config.show_side = !state.column_config.show_side,
                                3 => state.column_config.show_price = !state.column_config.show_price,
                                _ => state.column_config.show_qty = !state.column_config.show_qty,
                            }
                        } else if rest == "imb" {
                            state.show_imbalance = !state.show_imbalance;
                            persist = true;
                        } else if rest == "imb_win" {
                            const WINDOWS: &[u8] = &[1, 2, 5, 10];
                            let cur = state.imb_window_min;
                            state.imb_window_min = WINDOWS
                                .iter()
                                .find(|&&m| m > cur)
                                .copied()
                                .unwrap_or(WINDOWS[0]);
                            persist = true;
                        }
                        self.sidebar_data_dirty = true;
                    }
                    if persist {
                        self.autosave_snapshot();
                    }
                    return;
                }
                Some(mlc_core::state::Panel::BigTrades(pid)) => {
                    if rest == "mode:single" || rest == "mode:multi" {
                        let want = rest == "mode:multi";
                        let subs = self.panels_store.big_trades.get_mut(pid).map(|s| {
                            s.set_multi_mode(want);
                            (s.multi_mode, s.multi.symbol.clone(), s.multi.entries.clone())
                        });
                        if let Some((on, sym, entries)) = subs {
                            if on {
                                self.subscribe_multi_tape_roster(&sym, &entries, false);
                            }
                            self.autosave_snapshot();
                        }
                        self.sidebar_data_dirty = true;
                        return;
                    }
                    if let Some(state) = self.panels_store.big_trades.get_mut(pid) {
                        if let Some(i) = rest.strip_prefix("col:").and_then(|s| s.parse::<usize>().ok()) {
                            match i {
                                0 => state.column_config.show_time = !state.column_config.show_time,
                                1 => state.column_config.show_side = !state.column_config.show_side,
                                2 => state.column_config.show_price = !state.column_config.show_price,
                                3 => state.column_config.show_size = !state.column_config.show_size,
                                _ => state.column_config.show_notional = !state.column_config.show_notional,
                            }
                        } else if rest == "bt_size" {
                            const SIZES: &[f64] = &[0.0, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0];
                            let cur = state.size_threshold;
                            state.size_threshold = SIZES.iter()
                                .find(|&&s| s > cur + f64::EPSILON)
                                .copied()
                                .unwrap_or(0.0);
                        } else if rest == "bt_notional" {
                            const NOTIONALS: &[f64] = &[10_000.0, 50_000.0, 100_000.0, 500_000.0, 1_000_000.0];
                            state.notional_threshold = match state.notional_threshold {
                                None => Some(NOTIONALS[0]),
                                Some(cur) => NOTIONALS.iter().find(|&&n| n > cur * 1.001).copied(),
                            };
                        } else if rest == "skin" {
                            use mlc_app_panels::trading::order_flow::trade_tape::TapeSkin;
                            let cur = state.skin.unwrap_or(TapeSkin::Classic);
                            state.skin = Some(cur.next());
                        }
                        self.sidebar_data_dirty = true;
                    }
                    return;
                }
                _ => {}
            }
            let dom_pid = section.and_then(|item| match item {
                mlc_core::state::Panel::Dom(id) => Some(id),
                _ => None,
            });
            let Some(pid) = dom_pid else { return };
            match rest {
                "mode:dom" | "mode:multi" => {
                    let want_multi = rest == "mode:multi";
                    let sub_info = self.panels_store.dom.get_mut(&pid).map(|state| {
                        if state.multi_mode != want_multi {
                            state.set_multi_mode(want_multi);
                        }
                        (state.multi_mode, state.multi.symbol.clone(), state.multi.config.exchanges.clone())
                    });
                    if let Some((on, sym, exchanges)) = sub_info {
                        if on {
                            self.subscribe_multi_roster(&sym, &exchanges);
                        }
                        self.autosave_snapshot();
                    }
                }
                "skin" => {
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        use mlc_app_panels::trading::order_flow::dom::DomSkin;
                        let skins = DomSkin::all();
                        let cur = skins.iter().position(|s| *s == state.skin).unwrap_or(0);
                        let next = skins[(cur + 1) % skins.len()];
                        state.set_skin(next);
                    }
                }
                // Skin texture layer manual switches — edit the live
                // theme's panel_style (skin presets re-apply their own).
                "tex:gradient" | "tex:flat" | "tex:cells" | "tex:mosaic" => {
                    use mlc_core::theme::BarTexture;
                    let tex = match rest {
                        "tex:flat" => BarTexture::Flat,
                        "tex:cells" => BarTexture::Cells,
                        "tex:mosaic" => BarTexture::Mosaic,
                        _ => BarTexture::Gradient,
                    };
                    self.panel_app.theme_manager.current_mut().panel_style.bar_texture = tex;
                }
                "zebra" => {
                    let ps = &mut self.panel_app.theme_manager.current_mut().panel_style;
                    ps.row_zebra = !ps.row_zebra;
                }
                // Intensity normalization — per-panel Multi-DOM config
                // (TapeSurf Individual vs Cross Exchange), persisted.
                "norm:cross" | "norm:col" => {
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        state.multi.config.per_column_norm = rest == "norm:col";
                    }
                    self.autosave_snapshot();
                }
                // Solo-ladder depth-wave / big-marker toggles (Multi-DOM
                // parity), persisted.
                "wave" => {
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        state.show_depth_wave = !state.show_depth_wave;
                    }
                    self.autosave_snapshot();
                }
                "big" => {
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        state.show_big_markers = !state.show_big_markers;
                    }
                    self.autosave_snapshot();
                }
                "am" => {
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        state.auto_center = !state.auto_center;
                        if state.auto_center && state.market_price > 0.0 {
                            state.center_price = state.market_price;
                        }
                    }
                }
                "filter" => {
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        state.min_volume_filter = match state.min_volume_filter {
                            v if v < 0.5 => 1.0,
                            v if v < 1.5 => 5.0,
                            v if v < 5.5 => 10.0,
                            v if v < 10.5 => 25.0,
                            v if v < 25.5 => 50.0,
                            _ => 0.0,
                        };
                    }
                }
                "tick" => {
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        const TICKS: &[f64] = &[0.001, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 50.0, 100.0];
                        let cur = state.tick_size;
                        let next = TICKS.iter().find(|&&t| t > cur * 1.001).copied().unwrap_or(TICKS[0]);
                        state.tick_size = next;
                        state.volume_by_price.clear();
                    }
                }
                // Roster management (ex_add / ex_del) and grid / depth / imb
                // buttons left the modal (owner spec 2026-07-30): columns
                // live in the header picker dropdown, grid on the toolbar
                // tick chip (+ ctrl+scroll), imbalance on the `sec_imb`
                // toolbar chip.
                _ => {
                    if let Some(i_str) = rest.strip_prefix("col:") {
                        if let (Ok(i), Some(state)) = (i_str.parse::<usize>(), self.panels_store.dom.get_mut(&pid)) {
                            match i {
                                0 => state.column_config.show_bid_orders = !state.column_config.show_bid_orders,
                                1 => state.column_config.show_sell_trades = !state.column_config.show_sell_trades,
                                2 => state.column_config.show_buy_trades = !state.column_config.show_buy_trades,
                                _ => state.column_config.show_ask_orders = !state.column_config.show_ask_orders,
                            }
                        }
                    }
                }
            }
            self.sidebar_data_dirty = true;
            return;
        }

        // === Trading Container modal clicks (owner spec 2026-07-28) ===
        if let Some(rest) = widget_id.strip_prefix("tc_modal:") {
            use mlc_app_layout::layout::modals::TcModalSection;
            use mlc_app_panels::trading::trading::trading_container::{BottomTab, CenterKind};
            let tc_pid = self.panels_store.trading_container.keys().next().copied();
            match rest {
                "close" => {
                    self.tc_modal.close();
                }
                "modal_bg" | "header" => {
                    // Absorb click — prevent fall-through.
                }
                "sidebar:map" => {
                    self.tc_modal.section = TcModalSection::Map;
                }
                "sidebar:setup" => {
                    self.tc_modal.section = TcModalSection::Setup;
                }
                // SETUP interior layout mode (owner spec 2026-07-29):
                // Fixed v1 regions ↔ Free uzor-docking. First switch to
                // Free migrates the live v1 geometry into a docking tree.
                "layout:fixed" | "layout:free" => {
                    use mlc_app_panels::trading::trading::trading_container::TcLayoutMode;
                    let mode = if rest == "layout:free" { TcLayoutMode::Free } else { TcLayoutMode::Fixed };
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            if tc.layout_mode != mode {
                                tc.layout_mode = mode;
                                if mode == TcLayoutMode::Free && tc.free_dock.is_none() {
                                    tc.free_dock = Some(tc.build_free_dock());
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                }
                // SETUP focus-highlight style (owner spec 2026-07-29):
                // accent frame ↔ main-chart-style 2px corner bar.
                "focus_style:frame" | "focus_style:bar" => {
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            tc.focus_style_bar = rest == "focus_style:bar";
                        }
                        self.autosave_snapshot();
                    }
                }
                // SETUP risk-calculator hosting (owner spec 2026-07-29):
                // its own panel ↔ overlay popover behind the OE gauge chip.
                "rc_host:panel" | "rc_host:overlay" => {
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            tc.set_rc_overlay(rest == "rc_host:overlay");
                        }
                        self.autosave_snapshot();
                    }
                }
                // SETUP center switcher — same action as the header's
                // `tc_center` dropdown rows.
                "center:dom" | "center:chart" => {
                    let kind = if rest == "center:dom" { CenterKind::Dom } else { CenterKind::Chart };
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            tc.center_kind = kind;
                        }
                        self.autosave_snapshot();
                    }
                }
                "reset_layout" => {
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            tc.reset_layout();
                        }
                        self.autosave_snapshot();
                    }
                }
                // MAP center card: body click OR the dedicated fast-swap
                // button both flip DOM↔Chart (owner spec 2026-07-28).
                "map:center" | "map:center:swap" => {
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            tc.center_kind = match tc.center_kind {
                                CenterKind::Dom => CenterKind::Chart,
                                CenterKind::Chart => CenterKind::Dom,
                            };
                        }
                        self.autosave_snapshot();
                    }
                }
                // Free-mode MAP (owner spec 2026-07-29): the map mirrors the
                // LIVE docking tree — `map:leaf{raw}:tab:{wire}` activates
                // that tab in that stack.
                r if r.starts_with("map:leaf") => {
                    let tail = &r["map:leaf".len()..];
                    if let Some((raw_s, wire)) = tail.split_once(":tab:") {
                        if let (Ok(raw), Some(kind)) = (
                            raw_s.parse::<u64>(),
                            mlc_app_panels::trading::trading::trading_container::TcLeaf::from_wire_id(wire),
                        ) {
                            if let Some(pid) = tc_pid {
                                if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                                    if let Some(dock) = &tc.free_dock {
                                        let mut ds = dock.0.borrow_mut();
                                        if let Some(l) = ds.tree_mut().leaf_mut(uzor::panels::LeafId(raw)) {
                                            if let Some(i) = l.panels.iter().position(|p| p == &kind) {
                                                l.active_tab = i;
                                            }
                                        }
                                    }
                                }
                                self.sidebar_data_dirty = true;
                            }
                        }
                    }
                }
                // MAP bottom strip — ONE card with 3 tab buttons switching
                // its active view.
                "map:bottom:tab:positions" | "map:bottom:tab:orders" | "map:bottom:tab:trades" => {
                    let tab = match rest {
                        "map:bottom:tab:positions" => BottomTab::Positions,
                        "map:bottom:tab:orders" => BottomTab::Orders,
                        _ => BottomTab::Trades,
                    };
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            tc.active_tab = tab;
                        }
                    }
                }
                // Ticker-tape entry format (owner spec 2026-07-28).
                "tapefmt:full" | "tapefmt:short" => {
                    use mlc_app_panels::trading::trading::ticker_tape::TapeFormat;
                    let fmt = if rest == "tapefmt:full" { TapeFormat::Full } else { TapeFormat::Short };
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            tc.ticker_tape.format = fmt;
                        }
                    }
                }
                // Metrics-strip inclusion toggles (owner spec 2026-07-28).
                r if r.starts_with("metric:") => {
                    let key = &r["metric:".len()..];
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            let cfg = &mut tc.metrics.config;
                            match key {
                                "upnl" => cfg.upnl = !cfg.upnl,
                                "rpnl" => cfg.rpnl = !cfg.rpnl,
                                "balance" => cfg.balance = !cfg.balance,
                                "positions" => cfg.positions = !cfg.positions,
                                "orders" => cfg.orders = !cfg.orders,
                                "trades" => cfg.trades = !cfg.trades,
                                _ => {}
                            }
                        }
                    }
                }
                // History-depth option buttons — `hist:{table}:{idx}`, idx
                // into `HistoryDepth::OPTIONS` (owner spec 2026-07-28).
                r if r.starts_with("hist:") => {
                    use mlc_app_panels::trading::trading::history::HistoryDepth;
                    let mut parts = r["hist:".len()..].splitn(2, ':');
                    let table = parts.next().unwrap_or_default();
                    let idx: usize = parts.next().and_then(|s| s.parse().ok()).unwrap_or(2);
                    let depth = HistoryDepth::OPTIONS.get(idx).copied().unwrap_or_default();
                    if let Some(pid) = tc_pid {
                        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                            match table {
                                "positions" => tc.position_manager.history_depth = depth,
                                "orders" => tc.orders_log.history_depth = depth,
                                "trades" => tc.trade_log.history_depth = depth,
                                _ => {}
                            }
                        }
                    }
                }
                _ => {
                    eprintln!("[ChartApp] unhandled tc_modal click: {}", rest);
                }
            }
            return;
        }

        // === Tags & Tabs modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("tags_tabs:") {
            use mlc_app_layout::ui::modal_settings::{TagsTabsSidebar, TagsTabsTagsTab};
            use mlc_core::ids::BubbleId;
            match rest {
                "close" => {
                    self.panel_app.close_tags_tabs();
                    eprintln!("[ChartApp] tags_tabs closed");
                }
                "modal_bg" => {
                    // Absorb click — prevent fall-through.
                }
                "sidebar:tabs" => {
                    self.panel_app.tags_tabs_state.set_sidebar(TagsTabsSidebar::Tabs);
                }
                "sidebar:tags" => {
                    self.panel_app.tags_tabs_state.set_sidebar(TagsTabsSidebar::Tags);
                }
                "sidebar:map" => {
                    self.panel_app.tags_tabs_state.set_sidebar(TagsTabsSidebar::Map);
                }
                id if id.starts_with("tab:") => {
                    use mlc_app_layout::ui::modal_settings::OverlayPanelTreeTab;
                    let tab = match &id["tab:".len()..] {
                        "tree_view"  => OverlayPanelTreeTab::TreeView,
                        "eliminate"  => OverlayPanelTreeTab::Eliminate,
                        "hidden"     => OverlayPanelTreeTab::Hidden,
                        "minimap"    => OverlayPanelTreeTab::Minimap,
                        _            => return,
                    };
                    self.panel_app.overlay_settings_state.set_tab(tab);
                }
                id if id.starts_with("tags_tab:") => {
                    let tab = match &id["tags_tab:".len()..] {
                        "groups"  => TagsTabsTagsTab::Groups,
                        "details" => TagsTabsTagsTab::Details,
                        _         => return,
                    };
                    self.panel_app.tags_tabs_state.set_tags_tab(tab);
                }
                id if id.starts_with("tags:select_group:") => {
                    if let Ok(gid_val) = id["tags:select_group:".len()..].parse::<u64>() {
                        self.panel_app.tags_tabs_state.selected_group_id = Some(BubbleId(gid_val));
                        self.panel_app.tags_tabs_state.set_tags_tab(TagsTabsTagsTab::Details);
                        eprintln!("[TagsTabs] Selected group {}", gid_val);
                    }
                }
                id if id.starts_with("tags:delete_group:") => {
                    if let Ok(gid_val) = id["tags:delete_group:".len()..].parse::<u64>() {
                        let gid = BubbleId(gid_val);
                        self.remove_tag(gid);
                        // Clear selection if we deleted the selected group.
                        if self.panel_app.tags_tabs_state.selected_group_id == Some(gid) {
                            self.panel_app.tags_tabs_state.selected_group_id = None;
                            self.panel_app.tags_tabs_state.set_tags_tab(TagsTabsTagsTab::Groups);
                        }
                        eprintln!("[TagsTabs] Deleted group {}", gid_val);
                    }
                }
                id if id.starts_with("tags:toggle_flag:") => {
                    // Format: tags:toggle_flag:{group_id}:{flag_id}
                    let suffix = &id["tags:toggle_flag:".len()..];
                    if let Some(colon_pos) = suffix.find(':') {
                        let gid_str  = &suffix[..colon_pos];
                        let flag_str = &suffix[colon_pos + 1..];
                        if let Ok(gid_val) = gid_str.parse::<u64>() {
                            let gid = BubbleId(gid_val);
                            let toggled = self.core.bubbles_mut().with_bubble_mut(gid, |group| {
                                match flag_str {
                                    "sync_crosshair"  => group.sync_flags.sync_crosshair  = !group.sync_flags.sync_crosshair,
                                    "sync_viewport"   => group.sync_flags.sync_viewport   = !group.sync_flags.sync_viewport,
                                    "sync_symbol"     => group.sync_flags.sync_symbol     = !group.sync_flags.sync_symbol,
                                    "sync_timeframe"  => group.sync_flags.sync_timeframe  = !group.sync_flags.sync_timeframe,
                                    "sync_drawings"   => group.sync_flags.sync_drawings   = !group.sync_flags.sync_drawings,
                                    "sync_indicators" => group.sync_flags.sync_indicators = !group.sync_flags.sync_indicators,
                                    _ => {}
                                }
                            }).is_some();
                            if toggled {
                                eprintln!("[TagsTabs] Toggled {} on group {}", flag_str, gid_val);
                            }
                        }
                    }
                }
                // --- Panel tree actions (ported from overlay_settings) ---
                id if id.starts_with("select:") => {
                    if let Ok(node_id) = id["select:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(node_id);
                        let leaf_id = mlc_core::LeafId(node_id);
                        if self.panel_app.panel_grid.docking().tree().leaf(leaf_id).is_some() {
                            self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        }
                    }
                }
                id if id.starts_with("minimap_leaf:") => {
                    if let Ok(leaf_id_val) = id["minimap_leaf:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(leaf_id_val);
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                id if id.starts_with("eliminate:") => {
                    if let Ok(leaf_id_val) = id["eliminate:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        let removed = self.panel_app.panel_grid.close_leaf(leaf_id);
                        if removed {
                            if self.panel_app.overlay_settings_state.target_leaf_id == Some(leaf_id) {
                                self.panel_app.overlay_settings_state.target_leaf_id = None;
                                self.panel_app.overlay_settings_state.selected_node_id = None;
                            }
                            eprintln!("[TagsTabs] Eliminated leaf {}", leaf_id_val);
                            // Persist the removal (same omission as the
                            // overlay-settings eliminate handler).
                            self.autosave_snapshot();
                        }
                    }
                }
                id if id.starts_with("hide:") => {
                    if let Ok(leaf_id_val) = id["hide:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        let hidden = self.panel_app.panel_grid.docking_mut().tree_mut().hide_leaf(leaf_id);
                        if hidden {
                            eprintln!("[TagsTabs] Hidden leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("show:") || id.starts_with("restore:") => {
                    let prefix = if id.starts_with("show:") { "show:" } else { "restore:" };
                    if let Ok(leaf_id_val) = id[prefix.len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.docking_mut().tree_mut().show_leaf(leaf_id);
                        eprintln!("[TagsTabs] Shown leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("expand:") => {
                    if let Ok(leaf_id_val) = id["expand:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.panel_app.panel_grid.toggle_expand();
                        eprintln!("[TagsTabs] Toggled expand for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("untag:") => {
                    if let Ok(leaf_id_val) = id["untag:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.perform_desync(leaf_id);
                        eprintln!("[TagsTabs] Untagged leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("split_h:") => {
                    if let Ok(leaf_id_val) = id["split_h:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.do_split(mlc_core::SplitKind::SplitRight);
                        eprintln!("[TagsTabs] Split horizontal for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("split_v:") => {
                    if let Ok(leaf_id_val) = id["split_v:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.do_split(mlc_core::SplitKind::SplitBottom);
                        eprintln!("[TagsTabs] Split vertical for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("tag:") => {
                    if let Ok(leaf_id_val) = id["tag:".len()..].parse::<u64>() {
                        let leaf_id = mlc_core::LeafId(leaf_id_val);
                        // Open the sync color grid popup anchored near the modal center
                        let anchor_x = self.width as f64 / 2.0;
                        let anchor_y = self.height as f64 / 2.0;
                        self.panel_app.sync_color_grid.open(
                            leaf_id,
                            anchor_x,
                            anchor_y,
                            self.width as f64,
                            self.height as f64,
                        );
                        eprintln!("[TagsTabs] Opening tag color grid for leaf {}", leaf_id_val);
                    }
                }
                _ => {
                    eprintln!("[ChartApp] unhandled tags_tabs click: {}", rest);
                }
            }
            return;
        }

        // === Watchlist group name input modal clicks ===
        // NOTE: checked BEFORE wl_modal: so the higher-layer modal intercepts first.
        if let Some(rest) = widget_id.strip_prefix("wl_group_name:") {
            match rest {
                "save" => {
                    // Read text from TextFieldStore.
                    let wl_gn_id = uzor::WidgetId::from(crate::text_input::WATCHLIST_GROUP_NAME);
                    let name = self.input_coordinator.borrow().text_fields().text(&wl_gn_id).trim().to_string();
                    if !name.is_empty() {
                        use mlc_app_layout::ui::modal_settings::WatchlistGroupNameMode;
                        match self.standalone_panels.wl_group_name_input.mode.clone() {
                            WatchlistGroupNameMode::CreateNew => {
                                let new_id = self.core.watchlists_mut().create_list(name.clone());
                                self.watchlist_ui.active_list_id = new_id;
                                self.watchlist_actions.push(crate::WatchlistAction::CreateList { name: name.clone() });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[WatchlistGroupName] created new list '{}' id={}", name, new_id);
                            }
                            WatchlistGroupNameMode::Rename(id) => {
                                let found = {
                                    let mut wl = self.core.watchlists_mut();
                                    if let Some(list) = wl.find_mut(id) {
                                        list.name = name.clone();
                                        true
                                    } else {
                                        false
                                    }
                                };
                                if found {
                                    self.watchlist_actions.push(crate::WatchlistAction::RenameList { id, new_name: name.clone() });
                                    self.watchlists_dirty = true;
                                    self.persist_watchlists();
                                    eprintln!("[WatchlistGroupName] renamed list id={} to '{}'", id, name);
                                }
                            }
                        }
                    }
                    self.standalone_panels.wl_group_name_input.close();
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                }
                "cancel" | "close" => {
                    self.standalone_panels.wl_group_name_input.close();
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    eprintln!("[WatchlistGroupName] cancelled");
                }
                "input" => {
                    // Click-to-cursor — reposition caret. on_drag_end fires on mouseup
                    // via the global path in mod.rs::on_drag_end (line ~3614).
                    self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                }
                "modal_bg" => {
                    // Absorb clicks on modal background — do nothing
                }
                _ => {
                    eprintln!("[WatchlistGroupName] unhandled: {}", rest);
                }
            }
            return;
        }

        // === Watchlist modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("wl_modal:") {
            // Clear any leftover drag state so a click (< 5px movement) can
            // always dispatch through this handler properly.
            self.standalone_panels.watchlist_modal.drag_reorder = None;
            self.standalone_panels.watchlist_modal.drag_reorder_pending = None;
            self.handle_watchlist_modal_click(rest, x, y);
            return;
        }

        // === Clock popup clicks ===
        if widget_id == "clock_popup:bg" {
            self.panel_app.clock_popup_state.close();
            return;
        }
        if let Some(item) = widget_id.strip_prefix("clock_popup:") {
            if item == "clock:use_24h" {
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.scale_settings.time_format.use_24h =
                        !window.scale_settings.time_format.use_24h;
                }
                // keep popup open when toggling 24h
            } else if item == "show_utc" {
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.scale_settings.time_format.show_utc_prefix =
                        !window.scale_settings.time_format.show_utc_prefix;
                }
                // keep popup open
            } else if let Some(off) = item.strip_prefix("tz:") {
                if let Ok(offset) = off.parse::<i32>() {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.scale_settings.time_format.timezone_offset_hours = offset;
                    }
                }
                self.panel_app.clock_popup_state.close();
            }
            return;
        }

        // === Search modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("modal_search:") {
            self.handle_search_modal_click(rest, x, y);
            return;
        }

        // === Search modal compare-plus icon (merged-mode, symbol-search-
        // compare-inner arc, Wave 2, кусок 5) — its own top-level prefix
        // (not nested under "modal_search:") since it registers its own
        // hover rect distinct from the row body / star icon.
        if let Some(rest) = widget_id.strip_prefix("modal_cmp:") {
            self.handle_search_modal_cmp_click(rest);
            return;
        }

        // === Indicator search sidebar / sets clicks ===
        if let Some(rest) = widget_id.strip_prefix("ind_search:") {
            self.handle_indicator_search_action(rest);
            return;
        }

        // === Color picker clicks ===
        if widget_id.starts_with("color_picker_primitive:") {
            self.handle_color_picker_click(widget_id, x, y, "primitive");
            return;
        }
        if widget_id.starts_with("color_picker_indicator:") {
            self.handle_color_picker_click(widget_id, x, y, "indicator");
            return;
        }
        if widget_id.starts_with("color_picker_chart:") {
            self.handle_color_picker_click(widget_id, x, y, "chart");
            return;
        }
        if widget_id.starts_with("color_picker_panel:") {
            self.handle_color_picker_click(widget_id, x, y, "panel");
            return;
        }
        if widget_id.starts_with("color_picker_compare:") {
            self.handle_color_picker_click(widget_id, x, y, "compare");
            return;
        }

        // === Sync color grid clicks ===
        if widget_id.starts_with("sync_color_grid:") {
            self.handle_sync_color_grid_click(widget_id, x, y);
            return;
        }

        // === Preset name input modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("preset_name_input:") {
            match rest {
                "save" => {
                    // Read text from TextFieldStore.
                    let pni_id = WidgetId::from(crate::text_input::PRESET_NAME_INPUT);
                    let name = self.input_coordinator.borrow().text_fields().text(&pni_id).trim().to_string();
                    if !name.is_empty() {
                        use mlc_app_layout::ui::modal_settings::PresetNameInputMode;
                        match self.panel_app.preset_name_input.mode {
                            PresetNameInputMode::SaveAs => {
                                self.process_chart_out_event(
                                    mlc_core::events::ChartOutEvent::SavePreset { name }
                                );
                            }
                            PresetNameInputMode::Rename => {
                                let id = self.panel_app.preset_name_input.rename_preset_id
                                    .clone().unwrap_or_default();
                                self.process_chart_out_event(
                                    mlc_core::events::ChartOutEvent::RenamePreset { id, new_name: name }
                                );
                            }
                            PresetNameInputMode::NewChart => {
                                self.execute_new_chart_with_name(name);
                            }
                            PresetNameInputMode::CreateIndicatorSet => {
                                self.execute_create_indicator_set(name);
                            }
                        }
                    }
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    eprintln!("[ChartApp] preset name input committed via Save button");
                }
                "cancel" => {
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    eprintln!("[ChartApp] preset name input closed via Cancel button");
                }
                "close" => {
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    eprintln!("[ChartApp] preset name input closed via X");
                }
                "input" => {
                    // Click-to-cursor — reposition caret. on_drag_end fires on mouseup
                    // via the global path in mod.rs::on_drag_end (line ~3614).
                    self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                }
                "modal_bg" => {
                    // No-op — absorb click to prevent fall-through.
                }
                _ => {
                    eprintln!("[ChartApp] preset_name_input unhandled: {}", rest);
                }
            }
            return;
        }

        // === Chart browser modal clicks ===
        if let Some(action) = widget_id.strip_prefix("chart_browser:") {
            if action == "close" {
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                eprintln!("[ChartApp] chart browser closed via X");
            } else if action == "modal_bg" {
                // Absorb click — no-op.
            } else if action == "search_input" {
                // Click-to-cursor — reposition caret. on_drag_end fires on mouseup
                // via the global path in mod.rs::on_drag_end (line ~3614).
                self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
            } else if let Some(preset_id) = action.strip_prefix("rename:") {
                // Click rename icon — close browser, open rename modal for this preset.
                let preset_id = preset_id.to_string();
                let name_opt = self.core.presets().get(&preset_id).map(|p| p.name.clone());
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                if let Some(name) = name_opt {
                    self.panel_app.preset_name_input.open_rename(&preset_id, &name, 0);
                    self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
                    {
                        let pni_id = WidgetId::from(crate::text_input::PRESET_NAME_INPUT);
                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&pni_id, &name);
                        self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&pni_id);
                        self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::PRESET_NAME_INPUT);
                    }
                    eprintln!("[ChartApp] chart browser: rename preset '{}'", preset_id);
                }
            } else if let Some(preset_id) = action.strip_prefix("delete:") {
                // Click delete icon — delete preset.
                let preset_id = preset_id.to_string();
                eprintln!("[ChartApp] chart browser: delete preset '{}'", preset_id);
                self.process_chart_out_event(mlc_core::events::ChartOutEvent::DeletePreset { id: preset_id });
            } else if let Some(preset_id) = action.strip_prefix("item:") {
                // Click on preset row — load preset and close browser.
                // Read the flag BEFORE close() resets it.
                let open_in_new_tab = self.panel_app.chart_browser.open_in_new_tab;
                let preset_id = preset_id.to_string();
                eprintln!("[ChartApp] chart browser: {} preset '{}' (new_tab={})",
                    if open_in_new_tab { "open tab" } else { "load" }, preset_id, open_in_new_tab);
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
                if open_in_new_tab {
                    self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenTab { id: preset_id });
                } else {
                    // ChartBrowser "load" button = replace active slot.
                    self.process_chart_out_event(mlc_core::events::ChartOutEvent::LoadPresetInActiveSlot { id: preset_id });
                }
            } else {
                eprintln!("[ChartApp] chart_browser unhandled action: {}", action);
            }
            return;
        }

        // === User settings modal clicks ===
        if let Some(action) = widget_id.strip_prefix("user_settings:") {
            use mlc_app_layout::ui::modal_settings::UserSettingsTab;
            match action {
                "close" => {
                    self.panel_app.user_settings_state.close();
                    eprintln!("[ChartApp] user settings closed via X");
                }
                "modal_bg" => {
                    // Absorb click — defocus any active inline text inputs.
                    self.panel_app.user_settings_state.profile_rename_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                }
                "header" => {
                    // Drag start handled in mouse_down.
                }
                // Tab switching
                rest if rest.starts_with("tab:") => {
                    let tab_id = &rest[4..];
                    if let Some(tab) = UserSettingsTab::from_id(tab_id) {
                        self.panel_app.user_settings_state.set_tab(tab);
                        eprintln!("[ChartApp] user_settings tab switched to: {}", tab_id);
                    }
                }
                // Recalc mode radio option selected — hit id: "recalc_mode:{Key}"
                rest if rest.starts_with("recalc_mode:") => {
                    use crate::RecalcMode;
                    let mode_str = &rest["recalc_mode:".len()..];
                    let label = match mode_str {
                        "PerTick"  => "Per Tick",
                        "PerFrame" => "Per Frame",
                        "PerBar"   => "Per Bar",
                        _ => { eprintln!("[ChartApp] user_settings unknown recalc mode: {}", mode_str); return; }
                    };
                    self.panel_app.user_settings_state.recalc_mode_label = label.to_string();
                    let new_mode = match mode_str {
                        "PerTick" => RecalcMode::PerTick,
                        "PerBar"  => RecalcMode::PerBar,
                        _         => RecalcMode::PerFrame,
                    };
                    self.core.indicators_mut().set_recalc_mode(new_mode);
                    self.recalc_mode_changed = Some(mode_str.to_string());
                    // Reset counters and clear stale dirty flags when mode changes.
                    self.recalc_count = 0;
                    self.trade_count = 0;
                    self.recalc_log_timer = web_time::Instant::now();
                    self.core.indicators_mut().clear_pending();
                    eprintln!("[ChartApp] user_settings recalc_mode set to: {}", mode_str);
                }
                // Language radio option selected — hit id: "language:{code}"
                rest if rest.starts_with("language:") => {
                    let lang_code = &rest["language:".len()..];
                    let lang = mlc_core::i18n::Language::from_code(lang_code)
                        .unwrap_or(crate::Language::En);
                    crate::set_language(lang);
                    self.panel_app.user_settings_state.language = lang_code.to_string();
                    self.language_changed = Some(lang_code.to_string());
                    eprintln!("[ChartApp] language set to: {}", lang_code);
                }
                "diagnostics_toggle" => {
                    self.diagnostics_enabled = !self.diagnostics_enabled;
                    self.panel_app.user_settings_state.diagnostics_enabled = self.diagnostics_enabled;
                    eprintln!("[ChartApp] diagnostics_enabled = {}", self.diagnostics_enabled);
                }
                "trading_panel_topbar_takeover" => {
                    self.toggle_trading_panel_topbar_takeover();
                }
                "e2e_passphrase_input" => {
                    self.panel_app.user_settings_state.new_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(np_wid.clone());
                    // Position cursor at click point using pre-computed char positions.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "e2e_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&np_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] e2e_passphrase_input: focused");
                }
                "show_wizard" => {
                    self.panel_app.user_settings_state.show_welcome_wizard = true;
                    self.panel_app.user_settings_state.wizard_page = 0;
                    // Clear leaked state from profile_manager
                    {
                        let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                        let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        let mut ic = self.input_coordinator.borrow_mut();
                        ic.text_fields_mut().clear(&cp_wid);
                        ic.text_fields_mut().clear(&np_wid);
                        ic.text_fields_mut().blur();
                    }
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    eprintln!("[ChartApp] show_wizard: opening welcome wizard");
                }
                "server_toggle" => {
                    self.panel_app.user_settings_state.server_enabled =
                        !self.panel_app.user_settings_state.server_enabled;
                    self.server_enabled_changed = Some(self.panel_app.user_settings_state.server_enabled);
                    eprintln!("[ChartApp] server_enabled = {}", self.panel_app.user_settings_state.server_enabled);
                }
                // ── Welcome Wizard handlers ───────────────────────────────────
                // Page order: 0=Welcome+Lang, 1=Theme, 2=Profile+Passphrase
                "wizard_get_started" => {
                    // Page 0: user clicked Get Started — go to page 1 (Theme)
                    self.panel_app.user_settings_state.wizard_page = 1;
                    eprintln!("[ChartApp] wizard: Get Started clicked, going to page 1 (theme)");
                }
                "wizard_back" => {
                    // Back arrow — go to previous page
                    let current_page = self.panel_app.user_settings_state.wizard_page;
                    if current_page > 0 {
                        self.panel_app.user_settings_state.wizard_page = current_page - 1;
                    }
                    eprintln!("[ChartApp] wizard: back to page {}", current_page.saturating_sub(1));
                }
                "wizard_open_browser" => {
                    // Start device auth link flow (same as sign_in button)
                    self.pending_updater_cmd = Some("start_device_auth".to_string());
                    eprintln!("[ChartApp] wizard: starting device auth link flow");
                }
                id if id.starts_with("wizard_lang_") => {
                    let code = &id["wizard_lang_".len()..];
                    if let Some(lang) = mlc_core::i18n::Language::from_code(code) {
                        crate::set_language(lang);
                        self.panel_app.user_settings_state.language = code.to_string();
                        self.language_changed = Some(code.to_string());
                        eprintln!("[ChartApp] wizard: language set to {}", code);
                    }
                }
                "wizard_theme_dark" => {
                    self.panel_app.user_settings_state.wizard_selected_theme = "dark".to_string();
                    self.panel_app.theme_manager.set_preset("dark");
                    self.theme_changed = Some("dark".to_string());
                    eprintln!("[ChartApp] wizard: theme selected → dark");
                }
                "wizard_theme_light" => {
                    self.panel_app.user_settings_state.wizard_selected_theme = "light".to_string();
                    self.panel_app.theme_manager.set_preset("light");
                    self.theme_changed = Some("light".to_string());
                    eprintln!("[ChartApp] wizard: theme selected → light");
                }
                "wizard_theme_high_contrast" => {
                    self.panel_app.user_settings_state.wizard_selected_theme = "high_contrast".to_string();
                    self.panel_app.theme_manager.set_preset("high_contrast");
                    self.theme_changed = Some("high_contrast".to_string());
                    eprintln!("[ChartApp] wizard: theme selected → high_contrast");
                }
                "wizard_theme_high_contrast_mono" => {
                    self.panel_app.user_settings_state.wizard_selected_theme = "high_contrast_mono".to_string();
                    self.panel_app.theme_manager.set_preset("high_contrast_mono");
                    self.theme_changed = Some("high_contrast_mono".to_string());
                    eprintln!("[ChartApp] wizard: theme selected → high_contrast_mono");
                }
                "wizard_theme_mascot" => {
                    self.panel_app.user_settings_state.wizard_selected_theme = "mascot".to_string();
                    self.panel_app.theme_manager.set_preset("mascot");
                    self.theme_changed = Some("mascot".to_string());
                    eprintln!("[ChartApp] wizard: theme selected → mascot");
                }
                "wizard_theme_next" => {
                    // Page 1 (Theme): go to page 2 (Profile + Passphrase)
                    self.panel_app.user_settings_state.wizard_page = 2;
                    eprintln!("[ChartApp] wizard: theme done, going to page 2 (profile + passphrase)");
                }
                "wizard_profile_name_input" => {
                    // Focus the profile name input on the profile page
                    self.panel_app.user_settings_state.new_profile_name_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(npn_wid.clone());
                    // Clear selections on other fields
                    {
                        let mut ic = self.input_coordinator.borrow_mut();
                        if let Some(fs) = ic.text_fields_mut().field_state_mut(&np_wid) { fs.selection_start = None; }
                        if let Some(fs) = ic.text_fields_mut().field_state_mut(&cp_wid) { fs.selection_start = None; }
                    }
                    // Position cursor at click point using pre-computed char positions.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "wizard_profile_name_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&npn_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] wizard: profile name input focused");
                }
                "wizard_passphrase_input" => {
                    // Focus the passphrase input on the profile page
                    self.panel_app.user_settings_state.new_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(np_wid.clone());
                    // Clear selections on other fields
                    {
                        let mut ic = self.input_coordinator.borrow_mut();
                        if let Some(fs) = ic.text_fields_mut().field_state_mut(&npn_wid) { fs.selection_start = None; }
                        if let Some(fs) = ic.text_fields_mut().field_state_mut(&cp_wid) { fs.selection_start = None; }
                    }
                    // Position cursor at click point using pre-computed char positions.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "wizard_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&np_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] wizard: passphrase input focused");
                }
                "wizard_confirm_passphrase_input" => {
                    // Focus the confirm passphrase input on wizard page 2.
                    self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(cp_wid.clone());
                    // Clear selections on other fields
                    {
                        let mut ic = self.input_coordinator.borrow_mut();
                        if let Some(fs) = ic.text_fields_mut().field_state_mut(&np_wid) { fs.selection_start = None; }
                        if let Some(fs) = ic.text_fields_mut().field_state_mut(&npn_wid) { fs.selection_start = None; }
                    }
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "wizard_confirm_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&cp_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] wizard: confirm_passphrase_input focused");
                }
                "wizard_finish" => {
                    // Page 2: submit passphrase (and log profile name) then close wizard.
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    let passphrase = self.input_coordinator.borrow().text_fields().text(&np_wid).to_string();
                    let profile_name = self.input_coordinator.borrow().text_fields().text(&npn_wid).trim().to_string();
                    if passphrase.len() >= crate::user_manager::MIN_PASSPHRASE_LENGTH && !profile_name.is_empty() {
                        eprintln!("[ChartApp] wizard: profile name = {:?}", profile_name);
                        let profile_name_final = if profile_name.is_empty() { "Default".to_string() } else { profile_name.clone() };
                        self.pending_updater_cmd = Some(format!("wizard_complete:{}:{}", profile_name_final, passphrase));
                        self.panel_app.user_settings_state.show_welcome_wizard = false;
                        self.panel_app.user_settings_state.needs_vault_unlock = false;
                        {
                            let mut ic = self.input_coordinator.borrow_mut();
                            ic.text_fields_mut().clear(&np_wid);
                            ic.text_fields_mut().clear(&cp_wid);
                            ic.text_fields_mut().blur();
                        }
                        self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                        eprintln!("[ChartApp] wizard: setup complete");
                    }
                }
                "wizard_copy_key" => {
                    // Page 3: copy recovery key to clipboard
                    if let Some(ref key) = self.panel_app.user_settings_state.recovery_key_display {
                        self.clipboard_text = Some(key.clone());
                        self.panel_app.user_settings_state.recovery_key_copied_at =
                            web_time::SystemTime::now()
                                .duration_since(web_time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);
                        eprintln!("[ChartApp] wizard: recovery key copied to clipboard");
                    }
                }
                "wizard_recovery_confirm" => {
                    // Page 3: user confirmed they saved the recovery key — close wizard and promote
                    self.pending_updater_cmd = Some("recovery_key_confirmed".to_string());
                    self.panel_app.user_settings_state.show_welcome_wizard = false;
                    self.panel_app.user_settings_state.wizard_page = 0;
                    self.panel_app.user_settings_state.recovery_key_display = None;
                    {
                        let rk_wid = uzor::WidgetId::from(crate::text_input::RECOVERY_KEY);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&rk_wid);
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    }
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    eprintln!("[ChartApp] wizard: recovery key confirmed, closing wizard");
                }
                "profile_mgr:run_wizard" => {
                    self.panel_app.user_settings_state.show_welcome_wizard = true;
                    self.panel_app.user_settings_state.show_profile_manager = false;
                    self.panel_app.user_settings_state.wizard_page = 0;
                    // Clear leaked state from profile_manager
                    {
                        let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                        let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        let mut ic = self.input_coordinator.borrow_mut();
                        ic.text_fields_mut().clear(&cp_wid);
                        ic.text_fields_mut().clear(&np_wid);
                        ic.text_fields_mut().blur();
                    }
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    eprintln!("[ChartApp] profile_mgr: launching setup wizard");
                }
                // ── Vault unlock handler (returning encrypted users) ──────────
                "vault_unlock_btn" => {
                    // The user entered their passphrase on the vault-unlock overlay.
                    // Emit the e2e_setup: command so that main.rs derives the key and
                    // VALIDATES it before proceeding.  Do NOT dismiss the overlay here —
                    // main.rs will dismiss it on success, or set vault_unlock_error on
                    // failure so the user can retry with the correct passphrase.
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let passphrase = self.input_coordinator.borrow().text_fields().text(&np_wid).to_string();
                    if !passphrase.is_empty() {
                        // Clear any previous error while the request is in flight.
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.pending_updater_cmd = Some(format!("e2e_setup:{}", passphrase));
                        eprintln!("[ChartApp] vault_unlock: passphrase submitted, awaiting validation");
                    }
                }
                // ── Profile Manager handlers ───────────────────────────────────
                "profile_mgr:close" => {
                    // × button — dismiss profile manager, return to live chart
                    self.panel_app.user_settings_state.show_profile_manager = false;
                    self.panel_app.user_settings_state.profile_manager_page =
                        mlc_app_layout::ui::modal_settings::ProfileManagerPage::ProfileList;
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    {
                        let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                    }
                    eprintln!("[ChartApp] profile_mgr: close button clicked, dismissing");
                }
                "profile_mgr:back" => {
                    use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                    // Only block Back on ShowRecoveryKey — user MUST acknowledge recovery key
                    if matches!(self.panel_app.user_settings_state.profile_manager_page, ProfileManagerPage::ShowRecoveryKey) {
                        eprintln!("[ChartApp] profile_mgr: back blocked — must acknowledge recovery key");
                    } else if matches!(self.panel_app.user_settings_state.profile_manager_page, ProfileManagerPage::ProfileList) {
                        // Already on ProfileList — "Back" means dismiss if we have a live profile
                        if !self.panel_app.user_settings_state.runtime_profile_id.is_empty() {
                            self.panel_app.user_settings_state.show_profile_manager = false;
                            eprintln!("[ChartApp] profile_mgr: back from profile list — dismissing (live profile exists)");
                        }
                    } else {
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::ProfileList;
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.panel_app.user_settings_state.vault_unlock_attempts = 0;
                        {
                            let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                            self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                            self.input_coordinator.borrow_mut().text_fields_mut().blur();
                        }
                        self.panel_app.user_settings_state.new_passphrase_focused = false;
                        eprintln!("[ChartApp] profile_mgr: back to profile list");
                    }
                }
                "profile_mgr:create_new" => {
                    use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                    self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreateNew;
                    {
                        let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&npn_wid);
                    }
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    eprintln!("[ChartApp] profile_mgr: create new profile page");
                }
                "profile_mgr:unlock" => {
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let passphrase = self.input_coordinator.borrow().text_fields().text(&np_wid).to_string();
                    if !passphrase.is_empty() {
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.pending_updater_cmd = Some(format!("e2e_setup:{}", passphrase));
                        eprintln!("[ChartApp] profile_mgr: unlock passphrase submitted");
                    }
                }
                "profile_mgr:create_passphrase" => {
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    let passphrase = self.input_coordinator.borrow().text_fields().text(&np_wid).to_string();
                    let confirm = self.input_coordinator.borrow().text_fields().text(&cp_wid).to_string();
                    if passphrase.len() >= crate::user_manager::MIN_PASSPHRASE_LENGTH && passphrase == confirm {
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&cp_wid);
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                        self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                        self.pending_updater_cmd = Some(format!("e2e_setup:{}", passphrase));
                        eprintln!("[ChartApp] profile_mgr: create passphrase submitted");
                    }
                }
                "profile_mgr:create_confirm" => {
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    let name = self.input_coordinator.borrow().text_fields().text(&npn_wid).trim().to_string();
                    if !name.is_empty() {
                        self.pending_updater_cmd = Some(format!("profile_create:{}", name));
                        eprintln!("[ChartApp] profile_mgr: creating profile '{}'", name);
                    }
                }
                "profile_mgr:name_input" => {
                    self.panel_app.user_settings_state.new_profile_name_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(npn_wid.clone());
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:name_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&npn_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] profile_mgr: name input focused");
                }
                "profile_mgr:use_recovery_key" => {
                    use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                    self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UseRecoveryKey;
                    {
                        let rk_wid = uzor::WidgetId::from(crate::text_input::RECOVERY_KEY);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&rk_wid);
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    }
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    eprintln!("[ChartApp] profile_mgr: use recovery key page");
                }
                "profile_mgr:recovery_key_input" => {
                    self.panel_app.user_settings_state.recovery_key_display_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    let rk_wid = uzor::WidgetId::from(crate::text_input::RECOVERY_KEY);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(rk_wid.clone());
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:recovery_key_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] profile_mgr: recovery key input focused");
                }
                "profile_mgr:new_passphrase_input" => {
                    self.panel_app.user_settings_state.new_passphrase_focused = true;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(np_wid.clone());
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:new_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&np_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] profile_mgr: new_passphrase_input focused");
                }
                "profile_mgr:confirm_passphrase_input" => {
                    self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(cp_wid.clone());
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:confirm_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&cp_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] profile_mgr: confirm_passphrase_input focused");
                }
                "profile_mgr:create_confirm_passphrase_input" => {
                    // Confirm passphrase field on the CreatePassphrase page.
                    self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(cp_wid.clone());
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:create_confirm_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&cp_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] profile_mgr: create_confirm_passphrase_input focused");
                }
                "profile_mgr:save_new_passphrase" => {
                    let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                    let passphrase_text = self.input_coordinator.borrow().text_fields().text(&np_wid).to_string();
                    let confirm_text = self.input_coordinator.borrow().text_fields().text(&cp_wid).to_string();
                    use crate::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH;
                    if passphrase_text.len() >= MIN_PASSPHRASE_LENGTH && passphrase_text == confirm_text {
                        self.panel_app.user_settings_state.set_passphrase_error.clear();
                        self.panel_app.user_settings_state.new_passphrase_focused = false;
                        self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                        self.pending_updater_cmd = Some(format!("set_new_passphrase:{}", passphrase_text));
                        eprintln!("[ChartApp] profile_mgr: save_new_passphrase submitted");
                    } else if passphrase_text != confirm_text {
                        self.panel_app.user_settings_state.set_passphrase_error =
                            "Passphrases do not match".to_string();
                    } else {
                        self.panel_app.user_settings_state.set_passphrase_error =
                            format!("Passphrase must be at least {} characters", MIN_PASSPHRASE_LENGTH);
                    }
                }
                "profile_mgr:recovery_unlock" => {
                    let rk_wid = uzor::WidgetId::from(crate::text_input::RECOVERY_KEY);
                    let recovery_key_text = self.input_coordinator.borrow().text_fields().text(&rk_wid).to_string();
                    if !recovery_key_text.is_empty() {
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.pending_updater_cmd = Some(format!("recovery_unlock:{}", recovery_key_text));
                        eprintln!("[ChartApp] profile_mgr: recovery unlock submitted");
                    }
                }
                "profile_mgr:recovery_key_display" => {
                    // User clicked the recovery key display box — position cursor at click point.
                    self.panel_app.user_settings_state.recovery_key_display_focused = true;
                    let key_text = self.panel_app.user_settings_state.recovery_key_display
                        .clone()
                        .unwrap_or_default();
                    let rk_wid = uzor::WidgetId::from(crate::text_input::RECOVERY_KEY);
                    // Sync the store text in case it hasn't been set yet.
                    {
                        let current = self.input_coordinator.borrow().text_fields().text(&rk_wid).to_string();
                        if current != key_text {
                            self.input_coordinator.borrow_mut().text_fields_mut().set_text(&rk_wid, &key_text);
                        }
                    }
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(rk_wid.clone());
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:recovery_key_display"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&rk_wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                    }
                    eprintln!("[ChartApp] profile_mgr: recovery_key_display clicked, cursor positioned");
                }
                "profile_mgr:recovery_key_copy" => {
                    // Copy the recovery key to the system clipboard.
                    if let Some(ref key) = self.panel_app.user_settings_state.recovery_key_display {
                        self.clipboard_text = Some(key.clone());
                        eprintln!("[ChartApp] profile_mgr: recovery key copied to clipboard");
                    }
                }
                "profile_mgr:recovery_key_confirm" => {
                    // User confirmed they have written down the recovery key.
                    // Emit a command so main.rs clears pending_recovery_key and
                    // dismisses the ShowRecoveryKey page.
                    self.pending_updater_cmd = Some("recovery_key_confirmed".to_string());
                    eprintln!("[ChartApp] profile_mgr: recovery key confirmed by user");
                }
                // Legacy handler — kept for backwards compat
                "wizard_e2e" => {
                    self.panel_app.user_settings_state.wizard_e2e_chosen = true;
                    self.panel_app.user_settings_state.wizard_page = 1;
                    eprintln!("[ChartApp] wizard: legacy wizard_e2e handler");
                }
                // ── Profile inline input focus ─────────────────────────────────
                "profile_rename_input" => {
                    self.panel_app.user_settings_state.profile_rename_focused = true;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    eprintln!("[ChartApp] profile_rename_input: focused");
                }
                "new_profile_name_input" => {
                    self.panel_app.user_settings_state.new_profile_name_focused = true;
                    self.panel_app.user_settings_state.profile_rename_focused = false;
                    eprintln!("[ChartApp] new_profile_name_input: focused");
                }
                // ── Profile handlers ───────────────────────────────────────────
                "profile_rename_confirm" => {
                    let pr_wid = uzor::WidgetId::from(crate::text_input::PROFILE_RENAME);
                    let new_name = self.input_coordinator.borrow().text_fields().text(&pr_wid).trim().to_string();
                    let target_id = self.panel_app.user_settings_state.profile_rename_target_id.clone();
                    if !new_name.is_empty() {
                        if target_id.as_deref() == Some(&self.panel_app.user_settings_state.profile_id) {
                            self.panel_app.user_settings_state.profile_display_name = new_name.clone();
                        }
                        self.panel_app.user_settings_state.profile_rename_mode = false;
                        self.panel_app.user_settings_state.profile_rename_focused = false;
                        let tid = target_id.unwrap_or_else(|| self.panel_app.user_settings_state.profile_id.clone());
                        self.panel_app.user_settings_state.profile_rename_target_id = None;
                        self.pending_updater_cmd = Some(format!("profile_rename:{}:{}", tid, new_name));
                        eprintln!("[ChartApp] profile_rename_confirm: id={} new name = {}", tid, new_name);
                    }
                }
                "profile_rename_cancel" => {
                    self.panel_app.user_settings_state.profile_rename_mode = false;
                    self.panel_app.user_settings_state.profile_rename_focused = false;
                    {
                        let pr_wid = uzor::WidgetId::from(crate::text_input::PROFILE_RENAME);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&pr_wid);
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    }
                    self.panel_app.user_settings_state.profile_rename_target_id = None;
                    eprintln!("[ChartApp] profile_rename_cancel");
                }
                "profile_new" => {
                    self.panel_app.user_settings_state.show_new_profile_dialog = true;
                    self.panel_app.user_settings_state.new_profile_name_focused = true;
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    self.input_coordinator.borrow_mut().text_fields_mut().clear(&npn_wid);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(npn_wid);
                    eprintln!("[ChartApp] profile_new: opening dialog");
                }
                "profile_new_confirm" => {
                    let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                    let name = self.input_coordinator.borrow().text_fields().text(&npn_wid).trim().to_string();
                    if !name.is_empty() {
                        self.pending_updater_cmd = Some(format!("profile_create:{}", name));
                        self.panel_app.user_settings_state.show_new_profile_dialog = false;
                        self.panel_app.user_settings_state.new_profile_name_focused = false;
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&npn_wid);
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                        eprintln!("[ChartApp] profile_new_confirm: creating profile '{}'", name);
                    }
                }
                "profile_new_cancel" => {
                    self.panel_app.user_settings_state.show_new_profile_dialog = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    {
                        let npn_wid = uzor::WidgetId::from(crate::text_input::NEW_PROFILE_NAME);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&npn_wid);
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    }
                    eprintln!("[ChartApp] profile_new_cancel");
                }
                rest if rest.starts_with("vault_picker_profile:") => {
                    use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                    let profile_id = &rest["vault_picker_profile:".len()..];
                    let target_has_vault = self.panel_app.user_settings_state.profiles_with_vault_status
                        .iter()
                        .find(|(id, _, _, _)| id == profile_id)
                        .map(|(_, _, _, has_vault)| *has_vault)
                        .unwrap_or(false);
                    let target_name = self.panel_app.user_settings_state.profiles_with_vault_status
                        .iter()
                        .find(|(id, _, _, _)| id == profile_id)
                        .map(|(_, name, _, _)| name.clone())
                        .unwrap_or_default();
                    {
                        let np_wid = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid);
                    }
                    if !target_has_vault {
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        eprintln!("[ChartApp] vault_picker: passphrase setup for unencrypted profile {}", profile_id);
                    } else {
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.panel_app.user_settings_state.vault_unlock_attempts = 0;
                        eprintln!("[ChartApp] vault_picker: passphrase prompt for profile {}", profile_id);
                    }
                    // Switch to profile_manager modal to show the passphrase page
                    self.panel_app.user_settings_state.show_profile_manager = true;
                    self.panel_app.user_settings_state.show_welcome_wizard = false;
                    // Clear leaked state from wizard
                    {
                        let cp_wid = uzor::WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&cp_wid);
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    }
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                }
                rest if rest.starts_with("profile_mgr:select:") => {
                    use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                    let profile_id = &rest["profile_mgr:select:".len()..];
                    // Check if target profile has vault (encrypted)
                    let target_has_vault = self.panel_app.user_settings_state.profiles_with_vault_status
                        .iter()
                        .find(|(id, _, _, _)| id == profile_id)
                        .map(|(_, _, _, has_vault)| *has_vault)
                        .unwrap_or(false);
                    let target_name = self.panel_app.user_settings_state.profiles_with_vault_status
                        .iter()
                        .find(|(id, _, _, _)| id == profile_id)
                        .map(|(_, name, _, _)| name.clone())
                        .unwrap_or_default();
                    let np_wid_sel = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                    if !target_has_vault {
                        // Unencrypted profile — show CreatePassphrase inline (NO hot-reload)
                        // After passphrase is set, main.rs will create vault in target dir then switch
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid_sel);
                        eprintln!("[ChartApp] profile_mgr: showing passphrase setup for unencrypted profile {}", profile_id);
                    } else if profile_id == self.panel_app.user_settings_state.runtime_profile_id {
                        if self.panel_app.user_settings_state.needs_vault_unlock {
                            // Current profile, vault locked — show unlock passphrase page
                            self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                            self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                            self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                            self.panel_app.user_settings_state.vault_unlock_error = None;
                            self.panel_app.user_settings_state.vault_unlock_attempts = 0;
                            self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid_sel);
                            eprintln!("[ChartApp] profile_mgr: showing unlock for current profile {}", profile_id);
                        } else {
                            // Already on this profile, vault OK — dismiss
                            self.panel_app.user_settings_state.show_profile_manager = false;
                            eprintln!("[ChartApp] profile_mgr: selected current profile, dismissing");
                        }
                    } else {
                        // Show passphrase prompt BEFORE switching — no hot-reload until validated
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.panel_app.user_settings_state.vault_unlock_attempts = 0;
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid_sel);
                        eprintln!("[ChartApp] profile_mgr: showing passphrase prompt for profile {}", profile_id);
                    }
                }
                rest if rest.starts_with("profile_rename:") => {
                    let id = &rest["profile_rename:".len()..];
                    let current_name = {
                        let uss = &self.panel_app.user_settings_state;
                        uss.available_profiles.iter()
                            .find(|(pid, _, _)| pid == id)
                            .map(|(_, name, _)| name.clone())
                            .unwrap_or_else(|| uss.profile_display_name.clone())
                    };
                    self.panel_app.user_settings_state.profile_rename_mode = true;
                    self.panel_app.user_settings_state.profile_rename_focused = true;
                    self.panel_app.user_settings_state.profile_rename_target_id = Some(id.to_string());
                    let pr_wid = uzor::WidgetId::from(crate::text_input::PROFILE_RENAME);
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&pr_wid, &current_name);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(pr_wid.clone());
                    // Position cursor at end
                    let char_count = current_name.chars().count();
                    if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&pr_wid) {
                        fs.cursor = char_count;
                        fs.selection_start = None;
                    }
                    eprintln!("[ChartApp] profile_rename:{}: entering rename mode", id);
                }
                rest if rest.starts_with("profile_avatar_toggle:") => {
                    let id = &rest["profile_avatar_toggle:".len()..];
                    let uss = &mut self.panel_app.user_settings_state;
                    let already_open = uss.show_avatar_picker
                        && uss.profile_avatar_target_id.as_deref() == Some(id);
                    if already_open {
                        uss.show_avatar_picker = false;
                        uss.profile_avatar_target_id = None;
                    } else {
                        uss.show_avatar_picker = true;
                        uss.profile_avatar_target_id = Some(id.to_string());
                    }
                    eprintln!(
                        "[ChartApp] profile_avatar_toggle:{}: open = {}",
                        id,
                        uss.show_avatar_picker
                    );
                }
                rest if rest.starts_with("profile_switch:") => {
                    use mlc_app_layout::ui::modal_settings::ProfileManagerPage;
                    let profile_id = &rest["profile_switch:".len()..];
                    // Check if target profile has vault
                    let target_has_vault = self.panel_app.user_settings_state.profiles_with_vault_status
                        .iter()
                        .find(|(id, _, _, _)| id == profile_id)
                        .map(|(_, _, _, has_vault)| *has_vault)
                        .unwrap_or(false);
                    let target_name = self.panel_app.user_settings_state.profiles_with_vault_status
                        .iter()
                        .find(|(id, _, _, _)| id == profile_id)
                        .map(|(_, name, _, _)| name.clone())
                        .unwrap_or_default();
                    if profile_id == self.panel_app.user_settings_state.runtime_profile_id {
                        // Already on this profile — dismiss
                        eprintln!("[ChartApp] profile_switch: already on this profile, ignoring");
                    } else {
                        let np_wid_sw = uzor::WidgetId::from(crate::text_input::NEW_PASSPHRASE);
                        self.input_coordinator.borrow_mut().text_fields_mut().clear(&np_wid_sw);
                        if !target_has_vault {
                            // Unencrypted — show CreatePassphrase inline
                            self.panel_app.user_settings_state.show_profile_manager = true;
                            self.panel_app.user_settings_state.is_open = false;
                            self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                            self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                            self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                            eprintln!("[ChartApp] profile_switch: passphrase setup for unencrypted {}", profile_id);
                        } else {
                            // Encrypted — show UnlockPassphrase inline
                            self.panel_app.user_settings_state.show_profile_manager = true;
                            self.panel_app.user_settings_state.is_open = false;
                            self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                            self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                            self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                            self.panel_app.user_settings_state.vault_unlock_error = None;
                            self.panel_app.user_settings_state.vault_unlock_attempts = 0;
                            eprintln!("[ChartApp] profile_switch: passphrase prompt for {}", profile_id);
                        }
                    }
                }
                rest if rest.starts_with("profile_avatar:") => {
                    let avatar = &rest["profile_avatar:".len()..];
                    let uss = &mut self.panel_app.user_settings_state;
                    let target_id = uss.profile_avatar_target_id.clone();
                    // If targeting the active profile (or no explicit target), update active avatar
                    let is_active_target = target_id.as_deref()
                        .map(|tid| tid == uss.runtime_profile_id.as_str())
                        .unwrap_or(true);
                    if is_active_target {
                        uss.profile_avatar = avatar.to_string();
                        self.pending_updater_cmd = Some(format!("profile_set_avatar:{}", avatar));
                        eprintln!("[ChartApp] profile_avatar:{}: updated active profile avatar", avatar);
                    }
                    uss.show_avatar_picker = false;
                    uss.profile_avatar_target_id = None;
                }
                rest if rest.starts_with("profile_delete:") => {
                    let id = &rest["profile_delete:".len()..];
                    self.pending_updater_cmd = Some(format!("profile_delete:{}", id));
                    eprintln!("[ChartApp] profile_delete: deleting profile id = {}", id);
                }
                "profile_mgr:sign_in" => {
                    self.pending_updater_cmd = Some("start_device_auth".to_string());
                    eprintln!("[ChartApp] profile_mgr: sign_in via device auth link");
                }
                "profile_mgr:logout" => {
                    self.pending_updater_cmd = Some("logout".to_string());
                    eprintln!("[ChartApp] profile_mgr: logout requested");
                }
                _ => {
                    eprintln!("[ChartApp] user_settings unhandled action: {}", action);
                }
            }
            return;
        }

        // === Context menu clicks ===
        if let Some(rest) = widget_id.strip_prefix("context_menu:") {
            if rest == "bg" {
                // Click on context menu background — close the menu.
                self.panel_app.context_menu_state.close();
                return;
            }
            if let Some(action) = rest.strip_prefix("item:") {
                eprintln!("[ChartApp] Context menu action: {}", action);
                self.on_context_menu_action(action);
                return;
            }
        }

        // === Panel overlay zones (free-slot leaf color tag / gear) ===
        if let Some(rest) = widget_id.strip_prefix("panel_overlay:") {
            if let Some(colon) = rest.rfind(':') {
                let panel_id_str = &rest[..colon];
                let action = &rest[colon + 1..];
                if let Ok(panel_id) = panel_id_str.parse::<u64>() {
                    match action {
                        "color_tag" => {
                            if let Some(zones) = self.last_sidebar_result.as_ref()
                                .and_then(|sr| sr.panel_overlay_zones.iter().find(|(pid, _, _)| *pid == panel_id))
                                .map(|(_, _, z)| z.clone())
                            {
                                self.panel_app.sync_color_grid.open_for_panel(
                                    panel_id,
                                    zones.color_tag_rect[0],
                                    zones.color_tag_rect[1] + zones.color_tag_rect[3],
                                    self.width as f64,
                                    self.height as f64,
                                );
                            }
                        }
                        "gear" => {
                            // Overlay-gear doctrine (owner spec 2026-07-28):
                            // the trading container's gear opens the single
                            // COMMON modal; every other standalone panel's
                            // gear opens ITS OWN settings surface (dropdown
                            // or modal per the persisted preference).
                            let is_trading_container = self.panels_store.trading_container
                                .contains_key(&mlc_core::state::PanelId(panel_id));
                            if is_trading_container {
                                self.tc_modal.open();
                            } else if let Some((leaf_id, zones)) = self.last_sidebar_result.as_ref()
                                .and_then(|sr| sr.panel_overlay_zones.iter().find(|(pid, _, _)| *pid == panel_id))
                                .map(|(_, lid, z)| (*lid, z.clone()))
                            {
                                let panel = self.sidebar_state.slot_dockings.iter().find_map(|sd| {
                                    sd.inner().tree().leaf(leaf_id).and_then(|l| l.active_panel().cloned())
                                });
                                if let Some(panel) = panel {
                                    // Anchor: popup's right edge to the gear's
                                    // right edge, just below it (renderer
                                    // clamps to the screen).
                                    let anchor = (
                                        zones.dots_rect[0] + zones.dots_rect[2] - 240.0,
                                        zones.dots_rect[1] + zones.dots_rect[3] + 2.0,
                                    );
                                    self.panel_settings_open = Some((panel, anchor));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            return;
        }

        // === Overlay tab clicks — gear menu, color tag, body ===
        if let Some(rest) = widget_id.strip_prefix("leaf_tab:") {
            if let Some(colon) = rest.find(':') {
                let leaf_id_str = &rest[..colon];
                let sub_zone = &rest[colon + 1..];
                if let Ok(lid) = leaf_id_str.parse::<u64>() {
                    let leaf_id = mlc_core::LeafId(lid);
                    match sub_zone {
                        "gear" => {
                            self.panel_app.open_tags_tabs_for_leaf(leaf_id);
                        }
                        "color_tag" => {
                            if let Some(zones) = self.leaf_tab_hit_zones.get(&leaf_id).cloned() {
                                let anchor = zones.color_tag_rect;
                                self.panel_app.sync_color_grid.open(
                                    leaf_id,
                                    anchor[0],
                                    // Position below the color tag square
                                    anchor[1] + anchor[3],
                                    self.width as f64,
                                    self.height as f64,
                                );
                            }
                        }
                        _ => {
                            self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        }
                    }
                }
            }
            return;
        }

        // chart:pane BlackboxPanel — click landed on the chart canvas.
        // Close any open dropdowns/context-menu first (canvas click should
        // dismiss transient overlays — same as the on_click fallthrough path
        // used to do before chart:pane became a registered widget). Then
        // route to the chart's own dispatcher which handles drawing tool
        // placement, primitive selection, scale corner buttons, and scales
        // via panel_grid.resolve_input.
        //
        // GUARD: when a click-based drawing tool is active, canvas placement
        // already happens on mouse-press via on_drag_start. Skipping the
        // release-path call here prevents the same point from being
        // registered twice (which collapses 2-click primitives into
        // zero-length shapes — invisible on the canvas).
        if widget_id.starts_with("chart:pane:") {
            self.close_transient_overlays();
            if !self.has_click_drawing_tool() {
                self.handle_canvas_click(x, y);
            }
            return;
        }

        eprintln!("[ChartApp] unhandled widget click: {}", widget_id);
    }

    /// Spawn a `Panel` of `kind_str` inside the typed slot at `idx`.
    ///
    /// Called from the `[+]` dropdown click handler AND from the auto-seed
    /// path (`ChartOutEvent::ToggleSlot` — first open of an empty typed
    /// slot). `kind_str` is validated against the slot's
    /// [`mlc_app_sidebar_content::free_slot::SlotKind::allowed_panel_kinds`]
    /// — a kind that does not belong to this slot's type is a silent no-op
    /// (defense in depth; the dropdown itself is already filtered so this
    /// should never trigger from real UI clicks).
    pub(super) fn spawn_panel_in_slot(&mut self, idx: usize, kind_str: &str) {
        use mlc_app_sidebar_content::free_slot::SlotKind;

        let Some(slot_kind) = SlotKind::from_index(idx) else { return };
        if !slot_kind.allowed_panel_kinds().contains(&kind_str) {
            eprintln!(
                "[ChartApp] spawn_panel_in_slot: kind '{}' not allowed in slot {} ({:?}) — ignored",
                kind_str, idx, slot_kind
            );
            return;
        }

        use mlc_app_panels::trading::SymbolSource;
        use mlc_app_sidebar_content::state::SlotSourceMode;

        // Build SymbolSource from the toolbar's current source mode.
        // S2.7.1e: read instrument fields via branch_meta (B1).
        let spawn_active_cid = self.panel_app.panel_grid.active_container_id();
        let source = match self.sidebar_state.slot_source_mode {
            SlotSourceMode::Tagged => SymbolSource::HyperFocus,
            SlotSourceMode::Untagged => {
                // Frozen snapshot of active chart key at spawn time.
                let sym: String = spawn_active_cid
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let exch: String = spawn_active_cid
                    .and_then(|cid| self.exchange_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_else(|| self.active_exchange.as_str().to_string());
                let at: String = spawn_active_cid
                    .and_then(|cid| self.account_type_for_chart(cid))
                    .map(|at| at.to_string())
                    .unwrap_or_else(|| digdigdig3::AccountType::Spot.short_label().to_string());
                SymbolSource::Fixed { symbol: sym, exchange: exch, account_type: at }
            }
        };

        // Resolve immediately to get the concrete symbol for initial setup.
        // S2.7.1e: read instrument fields via branch_meta (B1).
        let active_resolved = spawn_active_cid.map(|cid| {
            mlc_app_panels::trading::ResolvedSymbol {
                symbol: self.symbol_for_chart(cid)
                    .map(String::from).unwrap_or_default(),
                exchange: self.exchange_for_chart(cid)
                    .map(String::from).unwrap_or_default(),
                account_type: self.account_type_for_chart(cid)
                    .map(|at| at.to_string()).unwrap_or_default(),
            }
        });
        let resolve_leaf = |lid: u64| -> Option<mlc_app_panels::trading::ResolvedSymbol> {
            let leaf = mlc_core::LeafId(lid);
            let leaf_cid = self.panel_app.panel_grid.container_id_for_leaf(leaf)?;
            Some(mlc_app_panels::trading::ResolvedSymbol {
                symbol: self.symbol_for_chart(leaf_cid)
                    .map(String::from).unwrap_or_default(),
                exchange: self.exchange_for_chart(leaf_cid)
                    .map(String::from).unwrap_or_default(),
                account_type: self.account_type_for_chart(leaf_cid)
                    .map(|at| at.to_string()).unwrap_or_default(),
            })
        };
        let resolved = source.resolve(active_resolved.as_ref(), &resolve_leaf);
        // No resolution (no active window / unresolved BoundToChart) → empty
        // symbol. Panel will pick up its key via group propagation rather
        // than carrying a hardcoded BTCUSDT.
        let symbol = resolved.as_ref().map(|r| r.symbol.clone())
            .unwrap_or_default();
        let exchange_str = resolved.as_ref().map(|r| r.exchange.clone())
            .unwrap_or_else(|| self.active_exchange.as_str().to_string());
        let account_type_str = resolved.as_ref().map(|r| r.account_type.clone())
            .unwrap_or_else(|| digdigdig3::AccountType::Spot.short_label().to_string());

        let resolved_tick_size = self.core.exchange_symbols()
            .tick_size_for(&exchange_str, &symbol)
            .filter(|t| *t > 0.0)
            .unwrap_or(0.01);

        let item_opt = match kind_str {
            "dom" => {
                let pid = self.panels_store.create_dom(symbol.clone(), resolved_tick_size);
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    let trade_handle = self.core.subscribe_trades(eid, &symbol, at);
                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                        state.exchange = exchange_str.clone();
                        state.account_type = account_type_str.clone();
                        state.shared_orderbook = self.core.subscribe_orderbook(eid, &symbol, at);
                        state.last_seen_update_id = None;
                        state.level_qty_bid.clear();
                        state.level_qty_ask.clear();
                        state.shared_trades = trade_handle;
                    }
                } else if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::Dom(pid))
            }
            "footprint" => {
                let pid = self.panels_store.create_footprint(symbol.clone(), resolved_tick_size);
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    let handle = self.core.subscribe_trades(eid, &symbol, at);
                    if let Some(state) = self.panels_store.footprint.get_mut(&pid) {
                        state.exchange = exchange_str.clone();
                        state.account_type = account_type_str.clone();
                        state.shared_trades = handle;
                        state.last_seen_ts_ms = 0;
                    }
                } else if let Some(state) = self.panels_store.footprint.get_mut(&pid) {
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::Footprint(pid))
            }
            "volume_profile" => {
                let pid = self.panels_store.create_volume_profile(symbol.clone(), resolved_tick_size);
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    let handle = self.core.subscribe_trades(eid, &symbol, at);
                    if let Some(state) = self.panels_store.volume_profile.get_mut(&pid) {
                        state.exchange = exchange_str.clone();
                        state.account_type = account_type_str.clone();
                        state.shared_trades = handle;
                        state.last_seen_ts_ms = 0;
                    }
                } else if let Some(state) = self.panels_store.volume_profile.get_mut(&pid) {
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::VolumeProfile(pid))
            }
            "liquidity_heatmap" => {
                let pid = self.panels_store.create_liquidity_heatmap(symbol.clone(), resolved_tick_size, 1000);
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    if let Some(state) = self.panels_store.liquidity_heatmap.get_mut(&pid) {
                        state.exchange = exchange_str.clone();
                        state.account_type = account_type_str.clone();
                        state.shared_orderbook = self.core.subscribe_orderbook(eid, &symbol, at);
                        state.last_seen_update_id = None;
                    }
                } else if let Some(state) = self.panels_store.liquidity_heatmap.get_mut(&pid) {
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::LiquidityHeatmap(pid))
            }
            "big_trades" => {
                let pid = self.panels_store.create_big_trades();
                // Subscribe to the shared trade ring and attach the handle
                // to the panel state so tick() can pull new trades.
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    let handle = self.core.subscribe_trades(eid, &symbol, at);
                    if let Some(state) = self.panels_store.big_trades.get_mut(&pid) {
                        state.symbol = symbol.clone();
                        state.exchange = exchange_str.clone();
                        state.account_type = account_type_str.clone();
                        state.shared_trades = handle;
                        state.last_seen_ts_ms = 0;
                    }
                } else if let Some(state) = self.panels_store.big_trades.get_mut(&pid) {
                    state.symbol = symbol.clone();
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::BigTrades(pid))
            }
            "l2_tape" => {
                let pid = self.panels_store.create_l2_tape();
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    if let Some(state) = self.panels_store.l2_tape.get_mut(&pid) {
                        state.symbol = symbol.clone();
                        state.exchange = exchange_str.clone();
                        state.account_type = account_type_str.clone();
                        state.shared_orderbook = self.core.subscribe_orderbook(eid, &symbol, at);
                        state.last_seen_update_id = None;
                        state.previous_book.clear();
                    }
                } else if let Some(state) = self.panels_store.l2_tape.get_mut(&pid) {
                    state.symbol = symbol.clone();
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::L2Tape(pid))
            }
            "trade_tape" => {
                let pid = self.panels_store.create_trade_tape();
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    let handle = self.core.subscribe_trades(eid, &symbol, at);
                    if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                        state.symbol = symbol.clone();
                        state.exchange = exchange_str.clone();
                        state.account_type = account_type_str.clone();
                        state.shared_trades = handle;
                        state.last_seen_ts_ms = 0;
                    }
                } else if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                    state.symbol = symbol.clone();
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::TradeTape(pid))
            }
            "order_entry" => {
                let pid = self.panels_store.create_order_entry(symbol.clone());
                if let Some(state) = self.panels_store.order_entry.get_mut(&pid) {
                    state.source = source.clone();
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                // Bind this panel's own (exchange, account_type) trading
                // snapshot — creation-time only, no rebind path if the
                // panel's symbol source changes later.
                let eid = digdigdig3::ExchangeId::from_str(&exchange_str).unwrap_or(self.active_exchange);
                let at = crate::account_type_from_label(&account_type_str);
                if let Some(mut tm) = self.core.trading_mut() {
                    let snap = tm.snapshot_for(eid, at);
                    if let Some(state) = self.panels_store.order_entry.get_mut(&pid) {
                        state.set_snapshot(snap);
                    }
                }
                Some(mlc_core::state::Panel::OrderEntry(pid))
            }
            "position_manager" => {
                let pid = self.panels_store.create_position_manager();
                // Same creation-time-only snapshot binding as "order_entry".
                let eid = digdigdig3::ExchangeId::from_str(&exchange_str).unwrap_or(self.active_exchange);
                let at = crate::account_type_from_label(&account_type_str);
                if let Some(mut tm) = self.core.trading_mut() {
                    let snap = tm.snapshot_for(eid, at);
                    if let Some(state) = self.panels_store.position_manager.get_mut(&pid) {
                        state.set_snapshot(snap);
                    }
                }
                Some(mlc_core::state::Panel::PositionManager(pid))
            }
            "trade_log" => {
                let pid = self.panels_store.create_trade_log();
                // Same creation-time-only snapshot binding as "order_entry".
                let eid = digdigdig3::ExchangeId::from_str(&exchange_str).unwrap_or(self.active_exchange);
                let at = crate::account_type_from_label(&account_type_str);
                if let Some(mut tm) = self.core.trading_mut() {
                    let snap = tm.snapshot_for(eid, at);
                    if let Some(state) = self.panels_store.trade_log.get_mut(&pid) {
                        state.set_snapshot(snap);
                    }
                }
                Some(mlc_core::state::Panel::TradeLog(pid))
            }
            "risk_calculator" => {
                let pid = self.panels_store.create_risk_calculator();
                Some(mlc_core::state::Panel::RiskCalculator(pid))
            }
            "trading_container" => {
                // `resolved_tick_size` (computed above, same as every other
                // spawn arm) — was hardcoded `0.01` here, unlike "dom"/
                // "footprint"/"volume_profile"/"liquidity_heatmap" which all
                // already use it (found while investigating the OKX
                // orderbook-tick-size bug, 2026-07-25).
                let pid = self.panels_store.create_trading_container(symbol.clone(), resolved_tick_size, 0.0);
                // Header-tab TF control default (trading-header-tf arc,
                // 2026-07-25) — seeded from the active chart's live
                // timeframe at spawn time, "1m" when none is known yet.
                let seed_tf = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.timeframe_for_chart(cid))
                    .map(|tf| tf.name.clone())
                    .unwrap_or_else(|| "1m".to_string());
                if let Some(state) = self.panels_store.trading_container.get_mut(&pid) {
                    state.source = source.clone();
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                    state.timeframe = seed_tf;
                    state.dom.exchange = exchange_str.clone();
                    state.dom.account_type = account_type_str.clone();
                    state.order_entry.exchange = exchange_str.clone();
                    state.order_entry.account_type = account_type_str.clone();
                }
                // Subscribe the container's hosted DOM to live orderbook/trade
                // feeds — same pattern as the standalone "dom" spawn arm above.
                if !symbol.is_empty() {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    let trade_handle = self.core.subscribe_trades(eid, &symbol, at);
                    let ob_handle = self.core.subscribe_orderbook(eid, &symbol, at);
                    if let Some(state) = self.panels_store.trading_container.get_mut(&pid) {
                        state.dom.shared_orderbook = ob_handle;
                        state.dom.last_seen_update_id = None;
                        state.dom.level_qty_bid.clear();
                        state.dom.level_qty_ask.clear();
                        state.dom.shared_trades = trade_handle;
                    }
                }
                // Bind the container's OrderEntry / PositionManager / TradeLog
                // to the same (exchange, account_type) trading snapshot as
                // their standalone counterparts (creation-time-only binding —
                // see the "order_entry" arm above).
                {
                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                        .unwrap_or(self.active_exchange);
                    let at = crate::account_type_from_label(&account_type_str);
                    if let Some(mut tm) = self.core.trading_mut() {
                        let snap = tm.snapshot_for(eid, at);
                        if let Some(state) = self.panels_store.trading_container.get_mut(&pid) {
                            state.order_entry.set_snapshot(snap.clone());
                            state.position_manager.set_snapshot(snap.clone());
                            state.trade_log.set_snapshot(snap.clone());
                            state.orders_log.set_snapshot(snap.clone());
                            state.metrics.set_snapshot(snap.clone());
                            state.risk_calculator.set_snapshot(snap);
                        }
                    }
                }
                Some(mlc_core::state::Panel::TradingContainer(pid))
            }
            "indicator_panel" => {
                // Spawn unbound (instance_id=0).  User picks the
                // indicator from the panel's own control plane.
                let pid = self.panels_store.create_indicator_panel(symbol.clone(), 0);
                if let Some(state) = self.panels_store.indicator_panel.get_mut(&pid) {
                    state.exchange = exchange_str.clone();
                    state.account_type = account_type_str.clone();
                }
                Some(mlc_core::state::Panel::IndicatorPanel(pid))
            }
            _ => None,
        };

        // Subscribe depth for panels that need raw orderbook data.
        // DOM is excluded here because it subscribes via subscribe_orderbook()
        // above, which internally calls subscribe_depth().
        let needs_depth = matches!(kind_str, "l2_tape" | "liquidity_heatmap");
        if needs_depth {
            let eid = self.core.exchange_symbols()
                .find_by_label(&exchange_str)
                .unwrap_or(self.active_exchange);
            let at = crate::account_type_from_label(&account_type_str);
            self.core.subscribe_depth(eid, &symbol, at);
        }

        // Register market-data panels in TagManager as Synced members.
        // order_entry / trading_container are account-bound, not market-data.
        // indicator_panel is market-data (symbol-bound).
        let is_market_data = matches!(
            kind_str,
            "dom" | "footprint" | "volume_profile" | "liquidity_heatmap"
                | "big_trades" | "l2_tape" | "trade_tape" | "indicator_panel"
        );
        if is_market_data {
            if let Some(ref item) = item_opt {
                let pid = mlc_app_sidebar_content::free_slot::PanelId(item.panel_id().0);
                let member = mlc_core::ids::SyncMemberId::Panel(pid.0);
                match self.sidebar_state.slot_source_mode {
                    mlc_app_sidebar_content::state::SlotSourceMode::Tagged => {
                        // Attach to active chart's sync group. If that group is
                        // still auto-created (untagged), promote it to a real
                        // visible color tag so the panel-chart pair shows a
                        // shared color marker — this is the moment a 1-member
                        // untagged group becomes a 2+-member tagged one.
                        let group_id = self.core.tags().active_chart_tag()
                            .or_else(|| {
                                self.panel_app.panel_grid
                                    .active_container_id()
                                    .and_then(|cid| self.bubble_for_chart(cid))
                            });
                        if let Some(gid) = group_id {
                            // Promote if currently auto-created.
                            let was_auto = self.core.tags().get(gid)
                                .map(|t| t.auto_created)
                                .unwrap_or(false);
                            if was_auto {
                                if let Some(new_color) = self.promote_to_tagged(gid) {
                                    // Mirror color into leaf_color_tags for every
                                    // chart member of this group so chart leaf
                                    // headers also show the new color marker.
                                    let chart_member_ids: Vec<mlc_core::ChartId> =
                                        self.core.tags().chart_member_ids(gid)
                                            .into_iter()
                                            .collect();
                                    for cid in chart_member_ids {
                                        if let Some(leaf) = self.panel_app.panel_grid.leaf_for_container_id(cid) {
                                            self.panel_app.leaf_color_tags.insert(leaf, new_color);
                                        }
                                    }
                                }
                            }
                            self.set_panel_synced(member, gid);
                        }
                    }
                    mlc_app_sidebar_content::state::SlotSourceMode::Untagged => {
                        // Spawn panel into its own 1-member auto-created group.
                        // S2.7.1e: read timeframe via branch_meta (B1).
                        let tf = spawn_active_cid
                            .and_then(|cid| self.timeframe_for_chart(cid))
                            .unwrap_or_else(mlc_core::state::Timeframe::h1);
                        let solo_gid = self.create_auto_bubble(
                            [0.0, 0.0, 0.0, 0.0],
                            symbol.clone(),
                            tf,
                        );
                        self.set_panel_auto(member, solo_gid);
                        // Seed exchange/account_type on the new group so apply_key
                        // picks them up next frame.
                        self.core.bubbles_mut().set_bubble_instrument(solo_gid, exchange_str.clone(), account_type_str.clone());
                    }
                }
            }
        }

        if let Some(item) = item_opt {
            use mlc_app_sidebar_content::state::AgentSpawnLayout;
            use uzor::panels::SplitKind;

            let layout = self.sidebar_state.slot_spawn_layout;
            let focused_in_slot = self.sidebar_state.focused_free_leaf
                .filter(|(si, _)| *si == idx)
                .map(|(_, lid)| lid);
            let tree_empty = self.sidebar_state.slot_dockings[idx]
                .inner()
                .tree()
                .leaves()
                .is_empty();

            let new_leaf = match (layout, focused_in_slot, tree_empty) {
                // Empty slot — first panel goes in directly regardless of layout choice.
                (_, _, true) => {
                    self.sidebar_state.slot_dockings[idx].inner_mut()
                        .tree_mut().add_leaf(item)
                }
                // Replace mode: kill the focused leaf, put new in its place.
                (AgentSpawnLayout::Replace, Some(focus), false) => {
                    let tree = self.sidebar_state.slot_dockings[idx]
                        .inner_mut().tree_mut();
                    tree.remove_leaf(focus);
                    tree.add_leaf(item)
                }
                // SplitH (side-by-side) / SplitV (stacked) with a focused leaf —
                // bisect that leaf using the chosen direction.  Use the binary
                // `split_leaf` (NOT `split_leaf_with_children`): the binary form
                // keeps the original leaf verbatim and creates ONE fresh sibling
                // carrying ONLY `new_panel`.  The compound form would clone all
                // original panels into every sibling, aliasing panel ids.
                (AgentSpawnLayout::SplitH, Some(focus), false)
                | (AgentSpawnLayout::SplitV, Some(focus), false) => {
                    let split_kind = if layout == AgentSpawnLayout::SplitH {
                        SplitKind::SplitRight
                    } else {
                        SplitKind::SplitBottom
                    };
                    let tree = self.sidebar_state.slot_dockings[idx]
                        .inner_mut().tree_mut();
                    match tree.split_leaf(focus, split_kind, item.clone()) {
                        Some(new_id) => new_id,
                        None => {
                            // Split fell back — append as a peer leaf at root.
                            self.sidebar_state.slot_dockings[idx].inner_mut()
                                .tree_mut().add_leaf(item)
                        }
                    }
                }
                // No focused leaf — just append (will sit next to existing peers).
                (_, None, false) => {
                    self.sidebar_state.slot_dockings[idx].inner_mut()
                        .tree_mut().add_leaf(item)
                }
            };

            // Move focus to the freshly spawned leaf.
            self.sidebar_state.focused_free_leaf = Some((idx, new_leaf));
            eprintln!("[ChartApp] slot:{}:spawn:{} — leaf {:?} layout={:?}", idx, kind_str, new_leaf, layout);
            self.sidebar_state.slot_spawn_dropdown = None;
            self.sidebar_data_dirty = true;
            self.autosave_snapshot();
        }
    }

    /// Which store an `OrderEntryState` lives in — the standalone
    /// `panels_store.order_entry` map, or hosted inside a
    /// `panels_store.trading_container[pid].order_entry` child. Both paths
    /// share the same submit flow (see [`Self::submit_order_entry_target`]).
    /// `pub(crate)` so `text_input::dispatch`'s per-instance `oe_field`
    /// Enter/Escape handlers can reach the target panel too.
    pub(crate) fn order_entry_mut(
        &mut self,
        target: OrderEntryTarget,
    ) -> Option<&mut mlc_app_panels::trading::trading::order_entry::OrderEntryState> {
        match target {
            OrderEntryTarget::Standalone(pid) => self.panels_store.order_entry.get_mut(&pid),
            OrderEntryTarget::Container(pid) => self.panels_store.trading_container.get_mut(&pid)
                .map(|c| &mut c.order_entry),
        }
    }

    /// Which store a `RiskCalculatorState` lives in — mirrors `order_entry_mut`.
    pub(crate) fn risk_calculator_mut(
        &mut self,
        target: RiskCalculatorTarget,
    ) -> Option<&mut mlc_app_panels::trading::trading::risk_calculator::RiskCalculatorState> {
        match target {
            RiskCalculatorTarget::Standalone(pid) => self.panels_store.risk_calculator.get_mut(&pid),
            RiskCalculatorTarget::Container(pid) => self.panels_store.trading_container.get_mut(&pid)
                .map(|c| &mut c.risk_calculator),
        }
    }

    /// Parse a `"slot:{idx}:leaf:{raw}"` slot_prefix (the exact string
    /// `mlc-app-sidebar-content::render::free_item_renderer` mints for every
    /// panel instance rendered in a free-slot sidebar) back into its
    /// `(slot_idx, leaf_id)` components.
    fn parse_slot_prefix(slot_prefix: &str) -> Option<(usize, u64)> {
        let rest = slot_prefix.strip_prefix("slot:")?;
        let (idx_str, leaf_str) = rest.split_once(":leaf:")?;
        Some((idx_str.parse().ok()?, leaf_str.parse().ok()?))
    }

    /// Resolve the `OrderEntryTarget` a per-instance `"{slot_prefix}:oe_field"`
    /// TextFieldStore id was minted for — reverses the slot_prefix lookup
    /// already used by the `"oe:"` click block above.
    pub(crate) fn oe_target_for_slot_prefix(&self, slot_prefix: &str) -> Option<OrderEntryTarget> {
        let (idx, raw) = Self::parse_slot_prefix(slot_prefix)?;
        let leaf_id = uzor::panels::LeafId(raw);
        let item_opt = self.sidebar_state.slot_dockings.get(idx)?
            .inner().tree().leaf(leaf_id).and_then(|l| l.active_panel().cloned());
        match item_opt {
            Some(mlc_core::state::Panel::OrderEntry(pid)) => Some(OrderEntryTarget::Standalone(pid)),
            Some(mlc_core::state::Panel::TradingContainer(pid)) => Some(OrderEntryTarget::Container(pid)),
            _ => None,
        }
    }

    /// Same as `oe_target_for_slot_prefix` but for RiskCalculator instances.
    pub(crate) fn rc_target_for_slot_prefix(&self, slot_prefix: &str) -> Option<RiskCalculatorTarget> {
        let (idx, raw) = Self::parse_slot_prefix(slot_prefix)?;
        let leaf_id = uzor::panels::LeafId(raw);
        let item_opt = self.sidebar_state.slot_dockings.get(idx)?
            .inner().tree().leaf(leaf_id).and_then(|l| l.active_panel().cloned());
        match item_opt {
            Some(mlc_core::state::Panel::RiskCalculator(pid)) => Some(RiskCalculatorTarget::Standalone(pid)),
            Some(mlc_core::state::Panel::TradingContainer(pid)) => Some(RiskCalculatorTarget::Container(pid)),
            _ => None,
        }
    }

    /// Build the per-instance shared TextFieldStore id for OrderEntry's 3
    /// numeric fields (Price/StopPrice/Quantity) — mirrors
    /// `PRIMITIVE_SETTINGS_FIELD`'s "one WidgetId covers all inline
    /// sub-fields" pattern, made per-instance via `slot_prefix` since
    /// multiple OrderEntry instances can render simultaneously (standalone
    /// map + per-container).
    fn oe_field_widget_id(slot_prefix: &str) -> WidgetId {
        WidgetId::from(format!("{}:oe_field", slot_prefix))
    }

    /// Same as `oe_field_widget_id` for RiskCalculator's 5 numeric fields.
    fn rc_field_widget_id(slot_prefix: &str) -> WidgetId {
        WidgetId::from(format!("{}:rc_field", slot_prefix))
    }

    /// Map an `"oe:*"` local_id to the `OrderEntryElement` it edits, if any.
    fn oe_field_for_local_id(local_id: &str) -> Option<mlc_app_panels::trading::trading::order_entry::OrderEntryElement> {
        use mlc_app_panels::trading::trading::order_entry::OrderEntryElement;
        match local_id {
            "oe:price" => Some(OrderEntryElement::PriceInput),
            "oe:stop_price" => Some(OrderEntryElement::StopPriceInput),
            "oe:qty" => Some(OrderEntryElement::QuantityInput),
            _ => None,
        }
    }

    /// Map a `"risk_calculator:*"` local_id to the `RiskCalcField` it edits, if any.
    fn rc_field_for_local_id(local_id: &str) -> Option<mlc_app_panels::trading::trading::risk_calculator::RiskCalcField> {
        use mlc_app_panels::trading::trading::risk_calculator::RiskCalcField;
        match local_id {
            "risk_calculator:account_size" => Some(RiskCalcField::AccountSize),
            "risk_calculator:risk_percent" => Some(RiskCalcField::RiskPercent),
            "risk_calculator:entry_price" => Some(RiskCalcField::EntryPrice),
            "risk_calculator:stop_loss" => Some(RiskCalcField::StopLoss),
            "risk_calculator:take_profit" => Some(RiskCalcField::TakeProfit),
            _ => None,
        }
    }

    /// Begin editing one of OrderEntry's numeric fields: auto-commits
    /// whatever was previously active on this SAME instance (mirrors the
    /// pre-migration `OrderEntryState::commit_editing()` call at the top of
    /// each field-click arm), then seeds the shared per-instance
    /// `TextFieldStore` field with the current committed value (plain
    /// parseable form, NOT `format_quantity()`'s K-suffixed display form)
    /// and focuses + select-alls it (numeric-field precedent — see
    /// `start_prim_field_editing`'s `select_all` callers).
    ///
    /// No-ops when `field` is ALREADY `active_field` — clicking inside a
    /// field that is already being edited must reposition the caret /
    /// extend a drag-selection, NOT restart editing. That reposition is
    /// handled generically by `TextFieldStore::on_drag_start`/`on_drag_move`
    /// on the mousedown that precedes this click (wired unconditionally in
    /// `input::mod::on_drag_start`/`on_drag_move` for every registered +
    /// geometry-fresh field — see `order_entry::draw_oe_field_row`'s
    /// `update_field` call); re-seeding here on every subsequent click would
    /// silently discard it (owner-reported regression: caret always
    /// snapping to text end, selection invisible).
    fn start_oe_field_edit(
        &mut self,
        target: OrderEntryTarget,
        slot_prefix: &str,
        field: mlc_app_panels::trading::trading::order_entry::OrderEntryElement,
    ) {
        use mlc_app_panels::trading::trading::order_entry::OrderEntryElement;
        if self.order_entry_mut(target).is_some_and(|s| s.active_field == Some(field)) {
            return;
        }
        let wid = Self::oe_field_widget_id(slot_prefix);
        self.commit_oe_field(target, &wid);
        let Some(state) = self.order_entry_mut(target) else { return };
        let seed = match field {
            OrderEntryElement::PriceInput => state.price.map(|p| format!("{}", p)).unwrap_or_default(),
            OrderEntryElement::StopPriceInput => state.stop_price.map(|p| format!("{}", p)).unwrap_or_default(),
            OrderEntryElement::QuantityInput => if state.quantity > 0.0 { format!("{}", state.quantity) } else { String::new() },
            _ => return,
        };
        state.active_field = Some(field);
        let mut coord = self.input_coordinator.borrow_mut();
        coord.text_fields_mut().register(wid.clone(), TextFieldConfig::text()
            .with_filter(|c| c.is_ascii_digit() || c == '.'));
        coord.text_fields_mut().set_text(&wid, &seed);
        coord.text_fields_mut().focus(wid.clone());
        if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
            fs.selection_start = Some(0);
        }
    }

    /// Commit whatever OrderEntry numeric field is currently active on
    /// `target`: parses the shared field's live text as `f64` and writes it
    /// back (Price/StopPrice/Quantity per `active_field`), then blurs.
    /// Parse failure silently keeps the old value (same contract as the
    /// primitive-settings numeric sub-fields — see
    /// `commit_prim_editing_text`). `pub(crate)` so
    /// `text_input::dispatch::text_action_oe_field` (Enter) and
    /// `submit_order_entry_target` (blur-before-submit) can reuse it.
    pub(crate) fn commit_oe_field(&mut self, target: OrderEntryTarget, wid: &WidgetId) {
        use mlc_app_panels::trading::trading::order_entry::OrderEntryElement;
        let Some(state) = self.order_entry_mut(target) else { return };
        let Some(field) = state.active_field.take() else { return };
        let text = self.input_coordinator.borrow().text_fields().text(wid).to_string();
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        if let Ok(value) = text.trim().parse::<f64>() {
            if let Some(state) = self.order_entry_mut(target) {
                match field {
                    OrderEntryElement::PriceInput => state.price = Some(value),
                    OrderEntryElement::StopPriceInput => state.stop_price = Some(value),
                    OrderEntryElement::QuantityInput => state.quantity = value,
                    _ => {}
                }
                state.validate();
            }
        }
    }

    /// RiskCalculator analogue of `start_oe_field_edit` — same no-op-when-
    /// already-active contract (see that method's doc).
    fn start_rc_field_edit(
        &mut self,
        target: RiskCalculatorTarget,
        slot_prefix: &str,
        field: mlc_app_panels::trading::trading::risk_calculator::RiskCalcField,
    ) {
        use mlc_app_panels::trading::trading::risk_calculator::RiskCalcField;
        if self.risk_calculator_mut(target).is_some_and(|s| s.active_field == Some(field)) {
            return;
        }
        let wid = Self::rc_field_widget_id(slot_prefix);
        self.commit_rc_field(target, &wid);
        let Some(state) = self.risk_calculator_mut(target) else { return };
        let seed = match field {
            RiskCalcField::AccountSize => if state.account_size > 0.0 { format!("{}", state.account_size) } else { String::new() },
            RiskCalcField::RiskPercent => if state.risk_percent > 0.0 { format!("{}", state.risk_percent) } else { String::new() },
            RiskCalcField::EntryPrice => if state.entry_price > 0.0 { format!("{}", state.entry_price) } else { String::new() },
            RiskCalcField::StopLoss => if state.stop_loss_price > 0.0 { format!("{}", state.stop_loss_price) } else { String::new() },
            RiskCalcField::TakeProfit => state.take_profit_price.map(|p| format!("{}", p)).unwrap_or_default(),
        };
        state.active_field = Some(field);
        let mut coord = self.input_coordinator.borrow_mut();
        coord.text_fields_mut().register(wid.clone(), TextFieldConfig::text()
            .with_filter(|c| c.is_ascii_digit() || c == '.'));
        coord.text_fields_mut().set_text(&wid, &seed);
        coord.text_fields_mut().focus(wid.clone());
        if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
            fs.selection_start = Some(0);
        }
    }

    /// RiskCalculator analogue of `commit_oe_field`.
    pub(crate) fn commit_rc_field(&mut self, target: RiskCalculatorTarget, wid: &WidgetId) {
        use mlc_app_panels::trading::trading::risk_calculator::RiskCalcField;
        let Some(state) = self.risk_calculator_mut(target) else { return };
        let Some(field) = state.active_field.take() else { return };
        let text = self.input_coordinator.borrow().text_fields().text(wid).to_string();
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        if let Ok(value) = text.trim().parse::<f64>() {
            if let Some(state) = self.risk_calculator_mut(target) {
                match field {
                    RiskCalcField::AccountSize => {
                        state.account_size = value;
                        state.account_size_overridden = true;
                    }
                    RiskCalcField::RiskPercent => state.risk_percent = value,
                    RiskCalcField::EntryPrice => {
                        state.entry_price = value;
                        state.entry_price_touched = true;
                    }
                    RiskCalcField::StopLoss => state.stop_loss_price = value,
                    RiskCalcField::TakeProfit => state.take_profit_price = Some(value),
                }
                state.calculate();
            }
        }
    }

    /// Validate and submit a standalone Order Entry panel's current order.
    fn submit_order_entry(&mut self, pid: mlc_core::state::PanelId, slot_prefix: &str) {
        self.submit_order_entry_target(OrderEntryTarget::Standalone(pid), slot_prefix);
    }

    /// Validate and submit a TradingContainer-hosted Order Entry's current order.
    fn submit_order_entry_in_container(&mut self, pid: mlc_core::state::PanelId, slot_prefix: &str) {
        self.submit_order_entry_target(OrderEntryTarget::Container(pid), slot_prefix);
    }

    /// Validate and submit the Order Entry panel's current order.
    ///
    /// Called right after `OrderEntryState::handle_click` consumes an
    /// `"oe:submit"` click (which unconditionally flips `submitting = true`
    /// — this is where validation + the actual `TradingMut::place_order`
    /// call happen). Builds a dig3 `OrderRequest` from the panel's own
    /// (exchange, account_type) binding and current field values. Shared by
    /// both the standalone OrderEntry panel and a TradingContainer-hosted one.
    ///
    /// Every branch logs via `sglog!` (reaches the browser DevTools console
    /// on wasm, `eprintln!` on native) — a plain `eprintln!` here is
    /// invisible in the browser, which made every failure of this path
    /// silent during the container e2e bug hunt.
    fn submit_order_entry_target(&mut self, target: OrderEntryTarget, slot_prefix: &str) {
        use mlc_app_panels::trading::trading::order_entry::{
            OrderSide as PanelSide, OrderType as PanelOrderType, TimeInForce as PanelTif,
        };

        // Commit any in-flight numeric edit (price/stop/qty) before
        // validating — the submit button itself does not commit (only
        // switching to a different field or pressing Enter does), so a
        // click straight from a still-being-typed field must not submit a
        // stale value.
        self.commit_oe_field(target, &Self::oe_field_widget_id(slot_prefix));

        let Some(state) = self.order_entry_mut(target) else { return };
        state.validate();
        if !state.is_valid() {
            sglog!("[trading] submit rejected: {:?}", state.errors);
            state.submitting = false;
            return;
        }

        let exchange_str = state.exchange.clone();
        let account_type_str = state.account_type.clone();
        let raw_symbol = state.symbol.clone();
        let side = state.side;
        let order_type = state.order_type;
        let price = state.price;
        let stop_price = state.stop_price;
        let quantity = state.quantity;
        let time_in_force = state.time_in_force;

        sglog!(
            "[trading] submit {:?} {:?} {} qty={} exchange={} account_type={}",
            side, order_type, raw_symbol, quantity, exchange_str, account_type_str
        );

        let Some(mut tm) = self.core.trading_mut() else {
            sglog!("[trading] submit failed: trading unavailable");
            if let Some(state) = self.order_entry_mut(target) {
                state.submitting = false;
                state.errors.clear();
                state.errors.push("trading unavailable".to_string());
            }
            return;
        };

        let eid = digdigdig3::ExchangeId::from_str(&exchange_str).unwrap_or(self.active_exchange);
        let at = crate::account_type_from_label(&account_type_str);
        let symbol = digdigdig3::core::utils::SymbolNormalizer::from_exchange(eid, &raw_symbol, at)
            .unwrap_or_else(|_| digdigdig3::Symbol::with_raw("", "", raw_symbol.clone()));

        let dig3_side = match side {
            PanelSide::Buy => digdigdig3::OrderSide::Buy,
            PanelSide::Sell => digdigdig3::OrderSide::Sell,
        };
        let dig3_order_type = match order_type {
            PanelOrderType::Market => digdigdig3::OrderType::Market,
            PanelOrderType::Limit => digdigdig3::OrderType::Limit { price: price.unwrap_or(0.0) },
            PanelOrderType::StopMarket => digdigdig3::OrderType::StopMarket { stop_price: stop_price.unwrap_or(0.0) },
            PanelOrderType::StopLimit => digdigdig3::OrderType::StopLimit {
                stop_price: stop_price.unwrap_or(0.0),
                limit_price: price.unwrap_or(0.0),
            },
        };
        let dig3_tif = match time_in_force {
            PanelTif::GTC => mlc_backend_trading_engine::TimeInForce::Gtc,
            PanelTif::IOC => mlc_backend_trading_engine::TimeInForce::Ioc,
            PanelTif::FOK => mlc_backend_trading_engine::TimeInForce::Fok,
        };

        let req = mlc_backend_trading_engine::OrderRequest {
            symbol,
            side: dig3_side,
            order_type: dig3_order_type,
            quantity,
            time_in_force: dig3_tif,
            account_type: at,
            client_order_id: None,
            reduce_only: false,
        };

        let result = tm.place_order(eid, at, req);
        drop(tm);

        match &result {
            Ok(()) => {
                sglog!("[trading] submit OK {:?} {} qty={} exchange={} account_type={}", side, raw_symbol, quantity, exchange_str, account_type_str);
            }
            Err(e) => {
                sglog!("[trading] submit FAILED: {}", e);
            }
        }

        if let Some(state) = self.order_entry_mut(target) {
            match result {
                Ok(()) => {
                    state.submitting = false;
                    state.errors.clear();
                }
                Err(e) => {
                    state.submitting = false;
                    state.errors.clear();
                    state.errors.push(e.to_string());
                }
            }
        }
    }

    /// Round a quantity to a sane precision (6 decimals) — used when writing
    /// a computed value (risk-calc position size) into another panel's
    /// field, so it never carries raw floating-point noise.
    fn round_qty(q: f64) -> f64 {
        (q * 1_000_000.0).round() / 1_000_000.0
    }

    /// Issue the actual `TradingMut::cancel_order` call(s) for whatever
    /// `OrdersLogState::pending_cancel`/`pending_cancel_all` a container's
    /// "orders:cancel*" click just set (see `OrdersLogState` module doc for
    /// why this panel can't call `TradingMut` itself). Cancel-All loops a
    /// single-order cancel over every currently open order — the paper
    /// engine only ever implements `CancelScope::Single` (see
    /// `TradingManager::cancel_order`), so there is no batched "cancel all"
    /// to route to.
    fn process_orders_log_pending_cancels(&mut self, pid: mlc_core::state::PanelId) {
        let Some(state) = self.panels_store.trading_container.get_mut(&pid) else { return };
        let cancel_all = state.orders_log.pending_cancel_all;
        state.orders_log.pending_cancel_all = false;
        let single = state.orders_log.pending_cancel.take();

        let order_ids: Vec<String> = if cancel_all {
            state.orders_log.open_orders.iter().map(|o| o.id.clone()).collect()
        } else if let Some(id) = single {
            vec![id]
        } else {
            Vec::new()
        };
        if order_ids.is_empty() {
            return;
        }

        let exchange_str = state.exchange.clone();
        let account_type_str = state.account_type.clone();
        let raw_symbol = state.symbol.clone();

        let eid = digdigdig3::ExchangeId::from_str(&exchange_str).unwrap_or(self.active_exchange);
        let at = crate::account_type_from_label(&account_type_str);
        let symbol = digdigdig3::core::utils::SymbolNormalizer::from_exchange(eid, &raw_symbol, at)
            .unwrap_or_else(|_| digdigdig3::Symbol::with_raw("", "", raw_symbol.clone()));

        let Some(mut tm) = self.core.trading_mut() else {
            sglog!("[trading] cancel failed: trading unavailable");
            return;
        };
        for order_id in &order_ids {
            let req = mlc_backend_trading_engine::CancelRequest {
                scope: mlc_backend_trading_engine::CancelScope::Single { order_id: order_id.clone() },
                symbol: Some(symbol.clone()),
                account_type: at,
            };
            let result = tm.cancel_order(eid, at, req);
            match &result {
                Ok(()) => sglog!("[trading] cancel OK {} exchange={} account_type={}", order_id, exchange_str, account_type_str),
                Err(e) => sglog!("[trading] cancel FAILED {}: {}", order_id, e),
            }
        }
    }

    /// Apply a container's RiskCalculator computed position size (and, for
    /// an empty Limit price, its entry price) into the sibling OrderEntry —
    /// the "risk_calculator:apply" click's actual effect (the panel itself
    /// only sets `apply_requested`, see `RiskCalculatorState` module doc).
    fn apply_risk_calculator_size_in_container(&mut self, pid: mlc_core::state::PanelId) {
        let Some(state) = self.panels_store.trading_container.get_mut(&pid) else { return };
        if !state.risk_calculator.apply_requested {
            return;
        }
        state.risk_calculator.apply_requested = false;
        if !state.risk_calculator.inputs_ready() || state.risk_calculator.position_size <= 0.0 {
            return;
        }

        let position_size = Self::round_qty(state.risk_calculator.position_size);
        let entry_price = state.risk_calculator.entry_price;

        state.order_entry.quantity = position_size;
        if matches!(state.order_entry.order_type, mlc_app_panels::trading::trading::order_entry::OrderType::Limit)
            && state.order_entry.price.is_none()
            && entry_price > 0.0
        {
            state.order_entry.price = Some(entry_price);
        }
        state.order_entry.recalc_estimated();
        state.order_entry.validate();
        sglog!("[trading] risk-calc apply size={:.6} entry={:.4}", position_size, entry_price);
    }

    /// Resolve a `timeframe_selector` dropdown item id (stripped of its
    /// `gran_star:`/`gran_edit:`/`gran_del:` action prefix) back to a
    /// [`mlc_core::chart_type::GranularityTarget`] — user-granularity-
    /// templates-inner arc, кусок 2, extended per-family scoping arc
    /// 2026-07-16 for footprint bucket intervals.
    ///
    /// `{GRANULARITY_INTERVAL_ID_PREFIX}{name}` (`ivl_{name}`) ids ALWAYS
    /// map to `Interval` — the dropdown builder mints this prefix
    /// exclusively for footprint's own rows (see
    /// `mlc-app-layout::panel_app::{prepend_interval_starred_items,
    /// append_interval_customs_and_add_row}`), so no ResolutionKind check is
    /// needed to disambiguate it from a kline `tf_` row.
    ///
    /// `tf_{name}` ids are ambiguous on their own — footprint's BUILTIN
    /// whitelist rows (`FOOTPRINT_TF_WHITELIST`) also use the `tf_` prefix
    /// (same TF-name grammar, `bubble.timeframe` mechanism) — so which
    /// family a plain `tf_{name}` id belongs to is decided by the ACTIVE
    /// chart type's `ResolutionKind` at click time: `Interval` resolution
    /// (footprint) → `GranularityTarget::Interval`, everything else →
    /// `GranularityTarget::Tf`. This is the SAME rule the dropdown/quick-pick
    /// builders use to decide which favorites bucket to read from — star/
    /// edit/delete/pin actions on a footprint TF row must land in
    /// `interval_tfs`/`starred_intervals`, never `time_tfs`/`starred_tfs`
    /// (owner: "остался запинен 1m на футпринте — тупо").
    ///
    /// `reso_param:{value}` ids map to `Param`, scoped to the active
    /// window's chart_type (a range_bar star action must never resolve
    /// against the volume_bar chart_type). `None` when the id shape is
    /// unrecognized or there is no active window to scope a `Param` target
    /// to.
    pub(crate) fn resolve_granularity_target(&self, target_id: &str) -> Option<mlc_core::chart_type::GranularityTarget> {
        if let Some(name) = target_id.strip_prefix(mlc_core::chart_type::GRANULARITY_INTERVAL_ID_PREFIX) {
            return Some(mlc_core::chart_type::GranularityTarget::Interval { name: name.to_string() });
        }
        if let Some(name) = target_id.strip_prefix("tf_") {
            let is_interval_kind = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.chart_type_for_chart(cid))
                .and_then(mlc_core::chart_type::by_id)
                .is_some_and(|def| matches!(def.resolution, mlc_core::chart_type::ResolutionKind::Interval));
            return Some(if is_interval_kind {
                mlc_core::chart_type::GranularityTarget::Interval { name: name.to_string() }
            } else {
                mlc_core::chart_type::GranularityTarget::Tf { name: name.to_string() }
            });
        }
        if let Some(value_str) = target_id.strip_prefix(mlc_core::chart_type::RESOLUTION_PARAM_ITEM_PREFIX) {
            let value: f64 = value_str.parse().ok()?;
            let chart_type = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.chart_type_for_chart(cid))?;
            return Some(mlc_core::chart_type::GranularityTarget::Param { chart_type: chart_type.to_string(), value });
        }
        None
    }

    /// Resolve a `timeframe_selector` dropdown ROW WIDGET id (the raw
    /// `"dropdown:timeframe_selector:{row_id}"` hit-zone id, BEFORE any
    /// trailing-icon-action prefix stripping) into a `(family, section,
    /// index)` triple for drag-to-reorder — owner spec 2026-07-16 items 2+3.
    ///
    /// `None` when `row_id` is not a reorderable row body: the "+ Add" rows,
    /// the `gran_pin:toggle` row, ATR(14), or any id the row-id list built
    /// by `granularity_section_row_ids` does not contain (trailing-icon
    /// action ids like `gran_star:…`/`gran_edit:…`/`gran_del:…` never reach
    /// here — see the caller in `on_drag_start`, which checks the row's OWN
    /// id, not an action sub-id, so a trailing-icon click is never
    /// misclassified as a reorder-handle press).
    pub(crate) fn resolve_granularity_reorder_row(
        &self,
        row_id: &str,
    ) -> Option<(mlc_core::chart_type::GranularityFamily, mlc_core::chart_type::GranularitySection, usize)> {
        use mlc_core::chart_type::{GranularityFamily, GranularitySection};

        let (section, bare_id) = match row_id.strip_prefix(mlc_core::chart_type::GRANULARITY_FAV_ID_PREFIX) {
            Some(stripped) => (GranularitySection::Favorites, stripped),
            None => (GranularitySection::Customs, row_id),
        };

        let resolution = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.chart_type_for_chart(cid))
            .and_then(mlc_core::chart_type::by_id)
            .map(|def| def.resolution);
        let family = if bare_id.starts_with(mlc_core::chart_type::GRANULARITY_INTERVAL_ID_PREFIX) {
            GranularityFamily::Interval
        } else if let Some(_value_str) = bare_id.strip_prefix(mlc_core::chart_type::RESOLUTION_PARAM_ITEM_PREFIX) {
            let chart_type = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.chart_type_for_chart(cid))?;
            GranularityFamily::Param { chart_type: chart_type.to_string() }
        } else if bare_id.starts_with("tf_") {
            let is_interval_kind = matches!(resolution, Some(mlc_core::chart_type::ResolutionKind::Interval));
            if is_interval_kind { GranularityFamily::Interval } else { GranularityFamily::Tf }
        } else {
            return None;
        };

        let user = self.core.user();
        let favorites = &user.profile().domain.granularity_favorites;
        let row_ids = mlc_app_layout::panel_app::granularity_section_row_ids(&family, section, favorites);
        let index = row_ids.iter().position(|id| id == row_id)?;
        Some((family, section, index))
    }

    /// Begin inline editing of an existing CUSTOM granularity value (pencil
    /// icon click) — user-granularity-templates-inner arc, кусок 2. Sets
    /// `granularity_edit` to the matching `EditTf`/`EditParam` variant and
    /// prefills+focuses the dedicated field with the current value's text
    /// (mirrors the "+ Add" row's empty-prefill focus sequence).
    pub(crate) fn begin_granularity_edit(&mut self, target: mlc_core::chart_type::GranularityTarget) {
        match target {
            mlc_core::chart_type::GranularityTarget::Tf { name } => {
                self.granularity_edit = Some(crate::GranularityEdit {
                    kind: crate::GranularityEditKind::EditTf { original: name.clone() },
                });
                let field_id = uzor::WidgetId::from(crate::text_input::TF_FAVORITE_INPUT);
                let mut coord = self.input_coordinator.borrow_mut();
                let tf = coord.text_fields_mut();
                tf.set_text(&field_id, &name);
                tf.begin_edit(&field_id);
                tf.focus(crate::text_input::TF_FAVORITE_INPUT);
            }
            mlc_core::chart_type::GranularityTarget::Param { value, .. } => {
                self.granularity_edit = Some(crate::GranularityEdit {
                    kind: crate::GranularityEditKind::EditParam { original: value },
                });
                let field_id = uzor::WidgetId::from(crate::text_input::PARAM_FAVORITE_INPUT);
                let mut coord = self.input_coordinator.borrow_mut();
                let tf = coord.text_fields_mut();
                tf.set_text(&field_id, &format_trimmed_param_value(value));
                tf.begin_edit(&field_id);
                tf.focus(crate::text_input::PARAM_FAVORITE_INPUT);
            }
            mlc_core::chart_type::GranularityTarget::Interval { name } => {
                self.granularity_edit = Some(crate::GranularityEdit {
                    kind: crate::GranularityEditKind::EditInterval { original: name.clone() },
                });
                let field_id = uzor::WidgetId::from(crate::text_input::TF_FAVORITE_INPUT);
                let mut coord = self.input_coordinator.borrow_mut();
                let tf = coord.text_fields_mut();
                tf.set_text(&field_id, &name);
                tf.begin_edit(&field_id);
                tf.focus(crate::text_input::TF_FAVORITE_INPUT);
            }
        }
    }

    /// Close the live symbol-search inline editor and clear its transient
    /// state (symbol-search-compare-inner arc, Wave 2, кусок 4). Shared by
    /// every exit path — row apply, "Advanced search…", Escape,
    /// background-click — and by BOTH surfaces (trading-header-symbol-edit
    /// arc, 2026-07-25 generalized this past the topbar-only
    /// `symbol_selector` dropdown): clearing `toolbar_state.open_dropdown_id`
    /// is a no-op when the live edit targeted a `TradingPanel` (it was never
    /// set for that target in the first place).
    pub(crate) fn close_symbol_quick_dropdown(&mut self) {
        self.panel_app.toolbar_state.open_dropdown_id = None;
        self.panel_app.toolbar_state.open_dropdown_position = None;
        self.trading_header_tb.open_dropdown_id = None;
        self.symbol_quick_edit = None;
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
    }

    /// Open the `symbol_selector` inline fast-search dropdown: set the
    /// toolbar dropdown-open state, seed the RAM edit state and focus the
    /// `SYMBOL_QUICK_SEARCH` field. Shared by the `OpenSymbolSearch`
    /// handler's quick branch and the instant quick-toggle transition
    /// (owner defect 2026-07-16: flipping the toggle must switch
    /// dropdown⇄modal immediately, not on next open).
    ///
    /// Mode 3 "inline" (Wave 4 rework — owner spec 2026-07-16): the editor
    /// starts SEEDED with the active symbol (`symbol_quick_seed_text()`),
    /// cursor at the end, so the button's text IS the editable text from
    /// frame one and the dropdown's first-frame results are the query for
    /// that symbol across venues. Mode 2 (dropdown-hosted input row) keeps
    /// the historical empty-start behaviour — `symbol_quick_seed_text()`
    /// returns `""` there.
    pub(crate) fn open_symbol_quick_dropdown(&mut self) {
        self.panel_app.toolbar_state.open_dropdown_id = Some("symbol_selector".to_string());
        self.panel_app.toolbar_state.open_dropdown_position = None;
        let seed = self.symbol_quick_seed_text();
        let cursor = seed.chars().count();
        self.symbol_quick_edit = Some(crate::SymbolQuickEdit {
            target: crate::SymbolQuickEditTarget::TopbarChart,
            text: seed.clone(),
            cursor,
            selection: None,
        });
        let q_id = uzor::WidgetId::from(crate::text_input::SYMBOL_QUICK_SEARCH);
        let mut ic = self.input_coordinator.borrow_mut();
        let tf = ic.text_fields_mut();
        tf.set_text(&q_id, &seed);
        tf.begin_edit(&q_id);
        tf.focus(crate::text_input::SYMBOL_QUICK_SEARCH);
    }

    /// Open the full-screen symbol-search MODAL directly, bypassing the
    /// `symbol_quick_search` branch in the `OpenSymbolSearch` handler —
    /// used by "Advanced search…" inside the quick dropdown (owner defect
    /// 2026-07-16: re-dispatching `OpenSymbolSearch` with quick ON looped
    /// straight back into the dropdown, making the row look dead) and by
    /// the instant quick-toggle transition.
    pub(crate) fn open_symbol_search_modal_direct(&mut self) {
        self.modal_state.open(mlc_core::state::modal_state::OpenModal::SymbolSearch);
        self.modal_state.symbol_search_results = crate::ChartApp::build_demo_symbol_results(
            "",
            self.core.watchlists().find(self.watchlist_ui.active_list_id),
            self.core.exchange_symbols().as_map(),
        );
        let sym_id = uzor::WidgetId::from(crate::text_input::SYMBOL_SEARCH);
        let mut ic = self.input_coordinator.borrow_mut();
        let tf = ic.text_fields_mut();
        tf.clear(&sym_id);
        tf.begin_edit(&sym_id);
        tf.focus(crate::text_input::SYMBOL_SEARCH);
    }

    /// Flip `ProfileUiSettings::symbol_quick_search` and autosave — shared by
    /// the `symbol_selector` inline dropdown's toggle row
    /// (`sym_quick:toggle_quick`) and the full-screen search modal header's
    /// toggle (`modal_search:toggle_quick`, symbol-search-compare-inner arc,
    /// Wave 3 — owner spec 2026-07-16). The flip takes effect IMMEDIATELY
    /// (owner defect 2026-07-16): quick turned OFF while the dropdown is
    /// open → dropdown closes, modal opens in its place; quick turned ON
    /// while the symbol-search modal is open → modal closes, dropdown opens.
    pub(crate) fn toggle_symbol_quick_search(&mut self) {
        let now_on = {
            let mut user_mut = self.core.user_mut();
            let ui = &mut user_mut.profile_mut().ui;
            ui.symbol_quick_search = !ui.symbol_quick_search;
            ui.symbol_quick_search
        };
        self.autosave_snapshot();
        if !now_on && self.symbol_quick_edit.is_some() {
            self.close_symbol_quick_dropdown();
            self.open_symbol_search_modal_direct();
        } else if now_on
            && self.modal_state.current == mlc_core::state::modal_state::OpenModal::SymbolSearch
        {
            self.modal_state.close();
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            self.open_symbol_quick_dropdown();
        }
    }

    /// Flip `ProfileUiSettings::split_symbol_compare` and autosave — shared
    /// by the `symbol_selector` inline dropdown's toggle row
    /// (`sym_quick:toggle_split`) and the full-screen search modal header's
    /// toggle (`modal_search:toggle_split`, symbol-search-compare-inner arc,
    /// Wave 3 — owner spec 2026-07-16).
    pub(crate) fn toggle_split_symbol_compare(&mut self) {
        let mut user_mut = self.core.user_mut();
        let ui = &mut user_mut.profile_mut().ui;
        ui.split_symbol_compare = !ui.split_symbol_compare;
        drop(user_mut);
        self.autosave_snapshot();
    }

    /// Flip `ProfileUiSettings::trading_panel_topbar_takeover` and autosave —
    /// the User Settings modal's General-tab checkbox (workspace-panel arc,
    /// Stage 4 — owner spec 2026-07-25). Immediately updates the render-layer
    /// mirror on `user_settings_state` so the checkbox reflects the flip the
    /// same frame (mirrors `"diagnostics_toggle"`'s dual-write).
    pub(crate) fn toggle_trading_panel_topbar_takeover(&mut self) {
        let now_on = {
            let mut user_mut = self.core.user_mut();
            let ui = &mut user_mut.profile_mut().ui;
            ui.trading_panel_topbar_takeover = !ui.trading_panel_topbar_takeover;
            ui.trading_panel_topbar_takeover
        };
        self.panel_app.user_settings_state.trading_panel_topbar_takeover = now_on;
        self.autosave_snapshot();
        eprintln!("[ChartApp] trading_panel_topbar_takeover = {}", now_on);
    }

    /// Route a click inside the `symbol_selector` inline fast-search
    /// dropdown (symbol-search-compare-inner arc, Wave 2, кусок 4 — owner
    /// spec 2026-07-16). `item_id` is the raw row id AFTER the
    /// `"dropdown:symbol_selector:"` prefix has been stripped by the
    /// caller — same shape `handle_dropdown_select_with_chart` would
    /// otherwise receive.
    pub(crate) fn handle_symbol_quick_click(&mut self, item_id: &str) {
        if item_id == "sym_quick:toggle_quick" {
            self.toggle_symbol_quick_search();
            return;
        }
        if item_id == "sym_quick:toggle_split" {
            self.toggle_split_symbol_compare();
            return;
        }
        if item_id == "sym_quick:open_modal" {
            // DIRECT modal open — re-dispatching `OpenSymbolSearch` here
            // loops back into the quick branch while quick is ON and
            // reopens the dropdown (owner defect 2026-07-16, "Advanced
            // search does nothing").
            self.close_symbol_quick_dropdown();
            self.open_symbol_search_modal_direct();
            return;
        }
        if let Some(key) = item_id.strip_prefix("sym_quick_star:") {
            let (symbol, exchange, account_type) = parse_symbol_composite_key(key);
            // Multi-DOM roster picker: the trailing icon is the DOM toggle,
            // not the watchlist star — same action as clicking the row.
            match self.symbol_quick_edit.as_ref().map(|e| e.target) {
                Some(crate::SymbolQuickEditTarget::MultiDomRoster(pid)) => {
                    self.toggle_multi_dom_roster(pid, &symbol, &exchange, &account_type);
                    return;
                }
                Some(crate::SymbolQuickEditTarget::MultiTapeRoster(pid)) => {
                    self.toggle_multi_tape_roster(pid, &symbol, &exchange, &account_type);
                    return;
                }
                Some(crate::SymbolQuickEditTarget::SectionPanel(pid)) => {
                    // Section-picker icon = add the venue to THIS panel in
                    // parallel (owner order 2026-07-31) — the watchlist is
                    // «куда-то в другое место».
                    if self.add_parallel_section_venue(pid, &symbol, &exchange, &account_type) {
                        return;
                    }
                }
                _ => {}
            }
            self.watchlist_actions.push(crate::WatchlistAction::Toggle { symbol, exchange, account_type });
            self.watchlists_dirty = true;
            return;
        }
        if let Some(key) = item_id.strip_prefix("sym_quick_cmp:") {
            let (symbol, exchange, account_type) = parse_symbol_composite_key(key);
            self.add_compare_symbol(&symbol, &exchange, &account_type);
            self.close_symbol_quick_dropdown();
            return;
        }
        if let Some(key) = item_id.strip_prefix(mlc_app_layout::panel_app::SYMBOL_QUICK_ITEM_PREFIX) {
            let (symbol, exchange_id, account_type) = parse_symbol_composite_key(key);
            // Target-aware commit: the quicksearch edit may be driven by the
            // trading container's embedded header toolbar, not the topbar.
            match self.symbol_quick_edit.as_ref().map(|e| e.target) {
                Some(crate::SymbolQuickEditTarget::TradingPanel(pid)) => {
                    self.commit_symbol_search_to_trading_panel(pid, &symbol, &exchange_id, &account_type);
                    self.close_symbol_quick_dropdown();
                    return;
                }
                Some(crate::SymbolQuickEditTarget::SectionPanel(pid)) => {
                    self.commit_symbol_search_to_section_panel(pid, &symbol, &exchange_id, &account_type);
                    self.close_symbol_quick_dropdown();
                    return;
                }
                Some(crate::SymbolQuickEditTarget::MultiDomRoster(pid)) => {
                    // Roster editing is multi-click — the dropdown stays
                    // open so several columns can be added/removed in one
                    // session; click-away / Escape closes it.
                    self.toggle_multi_dom_roster(pid, &symbol, &exchange_id, &account_type);
                    return;
                }
                Some(crate::SymbolQuickEditTarget::MultiTapeRoster(pid)) => {
                    // Same multi-click contract as the Multi-DOM roster.
                    self.toggle_multi_tape_roster(pid, &symbol, &exchange_id, &account_type);
                    return;
                }
                _ => {}
            }
            let resolved_exchange = self.core.exchange_symbols()
                .find_by_label(&exchange_id)
                .unwrap_or(self.active_exchange);
            if let Some(chart_id) = self.panel_app.panel_grid.active_container_id() {
                self.apply_symbol(chart_id, &symbol, resolved_exchange.as_str(), &account_type);
            }
            self.close_symbol_quick_dropdown();
            return;
        }
    }

}

/// Parse a `"SYMBOL:exchange_id:account_type"` composite key (symbols may
/// themselves contain `':'`, hence splitting from the right twice) — shared
/// by [`ChartApp::handle_symbol_quick_click`] (topbar dropdown rows) and the
/// trading-header inline editor's live-search popup rows
/// (`trading_symbol_popup:{pid}:item:{key}`, trading-header-symbol-edit arc,
/// 2026-07-25). Same parse shape `handle_search_modal_click`'s "star:"/
/// "item:" arms use (modals.rs).
fn parse_symbol_composite_key(key: &str) -> (String, String, String) {
    if let Some(at_pos) = key.rfind(':') {
        let at_part = &key[at_pos + 1..];
        let remainder = &key[..at_pos];
        if let Some(ex_pos) = remainder.rfind(':') {
            return (remainder[..ex_pos].to_string(), remainder[ex_pos + 1..].to_string(), at_part.to_string());
        }
        return (remainder.to_string(), String::new(), at_part.to_string());
    }
    (key.to_string(), String::new(), "S".to_string())
}

/// Trim a trailing `.0` (or more generally any trailing fractional zeros)
/// from an `f64`'s decimal rendering — prefill text for
/// [`ChartApp::begin_granularity_edit`]'s `EditParam` field
/// (`PARAM_FAVORITE_INPUT` only accepts digits+dot, so the prefill must
/// already be plain decimal text, no scientific notation).
fn format_trimmed_param_value(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        let s = format!("{:.2}", v);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
