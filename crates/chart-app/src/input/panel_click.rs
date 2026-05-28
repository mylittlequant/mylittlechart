//! Mega-router for click events: dispatches by widget_id to per-area
//! handlers (sidebar, watchlist, overlay, profile-manager, tags-tabs,
//! slot panels, agent leaves, etc.).

use crate::ChartApp;
use uzor::WidgetId;

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
                        zengeld_chart::ui::modal_settings::ProfileManagerPage::ProfileList;
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                    eprintln!("[ChartApp] profile_manager: dimmer clicked, dismissing (live profile exists)");
                }
                return;
            }
            // Absorb other profile_manager: background clicks
            if widget_id.starts_with("profile_manager:") {
                return;
            }
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
            self.watchlist_modal.open();
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
            if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                match mode {
                    0 => {
                        // Reset to original order from snapshot.
                        if let Some(snap) = list.order_snapshot.take() {
                            list.ungrouped = snap;
                        }
                        eprintln!("[Sidebar] Watchlist sort reset to original order");
                    }
                    1 | 2 => {
                        // Save snapshot if not already saved (first sort from unsorted state).
                        if list.order_snapshot.is_none() {
                            list.order_snapshot = Some(list.ungrouped.clone());
                        }
                        // Always sort from the snapshot base so switching between modes
                        // 1 and 2 does not compound on an already-sorted order.
                        let base = list.order_snapshot.as_ref().unwrap().clone();
                        let color_order: &[&str] = &[
                            "#ef5350", "#f59e0b", "#22c55e", "#3b82f6",
                            "#a855f7", "#ec4899", "#6b7280",
                        ];
                        let mut symbols = base;
                        symbols.sort_by(|a, b| {
                            let a_idx = list.get_color_flag(&a.symbol, &a.exchange, &a.account_type)
                                .and_then(|f| color_order.iter().position(|c| *c == f))
                                .unwrap_or(99);
                            let b_idx = list.get_color_flag(&b.symbol, &b.exchange, &b.account_type)
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
                            let symbol = item.symbol.clone();
                            let exchange = item.exchange.clone();
                            let account_type = item.account_type.clone();
                            if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                                let color = colors.get(color_idx).copied().unwrap_or("");
                                list.set_color_flag(&symbol, &exchange, &account_type, color);
                                let color_opt = if color.is_empty() { None } else { Some(color.to_string()) };
                                self.watchlist_actions.push(crate::WatchlistAction::SetColorFlag { symbol: symbol.clone(), exchange: exchange.clone(), account_type: account_type.clone(), color: color_opt });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[Sidebar] Color flag set: {}:{}:{} = {:?}", symbol, exchange, account_type, color);
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
                    // Remove symbol from snapshot (if active) before removing from list.
                    if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                        if let Some(ref mut snap) = list.order_snapshot {
                            snap.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == account_type));
                        }
                    }
                    self.sidebar_state.watchlist_manager.remove_symbol(&symbol, &exchange, &account_type);
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
                    let resolved_exchange = self.exchange_symbols
                        .keys()
                        .find(|eid| eid.as_str() == item_exchange)
                        .copied()
                        .unwrap_or(self.active_exchange);

                    let prev_symbol = self.panel_app.panel_grid.active_window()
                        .map(|w| w.symbol.clone());
                    let prev_exchange = self.panel_app.panel_grid.active_window()
                        .map(|w| w.exchange.clone());
                    let prev_account_type = self.panel_app.panel_grid.active_window()
                        .map(|w| w.account_type.clone());
                    if prev_symbol.as_deref() != Some(&symbol) || prev_exchange.as_deref() != Some(&item_exchange) || prev_account_type.as_deref() != Some(&item_account_type) {
                        // Capture old trade-stream identity BEFORE mutating window state.
                        let old_trade_exchange = self.active_exchange;
                        let old_trade_symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        let old_trade_at = self.panel_app.panel_grid.active_window()
                            .map(|w| crate::account_type_from_label(&w.account_type))
                            .unwrap_or(digdigdig3::AccountType::Spot);
                        let timeframe = self.panel_app.panel_grid.active_window()
                            .map(|w| w.timeframe.clone())
                            .unwrap_or_default();
                        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let old_sym = window.symbol.clone();
                            let old_exchange = window.exchange.clone();
                            let old_account_type = window.account_type.clone();
                            window.snapshot_drawings_for_symbol(&old_sym, &old_exchange, &old_account_type);
                            window.symbol = symbol.clone();
                            window.exchange = item_exchange.clone();
                            window.account_type = item_account_type.clone();
                            window.update_title();
                            window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
                            window.bars.clear();
                            window.viewport.bar_count = 0;
                            window.viewport.view_start = 0.0;
                            window.pending_symbol_load = true;
                            window.drawing_manager.clear_all_primitives();
                            window.restore_drawings_for_symbol(&symbol, &item_exchange, &item_account_type);
                        }
                        self.active_exchange = resolved_exchange;
                        // Unsubscribe only the old symbol's trade stream, leaving other
                        // windows' streams intact.
                        if !old_trade_symbol.is_empty() {
                            self.bridge.unsubscribe_trades(old_trade_exchange, &old_trade_symbol, old_trade_at);
                        }
                        let eid_str = resolved_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (watchlist sidebar click)", eid_str);
                        } else {
                            let item_at = crate::account_type_from_label(&item_account_type);
                            self.bridge.ensure_connector(resolved_exchange);
                            self.bridge.request_bars(resolved_exchange, &symbol, &timeframe, item_at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), false);
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
                        self.bridge.enable_connector(eid);
                        eprintln!("[Sidebar] Connector enabled: {}", exchange_id_str);
                    } else {
                        self.bridge.disable_connector(eid);
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

        // === Performance panel control clicks — cycle through values ===
        if widget_id == "perf:backend" {
            use sidebar_content::state::RenderBackend;
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

        // --- CLI spawn buttons: agent:spawn:{claude|codex|gemini|opencode} ---
        if let Some(cli_str) = widget_id.strip_prefix("agent:spawn:") {
            use gate4agent::AgentCli;
            use sidebar_content::agents_dock::{AgentLeafDescriptor, AgentPaneLeaf};
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
                    use sidebar_content::state::AgentSpawnLayout;
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
            self.sidebar_state.agent_spawn_layout = sidebar_content::state::AgentSpawnLayout::SplitH;
            self.sidebar_data_dirty = true;
            return;
        }
        if widget_id == "agent:split:v" {
            self.sidebar_state.agent_spawn_layout = sidebar_content::state::AgentSpawnLayout::SplitV;
            self.sidebar_data_dirty = true;
            return;
        }
        if widget_id == "agent:split:replace" {
            self.sidebar_state.agent_spawn_layout = sidebar_content::state::AgentSpawnLayout::Replace;
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
                use sidebar_content::agents_dock::AgentLeafDescriptor;
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
                    let _ = self.bridge.runtime().block_on(self.agent.stop_instance(id));
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
                            .get(&next_id).map(|s| s.as_str()).unwrap_or("");
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
                            .get(&leaf_id).map(|s| s.as_str()).unwrap_or("");
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
                                .get(&leaf_id).map(|s| s.as_str()).unwrap_or("");
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
                        let _ = self.bridge.runtime().block_on(self.agent.stop_instance(id));
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
                                    .get(&next_id).map(|s| s.as_str()).unwrap_or("");
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
                                match self.bridge.runtime().block_on(self.agent.start_pty_instance(id, config)) {
                                    Ok(()) => {
                                        eprintln!("[ChartApp] agent:leaf:{:?}:start PTY started", leaf_id);
                                        self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_PTY);
                                        self.agent_pty_hover_focused = true;
                                    }
                                    Err(e) => eprintln!("[ChartApp] start PTY error: {}", e),
                                }
                            }
                            gate4agent::InstanceMode::Chat => {
                                // Chat starts lazily on first send — focus input field.
                                // Sync buffer first to avoid leaking previous leaf's text.
                                let buf = self.sidebar_state.agent_input_buffers
                                    .get(&leaf_id).map(|s| s.as_str()).unwrap_or("");
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
                            match self.bridge.runtime().block_on(self.agent.send_chat_instance(id, &text)) {
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
                if idx < 4 {
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
                    if idx < 4 {
                        use zengeld_panels::trading::SymbolSource;
                        use sidebar_content::state::SlotSourceMode;

                        // Build SymbolSource from the toolbar's current source mode.
                        let source = match self.sidebar_state.slot_source_mode {
                            SlotSourceMode::Auto => SymbolSource::HyperFocus,
                            SlotSourceMode::Pinned => {
                                let sym = self.panel_app.panel_grid.active_window()
                                    .map(|w| w.symbol.clone())
                                    .unwrap_or_else(|| "BTCUSDT".to_string());
                                let exch = self.panel_app.panel_grid.active_window()
                                    .map(|w| w.exchange.clone())
                                    .unwrap_or_else(|| self.active_exchange.as_str().to_string());
                                let at = self.panel_app.panel_grid.active_window()
                                    .map(|w| w.account_type.clone())
                                    .unwrap_or_else(|| digdigdig3::AccountType::Spot.short_label().to_string());
                                SymbolSource::Fixed { symbol: sym, exchange: exch, account_type: at }
                            }
                            SlotSourceMode::Linked => {
                                let leaf_id = self.panel_app.panel_grid.docking().active_leaf()
                                    .map(|lid| lid.0)
                                    .unwrap_or(0);
                                SymbolSource::BoundToChart { leaf_id }
                            }
                        };

                        // Resolve immediately to get the concrete symbol for initial setup.
                        let active_resolved = self.panel_app.panel_grid.active_window().map(|w| {
                            zengeld_panels::trading::ResolvedSymbol {
                                symbol: w.symbol.clone(),
                                exchange: w.exchange.clone(),
                                account_type: w.account_type.clone(),
                            }
                        });
                        let resolve_leaf = |lid: u64| -> Option<zengeld_panels::trading::ResolvedSymbol> {
                            let leaf = zengeld_chart::LeafId(lid);
                            self.panel_app.panel_grid.window_for_leaf(leaf).map(|w| {
                                zengeld_panels::trading::ResolvedSymbol {
                                    symbol: w.symbol.clone(),
                                    exchange: w.exchange.clone(),
                                    account_type: w.account_type.clone(),
                                }
                            })
                        };
                        let resolved = source.resolve(active_resolved.as_ref(), &resolve_leaf);
                        let symbol = resolved.as_ref().map(|r| r.symbol.clone())
                            .unwrap_or_else(|| "BTCUSDT".to_string());
                        let exchange_str = resolved.as_ref().map(|r| r.exchange.clone())
                            .unwrap_or_else(|| self.active_exchange.as_str().to_string());
                        let account_type_str = resolved.as_ref().map(|r| r.account_type.clone())
                            .unwrap_or_else(|| digdigdig3::AccountType::Spot.short_label().to_string());

                        let resolved_tick_size = self.exchange_symbols
                            .iter()
                            .find(|(eid, _)| eid.as_str() == exchange_str)
                            .and_then(|(_, syms)| {
                                syms.iter().find(|s| s.symbol == symbol).and_then(|s| s.tick_size)
                            })
                            .filter(|t| *t > 0.0)
                            .unwrap_or(0.01);

                        let item_opt = match kind_str {
                            "dom" => {
                                let pid = self.panels_store.create_dom(symbol.clone(), resolved_tick_size);
                                if !symbol.is_empty() {
                                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                                        .unwrap_or(self.active_exchange);
                                    let at = crate::account_type_from_label(&account_type_str);
                                    let ob_handle = self.bridge.subscribe_orderbook(eid, &symbol, at);
                                    let trade_handle = self.bridge.subscribe_trades(eid, &symbol, at);
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        state.exchange = exchange_str.clone();
                                        state.account_type = account_type_str.clone();
                                        state.shared_orderbook = Some(ob_handle);
                                        state.last_seen_orderbook_version = 0;
                                        state.shared_trades = Some(trade_handle);
                                    }
                                } else if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::Dom(pid))
                            }
                            "footprint" => {
                                let pid = self.panels_store.create_footprint(symbol.clone(), resolved_tick_size);
                                if !symbol.is_empty() {
                                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                                        .unwrap_or(self.active_exchange);
                                    let at = crate::account_type_from_label(&account_type_str);
                                    let handle = self.bridge.subscribe_trades(eid, &symbol, at);
                                    if let Some(state) = self.panels_store.footprint.get_mut(&pid) {
                                        state.exchange = exchange_str.clone();
                                        state.account_type = account_type_str.clone();
                                        state.shared_trades = Some(handle);
                                        state.last_seen_trade_version = 0;
                                    }
                                } else if let Some(state) = self.panels_store.footprint.get_mut(&pid) {
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::Footprint(pid))
                            }
                            "volume_profile" => {
                                let pid = self.panels_store.create_volume_profile(symbol.clone(), resolved_tick_size);
                                if !symbol.is_empty() {
                                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                                        .unwrap_or(self.active_exchange);
                                    let at = crate::account_type_from_label(&account_type_str);
                                    let handle = self.bridge.subscribe_trades(eid, &symbol, at);
                                    if let Some(state) = self.panels_store.volume_profile.get_mut(&pid) {
                                        state.exchange = exchange_str.clone();
                                        state.account_type = account_type_str.clone();
                                        state.shared_trades = Some(handle);
                                        state.last_seen_trade_version = 0;
                                    }
                                } else if let Some(state) = self.panels_store.volume_profile.get_mut(&pid) {
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::VolumeProfile(pid))
                            }
                            "liquidity_heatmap" => {
                                let pid = self.panels_store.create_liquidity_heatmap(symbol.clone(), resolved_tick_size, 1000);
                                if !symbol.is_empty() {
                                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                                        .unwrap_or(self.active_exchange);
                                    let at = crate::account_type_from_label(&account_type_str);
                                    let handle = self.bridge.subscribe_orderbook(eid, &symbol, at);
                                    if let Some(state) = self.panels_store.liquidity_heatmap.get_mut(&pid) {
                                        state.exchange = exchange_str.clone();
                                        state.account_type = account_type_str.clone();
                                        state.shared_orderbook = Some(handle);
                                        state.last_seen_orderbook_version = 0;
                                    }
                                } else if let Some(state) = self.panels_store.liquidity_heatmap.get_mut(&pid) {
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::LiquidityHeatmap(pid))
                            }
                            "big_trades" => {
                                let pid = self.panels_store.create_big_trades();
                                // Subscribe to the shared trade ring and attach the handle
                                // to the panel state so tick() can pull new trades.
                                if !symbol.is_empty() {
                                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                                        .unwrap_or(self.active_exchange);
                                    let at = crate::account_type_from_label(&account_type_str);
                                    let handle = self.bridge.subscribe_trades(eid, &symbol, at);
                                    if let Some(state) = self.panels_store.big_trades.get_mut(&pid) {
                                        state.symbol = symbol.clone();
                                        state.exchange = exchange_str.clone();
                                        state.account_type = account_type_str.clone();
                                        state.shared_trades = Some(handle);
                                        state.last_seen_trade_version = 0;
                                    }
                                } else if let Some(state) = self.panels_store.big_trades.get_mut(&pid) {
                                    state.symbol = symbol.clone();
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::BigTrades(pid))
                            }
                            "l2_tape" => {
                                let pid = self.panels_store.create_l2_tape();
                                if !symbol.is_empty() {
                                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                                        .unwrap_or(self.active_exchange);
                                    let at = crate::account_type_from_label(&account_type_str);
                                    let handle = self.bridge.subscribe_orderbook(eid, &symbol, at);
                                    if let Some(state) = self.panels_store.l2_tape.get_mut(&pid) {
                                        state.symbol = symbol.clone();
                                        state.exchange = exchange_str.clone();
                                        state.account_type = account_type_str.clone();
                                        state.shared_orderbook = Some(handle);
                                        state.last_seen_orderbook_version = 0;
                                    }
                                } else if let Some(state) = self.panels_store.l2_tape.get_mut(&pid) {
                                    state.symbol = symbol.clone();
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::L2Tape(pid))
                            }
                            "trade_tape" => {
                                let pid = self.panels_store.create_trade_tape();
                                if !symbol.is_empty() {
                                    let eid = digdigdig3::ExchangeId::from_str(&exchange_str)
                                        .unwrap_or(self.active_exchange);
                                    let at = crate::account_type_from_label(&account_type_str);
                                    let handle = self.bridge.subscribe_trades(eid, &symbol, at);
                                    if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                                        state.symbol = symbol.clone();
                                        state.exchange = exchange_str.clone();
                                        state.account_type = account_type_str.clone();
                                        state.shared_trades = Some(handle);
                                        state.last_seen_version = 0;
                                    }
                                } else if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                                    state.symbol = symbol.clone();
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::TradeTape(pid))
                            }
                            "order_entry" => {
                                let pid = self.panels_store.create_order_entry(symbol.clone());
                                if let Some(state) = self.panels_store.order_entry.get_mut(&pid) {
                                    state.source = source.clone();
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::OrderEntry(pid))
                            }
                            "position_manager" => {
                                let pid = self.panels_store.create_position_manager();
                                Some(sidebar_content::free_slot::FreeItem::PositionManager(pid))
                            }
                            "trade_log" => {
                                let pid = self.panels_store.create_trade_log();
                                Some(sidebar_content::free_slot::FreeItem::TradeLog(pid))
                            }
                            "risk_calculator" => {
                                let pid = self.panels_store.create_risk_calculator();
                                Some(sidebar_content::free_slot::FreeItem::RiskCalculator(pid))
                            }
                            "trading_container" => {
                                let pid = self.panels_store.create_trading_container(symbol.clone(), 0.01, 0.0);
                                if let Some(state) = self.panels_store.trading_container.get_mut(&pid) {
                                    state.source = source.clone();
                                    state.exchange = exchange_str.clone();
                                    state.account_type = account_type_str.clone();
                                }
                                Some(sidebar_content::free_slot::FreeItem::TradingContainer(pid))
                            }
                            _ => None,
                        };

                        // Subscribe depth for panels that need raw orderbook data.
                        // DOM is excluded here because it subscribes via subscribe_orderbook()
                        // above, which internally calls subscribe_depth().
                        let needs_depth = matches!(kind_str, "l2_tape" | "liquidity_heatmap");
                        if needs_depth {
                            let eid = self.exchange_symbols
                                .keys()
                                .find(|e| e.as_str() == exchange_str)
                                .copied()
                                .unwrap_or(self.active_exchange);
                            let at = crate::account_type_from_label(&account_type_str);
                            self.bridge.subscribe_depth(eid, &symbol, at);
                        }

                        // Register market-data panels in TagManager as Synced members.
                        // order_entry / trading_container are account-bound, not market-data.
                        let is_market_data = matches!(
                            kind_str,
                            "dom" | "footprint" | "volume_profile" | "liquidity_heatmap"
                                | "big_trades" | "l2_tape" | "trade_tape"
                        );
                        if is_market_data {
                            if let Some(ref item) = item_opt {
                                let pid = sidebar_content::free_slot::PanelId(item.panel_id().0);
                                let member = zengeld_chart::tag_manager::SyncMemberId::Panel(pid.0);
                                let group_id = self.panel_app.tag_manager.active_chart_group
                                    .or_else(|| {
                                        self.panel_app.panel_grid
                                            .active_chart_id()
                                            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
                                    });
                                if let Some(gid) = group_id {
                                    self.panel_app.tag_manager.set_synced(member, gid);
                                }
                            }
                        }

                        if let Some(item) = item_opt {
                            use sidebar_content::state::AgentSpawnLayout;
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
                                // split that leaf using the chosen direction (mirrors agent spawn).
                                (AgentSpawnLayout::SplitH, Some(focus), false)
                                | (AgentSpawnLayout::SplitV, Some(focus), false) => {
                                    let split_kind = if layout == AgentSpawnLayout::SplitH {
                                        SplitKind::SplitRight
                                    } else {
                                        SplitKind::SplitBottom
                                    };
                                    let rw = self.sidebar_state.right_sidebar_width as f32;
                                    let rh = self.height as f32;
                                    let new_ids = self.sidebar_state.slot_dockings[idx]
                                        .inner_mut().tree_mut()
                                        .split_leaf_with_children(focus, split_kind, rw, rh);
                                    if new_ids.len() >= 2 {
                                        // Insert the new panel into the sibling slot
                                        // ([0] retains the existing item, [1] is the fresh empty leaf).
                                        let sibling = new_ids[1];
                                        let tree = self.sidebar_state.slot_dockings[idx]
                                            .inner_mut().tree_mut();
                                        if let Some(leaf) = tree.leaf_mut(sibling) {
                                            leaf.panels.push(item);
                                            leaf.active_tab = 0;
                                        }
                                        sibling
                                    } else {
                                        // Split fell back — append as a peer leaf at root.
                                        self.sidebar_state.slot_dockings[idx].inner_mut()
                                            .tree_mut().add_leaf(item)
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
                }
                return;
            }
        }

        // --- slot:{idx}:split:h / :split:v / :split:replace — spawn layout toggle ---
        if let Some(rest) = widget_id.strip_prefix("slot:") {
            if let Some((idx_str, action)) = rest.split_once(':') {
                if let Ok(idx) = idx_str.parse::<usize>() {
                    if idx < 4 {
                        match action {
                            "split:h" => {
                                self.sidebar_state.slot_spawn_layout =
                                    sidebar_content::state::AgentSpawnLayout::SplitH;
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "split:v" => {
                                self.sidebar_state.slot_spawn_layout =
                                    sidebar_content::state::AgentSpawnLayout::SplitV;
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "split:replace" => {
                                self.sidebar_state.slot_spawn_layout =
                                    sidebar_content::state::AgentSpawnLayout::Replace;
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "source:auto" => {
                                self.sidebar_state.slot_source_mode =
                                    sidebar_content::state::SlotSourceMode::Auto;
                                // If a panel in this slot is focused, update its source too.
                                if let Some((focused_idx, focused_leaf)) = self.sidebar_state.focused_free_leaf {
                                    if focused_idx == idx {
                                        let item_opt = self.sidebar_state.slot_dockings[idx]
                                            .inner()
                                            .tree()
                                            .leaf(focused_leaf)
                                            .and_then(|l| l.active_panel().cloned());
                                        if let Some(item) = item_opt {
                                            use zengeld_panels::trading::SymbolSource;
                                            use sidebar_content::free_slot::FreeItem;
                                            let src = SymbolSource::HyperFocus;
                                            // Market-data panels (dom, footprint, etc.) no longer use
                                            // SymbolSource; they are tracked via TagManager.
                                            match &item {
                                                FreeItem::OrderEntry(id) => { if let Some(s) = self.panels_store.order_entry.get_mut(id) { s.source = src; } }
                                                FreeItem::TradingContainer(id) => { if let Some(s) = self.panels_store.trading_container.get_mut(id) { s.source = src; } }
                                                _ => {}
                                            }
                                            self.autosave_snapshot();
                                        }
                                    }
                                }
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "source:pinned" => {
                                self.sidebar_state.slot_source_mode =
                                    sidebar_content::state::SlotSourceMode::Pinned;
                                // If a panel in this slot is focused, update its source too.
                                if let Some((focused_idx, focused_leaf)) = self.sidebar_state.focused_free_leaf {
                                    if focused_idx == idx {
                                        let item_opt = self.sidebar_state.slot_dockings[idx]
                                            .inner()
                                            .tree()
                                            .leaf(focused_leaf)
                                            .and_then(|l| l.active_panel().cloned());
                                        if let Some(item) = item_opt {
                                            use zengeld_panels::trading::SymbolSource;
                                            use sidebar_content::free_slot::FreeItem;
                                            // Pin to current active chart symbol/exchange/account_type.
                                            let symbol = self.panel_app.panel_grid.active_window()
                                                .map(|w| w.symbol.clone())
                                                .unwrap_or_default();
                                            let exchange = self.panel_app.panel_grid.active_window()
                                                .map(|w| w.exchange.clone())
                                                .unwrap_or_default();
                                            let account_type = self.panel_app.panel_grid.active_window()
                                                .map(|w| w.account_type.clone())
                                                .unwrap_or_default();
                                            let src = SymbolSource::Fixed { symbol, exchange, account_type };
                                            // Market-data panels (dom, footprint, etc.) no longer use
                                            // SymbolSource; they are tracked via TagManager.
                                            match &item {
                                                FreeItem::OrderEntry(id) => { if let Some(s) = self.panels_store.order_entry.get_mut(id) { s.source = src; } }
                                                FreeItem::TradingContainer(id) => { if let Some(s) = self.panels_store.trading_container.get_mut(id) { s.source = src; } }
                                                _ => {}
                                            }
                                            self.autosave_snapshot();
                                        }
                                    }
                                }
                                self.sidebar_data_dirty = true;
                                return;
                            }
                            "source:linked" => {
                                self.sidebar_state.slot_source_mode =
                                    sidebar_content::state::SlotSourceMode::Linked;
                                // If a panel in this slot is focused, update its source too.
                                if let Some((focused_idx, focused_leaf)) = self.sidebar_state.focused_free_leaf {
                                    if focused_idx == idx {
                                        let item_opt = self.sidebar_state.slot_dockings[idx]
                                            .inner()
                                            .tree()
                                            .leaf(focused_leaf)
                                            .and_then(|l| l.active_panel().cloned());
                                        if let Some(item) = item_opt {
                                            use zengeld_panels::trading::SymbolSource;
                                            use sidebar_content::free_slot::FreeItem;
                                            let leaf_id = self.panel_app.panel_grid.docking().active_leaf()
                                                .map(|lid| lid.0)
                                                .unwrap_or(0);
                                            let src = SymbolSource::BoundToChart { leaf_id };
                                            // Market-data panels (dom, footprint, etc.) no longer use
                                            // SymbolSource; they are tracked via TagManager.
                                            match &item {
                                                FreeItem::OrderEntry(id) => { if let Some(s) = self.panels_store.order_entry.get_mut(id) { s.source = src; } }
                                                FreeItem::TradingContainer(id) => { if let Some(s) = self.panels_store.trading_container.get_mut(id) { s.source = src; } }
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
                                        // Retrieve the FreeItem before removing so we can
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
                                            let member = zengeld_chart::tag_manager::SyncMemberId::Panel(item.panel_id().0);
                                            self.panel_app.tag_manager.disconnect(member);
                                            self.panel_app.tag_manager.synced_panels_remove(member);
                                            // Unsubscribe panels from shared data streams
                                            // before dropping their state.
                                            if let sidebar_content::free_slot::FreeItem::Dom(pid) = &item {
                                                if let Some(state) = self.panels_store.dom.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.bridge.unsubscribe_orderbook(eid, &state.symbol, at);
                                                            self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let sidebar_content::free_slot::FreeItem::BigTrades(pid) = &item {
                                                if let Some(state) = self.panels_store.big_trades.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let sidebar_content::free_slot::FreeItem::VolumeProfile(pid) = &item {
                                                if let Some(state) = self.panels_store.volume_profile.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let sidebar_content::free_slot::FreeItem::Footprint(pid) = &item {
                                                if let Some(state) = self.panels_store.footprint.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let sidebar_content::free_slot::FreeItem::L2Tape(pid) = &item {
                                                if let Some(state) = self.panels_store.l2_tape.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.bridge.unsubscribe_orderbook(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let sidebar_content::free_slot::FreeItem::TradeTape(pid) = &item {
                                                if let Some(state) = self.panels_store.trade_tape.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                                        }
                                                    }
                                                }
                                            }
                                            if let sidebar_content::free_slot::FreeItem::LiquidityHeatmap(pid) = &item {
                                                if let Some(state) = self.panels_store.liquidity_heatmap.get(pid) {
                                                    if !state.symbol.is_empty() {
                                                        if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                            let at = crate::account_type_from_label(&state.account_type);
                                                            self.bridge.unsubscribe_orderbook(eid, &state.symbol, at);
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
                                use zengeld_panels::trading::SymbolSource;
                                use sidebar_content::free_slot::FreeItem;
                                use sidebar_content::state::SlotSourceMode;
                                // Market-data panels no longer carry SymbolSource; their mode is
                                // always reflected as Auto (synced via TagManager).
                                let source = match &item {
                                    FreeItem::Dom(_)
                                    | FreeItem::Footprint(_)
                                    | FreeItem::VolumeProfile(_)
                                    | FreeItem::LiquidityHeatmap(_)
                                    | FreeItem::BigTrades(_)
                                    | FreeItem::L2Tape(_)
                                    | FreeItem::TradeTape(_) => Some(SymbolSource::HyperFocus),
                                    FreeItem::OrderEntry(id) => self.panels_store.order_entry.get(id).map(|s| s.source.clone()),
                                    FreeItem::TradingContainer(id) => self.panels_store.trading_container.get(id).map(|s| s.source.clone()),
                                    FreeItem::PositionManager(_) | FreeItem::TradeLog(_) | FreeItem::RiskCalculator(_) => None,
                                };
                                if let Some(src) = source {
                                    self.sidebar_state.slot_source_mode = match src {
                                        SymbolSource::HyperFocus => SlotSourceMode::Auto,
                                        SymbolSource::Fixed { .. } => SlotSourceMode::Pinned,
                                        SymbolSource::BoundToChart { .. } => SlotSourceMode::Linked,
                                    };
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
                            // Retrieve the FreeItem before removing so we can clean up the store.
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.panels.get(l.active_tab).cloned());
                            self.sidebar_state.slot_dockings[idx].inner_mut().tree_mut().remove_leaf(leaf_id);
                            if let Some(item) = item_opt {
                                // Deregister from TagManager before removing state.
                                let member = zengeld_chart::tag_manager::SyncMemberId::Panel(item.panel_id().0);
                                self.panel_app.tag_manager.disconnect(member);
                                self.panel_app.tag_manager.synced_panels_remove(member);
                                // Unsubscribe panels from shared data streams before
                                // dropping their state.
                                if let sidebar_content::free_slot::FreeItem::Dom(pid) = &item {
                                    if let Some(state) = self.panels_store.dom.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.bridge.unsubscribe_orderbook(eid, &state.symbol, at);
                                                self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let sidebar_content::free_slot::FreeItem::BigTrades(pid) = &item {
                                    if let Some(state) = self.panels_store.big_trades.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let sidebar_content::free_slot::FreeItem::VolumeProfile(pid) = &item {
                                    if let Some(state) = self.panels_store.volume_profile.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let sidebar_content::free_slot::FreeItem::Footprint(pid) = &item {
                                    if let Some(state) = self.panels_store.footprint.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let sidebar_content::free_slot::FreeItem::L2Tape(pid) = &item {
                                    if let Some(state) = self.panels_store.l2_tape.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.bridge.unsubscribe_orderbook(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let sidebar_content::free_slot::FreeItem::TradeTape(pid) = &item {
                                    if let Some(state) = self.panels_store.trade_tape.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.bridge.unsubscribe_trades(eid, &state.symbol, at);
                                            }
                                        }
                                    }
                                }
                                if let sidebar_content::free_slot::FreeItem::LiquidityHeatmap(pid) = &item {
                                    if let Some(state) = self.panels_store.liquidity_heatmap.get(pid) {
                                        if !state.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&state.exchange) {
                                                let at = crate::account_type_from_label(&state.account_type);
                                                self.bridge.unsubscribe_orderbook(eid, &state.symbol, at);
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
                            if let Some(sidebar_content::free_slot::FreeItem::Dom(pid)) = item_opt {
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
                            if let Some(sidebar_content::free_slot::FreeItem::Dom(pid)) = item_opt {
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
                                    sidebar_content::free_slot::FreeItem::Dom(pid) => {
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
                                    sidebar_content::free_slot::FreeItem::L2Tape(pid) => {
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
                                    sidebar_content::free_slot::FreeItem::TradeTape(pid) => {
                                        if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                                            match col_idx {
                                                0 => state.column_config.show_time = !state.column_config.show_time,
                                                1 => state.column_config.show_price = !state.column_config.show_price,
                                                2 => state.column_config.show_size = !state.column_config.show_size,
                                                _ => {}
                                            }
                                        }
                                    }
                                    sidebar_content::free_slot::FreeItem::BigTrades(pid) => {
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
                            if let Some(sidebar_content::free_slot::FreeItem::Dom(pid)) = item_opt {
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
                                // Resolve the active FreeItem for this leaf.
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
                                    use zengeld_panels::trading::SymbolSource;
                                    use sidebar_content::free_slot::FreeItem;

                                    // Read the current source from account-bound panel states.
                                    // Market-data panels (dom, footprint, etc.) no longer carry
                                    // SymbolSource — they are managed via TagManager.
                                    let current_source = match &item {
                                        FreeItem::Dom(_)
                                        | FreeItem::Footprint(_)
                                        | FreeItem::VolumeProfile(_)
                                        | FreeItem::LiquidityHeatmap(_)
                                        | FreeItem::BigTrades(_)
                                        | FreeItem::L2Tape(_)
                                        | FreeItem::TradeTape(_) => None,
                                        FreeItem::OrderEntry(id) => self.panels_store.order_entry.get(id).map(|s| s.source.clone()),
                                        FreeItem::TradingContainer(id) => self.panels_store.trading_container.get(id).map(|s| s.source.clone()),
                                        FreeItem::PositionManager(_) | FreeItem::TradeLog(_) | FreeItem::RiskCalculator(_) => None,
                                    };

                                    let new_source = match current_source {
                                        Some(SymbolSource::HyperFocus) => {
                                            // Pin to current active chart symbol/exchange/account_type.
                                            let symbol = self.panel_app.panel_grid.active_window()
                                                .map(|w| w.symbol.clone())
                                                .unwrap_or_default();
                                            let exchange = self.panel_app.panel_grid.active_window()
                                                .map(|w| w.exchange.clone())
                                                .unwrap_or_default();
                                            let account_type = self.panel_app.panel_grid.active_window()
                                                .map(|w| w.account_type.clone())
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
                                            FreeItem::OrderEntry(id) => { if let Some(s) = self.panels_store.order_entry.get_mut(id) { s.source = src; } }
                                            FreeItem::TradingContainer(id) => { if let Some(s) = self.panels_store.trading_container.get_mut(id) { s.source = src; } }
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
                                if let Some(sidebar_content::free_slot::FreeItem::Dom(pid)) = item_opt {
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
                                if let Some(sidebar_content::free_slot::FreeItem::Dom(pid)) = item_opt {
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
                                if let Some(sidebar_content::free_slot::FreeItem::Dom(pid)) = item_opt {
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
                        if let Some((leaf_id_str, local_id)) = leaf_rest.split_once(':') {
                            if local_id.starts_with("oe:") {
                                if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    if let Some(sidebar_content::free_slot::FreeItem::OrderEntry(pid)) = item_opt {
                                        use zengeld_panels::panel_trait::TradingPanel;
                                        if let Some(state) = self.panels_store.order_entry.get_mut(&pid) {
                                            if state.handle_click(local_id, x, y) {
                                                self.sidebar_data_dirty = true;
                                            }
                                        }
                                    }
                                }
                                return;
                            }
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
                                if let Some(item) = item_opt {
                                    use zengeld_panels::trading::SymbolSource;
                                    use sidebar_content::free_slot::FreeItem;
                                    use sidebar_content::state::SlotSourceMode;
                                    // Market-data panels no longer carry SymbolSource; they always
                                    // show as Auto (synced via TagManager).
                                    let source = match &item {
                                        FreeItem::Dom(_)
                                        | FreeItem::Footprint(_)
                                        | FreeItem::VolumeProfile(_)
                                        | FreeItem::LiquidityHeatmap(_)
                                        | FreeItem::BigTrades(_)
                                        | FreeItem::L2Tape(_)
                                        | FreeItem::TradeTape(_) => Some(SymbolSource::HyperFocus),
                                        FreeItem::OrderEntry(id) => self.panels_store.order_entry.get(id).map(|s| s.source.clone()),
                                        FreeItem::TradingContainer(id) => self.panels_store.trading_container.get(id).map(|s| s.source.clone()),
                                        FreeItem::PositionManager(_) | FreeItem::TradeLog(_) | FreeItem::RiskCalculator(_) => None,
                                    };
                                    if let Some(src) = source {
                                        self.sidebar_state.slot_source_mode = match src {
                                            SymbolSource::HyperFocus => SlotSourceMode::Auto,
                                            SymbolSource::Fixed { .. } => SlotSourceMode::Pinned,
                                            SymbolSource::BoundToChart { .. } => SlotSourceMode::Linked,
                                        };
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
                self.alert_manager.remove(id);
                eprintln!("[Sidebar] Alert deleted: {}", id);
                self.sidebar_data_dirty = true;
                self.autosave_snapshot();
            }
            return;
        }
        // Pattern: "alert_add_button" — open alert settings modal for new alert
        if widget_id == "alert_add_button" {
            let price = self.panel_app.panel_grid.active_window()
                .and_then(|w| w.bars.last())
                .map(|b| b.close)
                .unwrap_or(0.0);
            let symbol = self.panel_app.panel_grid.active_window()
                .map(|w| w.symbol.clone())
                .unwrap_or_else(|| "BTCUSD".to_string());
            let source = alerts::AlertSource::Price { symbol: symbol.clone() };
            self.panel_app.alert_settings_state.open_new(source, &symbol, price);
            self.panel_app.alert_settings_state.pin_initial_position(
                self.content_rect.width, self.content_rect.height,
            );
            eprintln!("[Sidebar] Alert settings modal opened (new, price={:.2})", price);
            return;
        }
        // Pattern: "alert_{id}" (row click — navigate to alert's window, then open edit modal)
        if let Some(id) = widget_id.strip_prefix("alert_").and_then(|s| s.parse::<u64>().ok()) {
            // Clone alert data we need before taking any mutable borrows.
            let alert_nav = self.alert_manager.get(id).map(|a| (a.window_id_hint, id));
            if let Some((window_id_hint, alert_id)) = alert_nav {
                // Navigate to the window that created this alert, if known.
                if let Some(wid) = window_id_hint {
                    let chart_id = zengeld_chart::ChartId(wid);
                    if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_chart_id(chart_id) {
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        eprintln!("[Sidebar] Alert {}: navigated to window {} (leaf {:?})", alert_id, wid, leaf_id);
                    }
                }
                // Open the edit modal.
                if let Some(alert) = self.alert_manager.get(alert_id) {
                    self.panel_app.alert_settings_state.open_edit(alert);
                    self.panel_app.alert_settings_state.pin_initial_position(
                        self.content_rect.width, self.content_rect.height,
                    );
                    eprintln!("[Sidebar] Alert edit opened: id={}", alert_id);
                }
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
                // 1. Try active drawing_manager first (handles Active + WindowOtherKey).
                let removed_from_dm = self.panel_app.panel_grid
                    .active_window_mut()
                    .and_then(|w| {
                        w.drawing_manager.find_index_by_id(id).map(|idx| {
                            w.drawing_manager.remove(idx);
                            true
                        })
                    })
                    .unwrap_or(false);

                if removed_from_dm {
                    self.sync_drawing_back_to_group();
                    self.autosave_snapshot();
                    self.sidebar_data_dirty = true;
                    eprintln!("[Sidebar] Drawing deleted from drawing_manager: {}", id);
                    return;
                }

                // 2. Try stashed_primitives (WindowStash).
                let removed_from_stash = self.panel_app.panel_grid
                    .active_window_mut()
                    .map(|w| {
                        let before = w.stashed_primitives.len();
                        w.stashed_primitives.retain(|p| p.data().id != id);
                        w.stashed_primitives.len() < before
                    })
                    .unwrap_or(false);

                if removed_from_stash {
                    self.autosave_snapshot();
                    self.sidebar_data_dirty = true;
                    eprintln!("[Sidebar] Drawing deleted from stashed_primitives: {}", id);
                    return;
                }

                // 3. Try group.primitives (GroupOtherKey).
                let group_id = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.group_id);
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        let before = group.primitives.len();
                        group.primitives.retain(|p| p.data().id != id);
                        if group.primitives.len() < before {
                            self.autosave_snapshot();
                            self.sidebar_data_dirty = true;
                            eprintln!("[Sidebar] Drawing deleted from group.primitives: {}", id);
                            return;
                        }
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
                if self.indicator_manager.get_instance(id).is_some() {
                    self.delete_indicator_instance(id);
                    return;
                }

                // Fall through: try group.indicator_configs (GroupIndicatorOtherKey).
                let group_id = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.group_id);
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        let before = group.indicator_configs.len();
                        group.indicator_configs.retain(|cfg| cfg.id != id);
                        if group.indicator_configs.len() < before {
                            self.autosave_snapshot();
                            self.sidebar_data_dirty = true;
                            eprintln!("[Sidebar] Group indicator config deleted: {}", id);
                        }
                    }
                }
            }
            return;
        }
        // --- Drawing settings ---
        if widget_id.contains("_drw_settings_") || widget_id.starts_with("drw_settings_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        self.panel_app.primitive_settings_state.open(idx);
                        eprintln!("[Sidebar] Primitive settings opened: {} (idx={})", id, idx);
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        let v = window.drawing_manager.primitives_mut()[idx].data().visible;
                        window.drawing_manager.primitives_mut()[idx].data_mut().visible = !v;
                    }
                }
                self.sidebar_data_dirty = true;
                eprintln!("[Sidebar] Drawing visibility toggled: {}", id);
            }
            return;
        }
        // --- Indicator visibility ---
        if widget_id.contains("_ind_vis_") || widget_id.starts_with("ind_vis_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                self.indicator_manager.toggle_visibility(id);
                self.sidebar_data_dirty = true;
                eprintln!("[Sidebar] Indicator visibility toggled: {}", id);
            }
            return;
        }
        // --- Drawing lock ---
        if widget_id.contains("_drw_lock_") || widget_id.starts_with("drw_lock_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        let l = window.drawing_manager.primitives_mut()[idx].data().locked;
                        window.drawing_manager.primitives_mut()[idx].data_mut().locked = !l;
                    }
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
        // Widget IDs use "cmp_" prefix. The numeric suffix is the series index.

        // --- Compare delete ---
        if widget_id.starts_with("cmp_delete_") {
            if let Some(idx) = widget_id.strip_prefix("cmp_delete_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(symbol) = window.compare_overlay.remove_series(idx) {
                        eprintln!("[Sidebar] Compare series removed: {} (idx={})", symbol, idx);
                    }
                }
            }
            return;
        }
        // --- Compare visibility toggle ---
        if widget_id.starts_with("cmp_vis_") {
            if let Some(idx) = widget_id.strip_prefix("cmp_vis_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    let new_vis = window.compare_overlay.toggle_visibility(idx);
                    eprintln!("[Sidebar] Compare visibility toggled: idx={} -> visible={}", idx, new_vis);
                }
            }
            return;
        }
        // --- Compare settings / alert / lock (no-op stubs) ---
        if widget_id.starts_with("cmp_settings_") || widget_id.starts_with("cmp_alert_") || widget_id.starts_with("cmp_lock_") {
            return;
        }
        // --- Compare row click (row selection) ---
        {
            let cmp_id = widget_id
                .strip_prefix("cmp_")
                .and_then(|s| s.parse::<u64>().ok());
            if let Some(id) = cmp_id {
                if self.sidebar_state.object_tree_items.iter().any(|item| {
                    item.id == id && item.category == zengeld_chart::ObjectCategory::Compare
                }) {
                    for item in &mut self.sidebar_state.object_tree_items {
                        item.selected = item.id == id && item.category == zengeld_chart::ObjectCategory::Compare;
                    }
                    eprintln!("[Sidebar] Compare series selected: idx={}", id);
                }
                return;
            }
        }

        // --- Drawing alert button ---
        if widget_id.contains("_drw_alert_") || widget_id.starts_with("drw_alert_") {
            if let Some(id) = widget_id.rsplit('_').next().and_then(|s| s.parse::<u64>().ok()) {
                let mut price = self.panel_app.panel_grid.active_window()
                    .and_then(|w| w.bars.last())
                    .map(|b| b.close)
                    .unwrap_or(0.0);
                let mut source_name = format!("Drawing {}", id);
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        let prim = &window.drawing_manager.primitives()[idx];
                        source_name = prim.display_name().to_string();
                        let pts = prim.points();
                        if !pts.is_empty() {
                            price = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
                        }
                    }
                }
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone())
                    .unwrap_or_else(|| "BTCUSD".to_string());
                let source = alerts::AlertSource::Drawing { primitive_id: id, label: source_name.clone() };
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
                let price = self.panel_app.panel_grid.active_window()
                    .and_then(|w| w.bars.last())
                    .map(|b| b.close)
                    .unwrap_or(0.0);
                let source_name = self.indicator_manager.get_instance(id)
                    .map(|inst| inst.name.clone())
                    .unwrap_or_else(|| format!("Indicator {}", id));
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone())
                    .unwrap_or_else(|| "BTCUSD".to_string());
                let source = alerts::AlertSource::Signal {
                    indicator_id: id,
                    label: source_name.clone(),
                    direction_filter: alerts::SignalDirection::Any,
                    bar_state: alerts::SignalBarState::Forming,
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
                // Find the first Active alert bound to this primitive.
                let alert = self.alert_manager.items()
                    .iter()
                    .find(|a| matches!(
                        &a.source,
                        alerts::AlertSource::Drawing { primitive_id, .. } if *primitive_id == prim_id
                    ) && a.status == alerts::AlertStatus::Active)
                    .cloned();
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
                let alert = self.alert_manager.items()
                    .iter()
                    .find(|a| matches!(
                        &a.source,
                        alerts::AlertSource::Signal { indicator_id, .. } if *indicator_id == ind_id
                    ) && a.status == alerts::AlertStatus::Active)
                    .cloned();
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

                    // Click-to-center: if a drawing row was clicked, scroll the
                    // viewport so the drawing's midpoint is horizontally centred.
                    if is_drw_row {
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let center_bar = window.drawing_manager.primitives()
                                .iter()
                                .find(|p| p.data().id == id)
                                .and_then(|prim| {
                                    let pts = prim.points();
                                    if pts.is_empty() {
                                        None
                                    } else {
                                        // Average ts_ms then convert to bar index
                                        let avg_ts = pts.iter().map(|p| p.0).sum::<i64>() / pts.len() as i64;
                                        Some(zengeld_chart::timestamp_ms_to_bar_f64(&window.bars, avg_ts))
                                    }
                                });
                            if let Some(mid_bar) = center_bar {
                                let visible_bars = window.viewport.chart_width / window.viewport.bar_spacing;
                                window.viewport.view_start = (mid_bar - visible_bars / 2.0).max(0.0);
                                if window.price_scale.scale_mode.is_auto_y() {
                                    window.calc_auto_scale();
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
            let parts: Vec<&str> = widget_id.strip_prefix("signal_").unwrap_or("").splitn(2, '_').collect();
            if parts.len() == 2 {
                if let Ok(bar_index) = parts[1].parse::<i64>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let visible_bars = window.viewport.chart_width / window.viewport.bar_spacing;
                        window.viewport.view_start = (bar_index as f64 - visible_bars / 2.0).max(0.0);
                        if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale();
                        }
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
        if widget_id.starts_with("toolbar:") || widget_id.starts_with("dtb:") || widget_id.starts_with("csb:") || widget_id.starts_with("btb:") || widget_id.starts_with("rtb:") || widget_id.starts_with("ilb:") {
            let item_id = widget_id
                .strip_prefix("toolbar:")
                .or_else(|| widget_id.strip_prefix("dtb:"))
                .or_else(|| widget_id.strip_prefix("csb:"))
                .or_else(|| widget_id.strip_prefix("btb:"))
                .or_else(|| widget_id.strip_prefix("rtb:"))
                .or_else(|| widget_id.strip_prefix("ilb:"))
                .unwrap_or(widget_id)
                .to_string();

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
            // on ChartWindow, so we extract the command here and apply it.
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

            // Split borrows: toolbar_state and panel_grid are separate fields of panel_app.
            // We can't split-borrow through panel_app, so we use a raw pointer for window.
            let window_ptr: *mut _ = self.panel_app.panel_grid.active_window_mut()
                .map(|w| w as *mut _)
                .unwrap_or(std::ptr::null_mut::<zengeld_chart::ChartWindow>());

            if !window_ptr.is_null() {
                // SAFETY: toolbar_state does not access panel_grid.
                let window = unsafe { &mut *window_ptr };
                let events = self.panel_app.toolbar_state.handle_toolbar_click_with_chart(
                    &item_id,
                    &mut window.crosshair,
                    &mut window.drawing_manager,
                );
                for event in events {
                    self.process_chart_out_event(event);
                }
            }
            return;
        }

        // === Dropdown items ===
        if let Some(rest) = widget_id.strip_prefix("dropdown:") {
            // Background click — close the dropdown without selecting any item.
            if rest == "__bg__" {
                self.panel_app.toolbar_state.open_dropdown_id = None;
                self.panel_app.toolbar_state.open_dropdown_position = None;
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

                let dropdown_id = dropdown_id.to_string();
                let item_id = item_id.to_string();

                let window_ptr: *mut _ = self.panel_app.panel_grid.active_window_mut()
                    .map(|w| w as *mut _)
                    .unwrap_or(std::ptr::null_mut::<zengeld_chart::ChartWindow>());

                if !window_ptr.is_null() {
                    // SAFETY: toolbar_state does not access panel_grid.
                    let window = unsafe { &mut *window_ptr };
                    let autosave = self.panel_app.autosave_enabled;
                    let events = self.panel_app.toolbar_state.handle_dropdown_select_with_chart(
                        &dropdown_id,
                        &item_id,
                        &mut window.crosshair,
                        &mut window.drawing_manager,
                        autosave,
                    );
                    for event in events {
                        self.process_chart_out_event(event);
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
                    use zengeld_chart::ui::modal_settings::OverlayPanelTreeTab;
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
                        let leaf_id = zengeld_chart::LeafId(node_id);
                        if self.panel_app.panel_grid.docking().tree().leaf(leaf_id).is_some() {
                            self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        }
                    }
                }
                id if id.starts_with("minimap_leaf:") => {
                    if let Ok(leaf_id_val) = id["minimap_leaf:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(leaf_id_val);
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                id if id.starts_with("eliminate:") => {
                    if let Ok(leaf_id_val) = id["eliminate:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let removed = self.panel_app.panel_grid.close_leaf(leaf_id);
                        if removed {
                            // If eliminated leaf was the target, clear it
                            if self.panel_app.overlay_settings_state.target_leaf_id == Some(leaf_id) {
                                self.panel_app.overlay_settings_state.target_leaf_id = None;
                                self.panel_app.overlay_settings_state.selected_node_id = None;
                            }
                            eprintln!("[OverlaySettings] Eliminated leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("hide:") => {
                    if let Ok(leaf_id_val) = id["hide:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let hidden = self.panel_app.panel_grid.docking_mut().tree_mut().hide_leaf(leaf_id);
                        if hidden {
                            eprintln!("[OverlaySettings] Hidden leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("show:") || id.starts_with("restore:") => {
                    let prefix = if id.starts_with("show:") { "show:" } else { "restore:" };
                    if let Ok(leaf_id_val) = id[prefix.len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.docking_mut().tree_mut().show_leaf(leaf_id);
                        eprintln!("[OverlaySettings] Shown leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("expand:") => {
                    if let Ok(leaf_id_val) = id["expand:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
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

        // === Tags & Tabs modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("tags_tabs:") {
            use zengeld_chart::ui::modal_settings::{TagsTabsSidebar, TagsTabsTagsTab};
            use zengeld_chart::tag_manager::SyncGroupId;
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
                    use zengeld_chart::ui::modal_settings::OverlayPanelTreeTab;
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
                        self.panel_app.tags_tabs_state.selected_group_id = Some(SyncGroupId(gid_val));
                        self.panel_app.tags_tabs_state.set_tags_tab(TagsTabsTagsTab::Details);
                        eprintln!("[TagsTabs] Selected group {}", gid_val);
                    }
                }
                id if id.starts_with("tags:delete_group:") => {
                    if let Ok(gid_val) = id["tags:delete_group:".len()..].parse::<u64>() {
                        let gid = SyncGroupId(gid_val);
                        self.panel_app.tag_manager.remove_group(gid);
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
                            let gid = SyncGroupId(gid_val);
                            if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                                match flag_str {
                                    "sync_crosshair"  => group.sync_flags.sync_crosshair  = !group.sync_flags.sync_crosshair,
                                    "sync_viewport"   => group.sync_flags.sync_viewport   = !group.sync_flags.sync_viewport,
                                    "sync_symbol"     => group.sync_flags.sync_symbol     = !group.sync_flags.sync_symbol,
                                    "sync_timeframe"  => group.sync_flags.sync_timeframe  = !group.sync_flags.sync_timeframe,
                                    "sync_drawings"   => group.sync_flags.sync_drawings   = !group.sync_flags.sync_drawings,
                                    "sync_indicators" => group.sync_flags.sync_indicators = !group.sync_flags.sync_indicators,
                                    _ => {}
                                }
                                eprintln!("[TagsTabs] Toggled {} on group {}", flag_str, gid_val);
                            }
                        }
                    }
                }
                // --- Panel tree actions (ported from overlay_settings) ---
                id if id.starts_with("select:") => {
                    if let Ok(node_id) = id["select:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(node_id);
                        let leaf_id = zengeld_chart::LeafId(node_id);
                        if self.panel_app.panel_grid.docking().tree().leaf(leaf_id).is_some() {
                            self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        }
                    }
                }
                id if id.starts_with("minimap_leaf:") => {
                    if let Ok(leaf_id_val) = id["minimap_leaf:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(leaf_id_val);
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                id if id.starts_with("eliminate:") => {
                    if let Ok(leaf_id_val) = id["eliminate:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let removed = self.panel_app.panel_grid.close_leaf(leaf_id);
                        if removed {
                            if self.panel_app.overlay_settings_state.target_leaf_id == Some(leaf_id) {
                                self.panel_app.overlay_settings_state.target_leaf_id = None;
                                self.panel_app.overlay_settings_state.selected_node_id = None;
                            }
                            eprintln!("[TagsTabs] Eliminated leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("hide:") => {
                    if let Ok(leaf_id_val) = id["hide:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let hidden = self.panel_app.panel_grid.docking_mut().tree_mut().hide_leaf(leaf_id);
                        if hidden {
                            eprintln!("[TagsTabs] Hidden leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("show:") || id.starts_with("restore:") => {
                    let prefix = if id.starts_with("show:") { "show:" } else { "restore:" };
                    if let Ok(leaf_id_val) = id[prefix.len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.docking_mut().tree_mut().show_leaf(leaf_id);
                        eprintln!("[TagsTabs] Shown leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("expand:") => {
                    if let Ok(leaf_id_val) = id["expand:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.panel_app.panel_grid.toggle_expand();
                        eprintln!("[TagsTabs] Toggled expand for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("untag:") => {
                    if let Ok(leaf_id_val) = id["untag:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.perform_desync(leaf_id);
                        eprintln!("[TagsTabs] Untagged leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("split_h:") => {
                    if let Ok(leaf_id_val) = id["split_h:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.do_split(zengeld_chart::SplitKind::SplitRight);
                        eprintln!("[TagsTabs] Split horizontal for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("split_v:") => {
                    if let Ok(leaf_id_val) = id["split_v:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.do_split(zengeld_chart::SplitKind::SplitBottom);
                        eprintln!("[TagsTabs] Split vertical for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("tag:") => {
                    if let Ok(leaf_id_val) = id["tag:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
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
                    let name = self.wl_group_name_input.editing.text.trim().to_string();
                    if !name.is_empty() {
                        use zengeld_chart::ui::modal_settings::WatchlistGroupNameMode;
                        match self.wl_group_name_input.mode.clone() {
                            WatchlistGroupNameMode::CreateNew => {
                                let new_id = self.sidebar_state.watchlist_manager.create_list(name.clone());
                                self.sidebar_state.watchlist_manager.active_list_id = new_id;
                                self.watchlist_actions.push(crate::WatchlistAction::CreateList { name: name.clone() });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[WatchlistGroupName] created new list '{}' id={}", name, new_id);
                            }
                            WatchlistGroupNameMode::Rename(id) => {
                                if let Some(list) = self.sidebar_state.watchlist_manager.lists.iter_mut().find(|l| l.id == id) {
                                    list.name = name.clone();
                                    self.watchlist_actions.push(crate::WatchlistAction::RenameList { id, new_name: name.clone() });
                                    self.watchlists_dirty = true;
                                    self.persist_watchlists();
                                    eprintln!("[WatchlistGroupName] renamed list id={} to '{}'", id, name);
                                }
                            }
                        }
                    }
                    self.wl_group_name_input.close();
                }
                "cancel" | "close" => {
                    self.wl_group_name_input.close();
                    eprintln!("[WatchlistGroupName] cancelled");
                }
                "input" => {
                    // Click-to-cursor using pre-computed character positions
                    if let Some(ref gni) = self.last_wl_group_name_result {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &gni.char_x_positions,
                            x,
                        );
                        self.wl_group_name_input.editing.cursor = new_cursor;
                        self.wl_group_name_input.editing.selection_start = None;
                    }
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
            self.watchlist_modal.drag_reorder = None;
            self.watchlist_modal.drag_reorder_pending = None;
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.scale_settings.time_format.use_24h =
                        !window.scale_settings.time_format.use_24h;
                }
                // keep popup open when toggling 24h
            } else if item == "show_utc" {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.scale_settings.time_format.show_utc_prefix =
                        !window.scale_settings.time_format.show_utc_prefix;
                }
                // keep popup open
            } else if let Some(off) = item.strip_prefix("tz:") {
                if let Ok(offset) = off.parse::<i32>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
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
                    let name = self.panel_app.preset_name_input.name().to_string();
                    if !name.trim().is_empty() {
                        use zengeld_chart::ui::modal_settings::PresetNameInputMode;
                        match self.panel_app.preset_name_input.mode {
                            PresetNameInputMode::SaveAs => {
                                self.process_chart_out_event(
                                    zengeld_chart::events::ChartOutEvent::SavePreset { name }
                                );
                            }
                            PresetNameInputMode::Rename => {
                                let id = self.panel_app.preset_name_input.rename_preset_id
                                    .clone().unwrap_or_default();
                                self.process_chart_out_event(
                                    zengeld_chart::events::ChartOutEvent::RenamePreset { id, new_name: name }
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
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] preset name input committed via Save button");
                }
                "cancel" => {
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] preset name input closed via Cancel button");
                }
                "close" => {
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] preset name input closed via X");
                }
                "input" => {
                    // Click-to-cursor using pre-computed character positions
                    if let Some(pni) = self.frame_result.as_ref().and_then(|r| r.preset_name_input.as_ref()) {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &pni.char_x_positions,
                            x,
                        );
                        self.panel_app.preset_name_input.editing.cursor = new_cursor;
                        self.panel_app.preset_name_input.editing.selection_start = None;
                    }
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
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                eprintln!("[ChartApp] chart browser closed via X");
            } else if action == "modal_bg" {
                // Absorb click — no-op.
            } else if action == "search_input" {
                // Click in search input — position cursor using char positions.
                let char_positions = self.frame_result.as_ref()
                    .and_then(|r| r.chart_browser.as_ref())
                    .map(|br| br.search_char_positions.clone());
                if let Some(positions) = char_positions {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&positions, x);
                    self.panel_app.chart_browser.search_editing.cursor = new_cursor;
                    self.panel_app.chart_browser.search_editing.selection_start = None;
                    self.panel_app.chart_browser.search_editing.reset_blink(0);
                }
            } else if let Some(preset_id) = action.strip_prefix("rename:") {
                // Click rename icon — close browser, open rename modal for this preset.
                let preset_id = preset_id.to_string();
                let name_opt = self.panel_app.presets.get(&preset_id).map(|p| p.name.clone());
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                if let Some(name) = name_opt {
                    self.panel_app.preset_name_input.open_rename(&preset_id, &name, 0);
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::PresetNameInput;
                    eprintln!("[ChartApp] chart browser: rename preset '{}'", preset_id);
                }
            } else if let Some(preset_id) = action.strip_prefix("delete:") {
                // Click delete icon — delete preset.
                let preset_id = preset_id.to_string();
                eprintln!("[ChartApp] chart browser: delete preset '{}'", preset_id);
                self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::DeletePreset { id: preset_id });
            } else if let Some(preset_id) = action.strip_prefix("item:") {
                // Click on preset row — load preset and close browser.
                // Read the flag BEFORE close() resets it.
                let open_in_new_tab = self.panel_app.chart_browser.open_in_new_tab;
                let preset_id = preset_id.to_string();
                eprintln!("[ChartApp] chart browser: {} preset '{}' (new_tab={})",
                    if open_in_new_tab { "open tab" } else { "load" }, preset_id, open_in_new_tab);
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                if open_in_new_tab {
                    self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::OpenTab { id: preset_id });
                } else {
                    self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::LoadPreset { id: preset_id });
                }
            } else {
                eprintln!("[ChartApp] chart_browser unhandled action: {}", action);
            }
            return;
        }

        // === User settings modal clicks ===
        if let Some(action) = widget_id.strip_prefix("user_settings:") {
            use zengeld_chart::ui::modal_settings::UserSettingsTab;
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
                    self.indicator_manager.recalc_mode = match mode_str {
                        "PerTick" => RecalcMode::PerTick,
                        "PerBar"  => RecalcMode::PerBar,
                        _         => RecalcMode::PerFrame,
                    };
                    self.recalc_mode_changed = Some(mode_str.to_string());
                    // Reset counters and clear stale dirty flags when mode changes.
                    self.recalc_count = 0;
                    self.trade_count = 0;
                    self.recalc_log_timer = std::time::Instant::now();
                    self.indicator_manager.clear_pending();
                    eprintln!("[ChartApp] user_settings recalc_mode set to: {}", mode_str);
                }
                // Language radio option selected — hit id: "language:{code}"
                rest if rest.starts_with("language:") => {
                    let lang_code = &rest["language:".len()..];
                    let lang = match lang_code {
                        "ru" => crate::Language::Ru,
                        _    => crate::Language::En,
                    };
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
                "e2e_passphrase_input" => {
                    self.panel_app.user_settings_state.new_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    // Position cursor at click point using pre-computed char positions.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "e2e_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] e2e_passphrase_input: focused");
                }
                "show_wizard" => {
                    self.panel_app.user_settings_state.show_welcome_wizard = true;
                    self.panel_app.user_settings_state.wizard_page = 0;
                    // Clear leaked state from profile_manager
                    self.panel_app.user_settings_state.confirm_passphrase_editing.clear();
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_editing.clear();
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
                "wizard_lang_en" => {
                    crate::set_language(crate::Language::En);
                    eprintln!("[ChartApp] wizard: language set to English");
                }
                "wizard_lang_ru" => {
                    crate::set_language(crate::Language::Ru);
                    eprintln!("[ChartApp] wizard: language set to Russian");
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
                    // Clear selections on other fields
                    self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                    self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                    // Position cursor at click point using pre-computed char positions.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "wizard_profile_name_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.new_profile_name_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] wizard: profile name input focused");
                }
                "wizard_passphrase_input" => {
                    // Focus the passphrase input on the profile page
                    self.panel_app.user_settings_state.new_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    // Clear selections on other fields
                    self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                    self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                    // Position cursor at click point using pre-computed char positions.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "wizard_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] wizard: passphrase input focused");
                }
                "wizard_confirm_passphrase_input" => {
                    // Focus the confirm passphrase input on wizard page 2.
                    self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    // Clear selections on other fields
                    self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                    self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "wizard_confirm_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] wizard: confirm_passphrase_input focused");
                }
                "wizard_finish" => {
                    // Page 2: submit passphrase (and log profile name) then close wizard.
                    let passphrase = self.panel_app.user_settings_state.new_passphrase_editing.text.clone();
                    let profile_name = self.panel_app.user_settings_state.new_profile_name_editing.text.trim().to_string();
                    if passphrase.len() >= zengeld_chart::MIN_PASSPHRASE_LENGTH && !profile_name.is_empty() {
                        eprintln!("[ChartApp] wizard: profile name = {:?}", profile_name);
                        let profile_name_final = if profile_name.is_empty() { "Default".to_string() } else { profile_name.clone() };
                        self.pending_updater_cmd = Some(format!("wizard_complete:{}:{}", profile_name_final, passphrase));
                        self.panel_app.user_settings_state.show_welcome_wizard = false;
                        self.panel_app.user_settings_state.needs_vault_unlock = false;
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                        self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                        self.panel_app.user_settings_state.confirm_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = 0;
                        self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                        self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                        eprintln!("[ChartApp] wizard: setup complete");
                    }
                }
                "wizard_copy_key" => {
                    // Page 3: copy recovery key to clipboard
                    if let Some(ref key) = self.panel_app.user_settings_state.recovery_key_display {
                        self.clipboard_text = Some(key.clone());
                        self.panel_app.user_settings_state.recovery_key_copied_at =
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
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
                    self.panel_app.user_settings_state.recovery_key_display_editing.text.clear();
                    self.panel_app.user_settings_state.recovery_key_display_editing.cursor = 0;
                    self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    eprintln!("[ChartApp] wizard: recovery key confirmed, closing wizard");
                }
                "profile_mgr:run_wizard" => {
                    self.panel_app.user_settings_state.show_welcome_wizard = true;
                    self.panel_app.user_settings_state.show_profile_manager = false;
                    self.panel_app.user_settings_state.wizard_page = 0;
                    // Clear leaked state from profile_manager
                    self.panel_app.user_settings_state.confirm_passphrase_editing.clear();
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_editing.clear();
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
                    let passphrase = self.panel_app.user_settings_state.new_passphrase_editing.text.clone();
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
                        zengeld_chart::ui::modal_settings::ProfileManagerPage::ProfileList;
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                    eprintln!("[ChartApp] profile_mgr: close button clicked, dismissing");
                }
                "profile_mgr:back" => {
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;
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
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                        self.panel_app.user_settings_state.new_passphrase_focused = false;
                        eprintln!("[ChartApp] profile_mgr: back to profile list");
                    }
                }
                "profile_mgr:create_new" => {
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;
                    self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreateNew;
                    self.panel_app.user_settings_state.new_profile_name_editing.text.clear();
                    self.panel_app.user_settings_state.new_profile_name_editing.cursor = 0;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    eprintln!("[ChartApp] profile_mgr: create new profile page");
                }
                "profile_mgr:unlock" => {
                    let passphrase = self.panel_app.user_settings_state.new_passphrase_editing.text.clone();
                    if !passphrase.is_empty() {
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.pending_updater_cmd = Some(format!("e2e_setup:{}", passphrase));
                        eprintln!("[ChartApp] profile_mgr: unlock passphrase submitted");
                    }
                }
                "profile_mgr:create_passphrase" => {
                    let passphrase = self.panel_app.user_settings_state.new_passphrase_editing.text.clone();
                    let confirm = self.panel_app.user_settings_state.confirm_passphrase_editing.text.clone();
                    if passphrase.len() >= zengeld_chart::MIN_PASSPHRASE_LENGTH && passphrase == confirm {
                        self.panel_app.user_settings_state.confirm_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = 0;
                        self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                        self.pending_updater_cmd = Some(format!("e2e_setup:{}", passphrase));
                        eprintln!("[ChartApp] profile_mgr: create passphrase submitted");
                    }
                }
                "profile_mgr:create_confirm" => {
                    let name = self.panel_app.user_settings_state.new_profile_name_editing.text.trim().to_string();
                    if !name.is_empty() {
                        self.pending_updater_cmd = Some(format!("profile_create:{}", name));
                        eprintln!("[ChartApp] profile_mgr: creating profile '{}'", name);
                    }
                }
                "profile_mgr:name_input" => {
                    self.panel_app.user_settings_state.new_profile_name_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:name_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.new_profile_name_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] profile_mgr: name input focused");
                }
                "profile_mgr:use_recovery_key" => {
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;
                    self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UseRecoveryKey;
                    self.panel_app.user_settings_state.recovery_key_display_editing.text.clear();
                    self.panel_app.user_settings_state.recovery_key_display_editing.cursor = 0;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    eprintln!("[ChartApp] profile_mgr: use recovery key page");
                }
                "profile_mgr:recovery_key_input" => {
                    self.panel_app.user_settings_state.recovery_key_display_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:recovery_key_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.recovery_key_display_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] profile_mgr: recovery key input focused");
                }
                "profile_mgr:new_passphrase_input" => {
                    self.panel_app.user_settings_state.new_passphrase_focused = true;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:new_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] profile_mgr: new_passphrase_input focused");
                }
                "profile_mgr:confirm_passphrase_input" => {
                    self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    // Position cursor at click point.
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:confirm_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] profile_mgr: confirm_passphrase_input focused");
                }
                "profile_mgr:create_confirm_passphrase_input" => {
                    // Confirm passphrase field on the CreatePassphrase page.
                    self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:create_confirm_passphrase_input"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                    }
                    eprintln!("[ChartApp] profile_mgr: create_confirm_passphrase_input focused");
                }
                "profile_mgr:save_new_passphrase" => {
                    let passphrase_text = self.panel_app.user_settings_state.new_passphrase_editing.text.clone();
                    let confirm_text = self.panel_app.user_settings_state.confirm_passphrase_editing.text.clone();
                    use zengeld_chart::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH;
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
                    let recovery_key_text = self.panel_app.user_settings_state.recovery_key_display_editing.text.clone();
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
                    // Sync the display editing state text in case it hasn't been set yet.
                    if self.panel_app.user_settings_state.recovery_key_display_editing.text != key_text {
                        self.panel_app.user_settings_state.recovery_key_display_editing.text = key_text.clone();
                        self.panel_app.user_settings_state.recovery_key_display_editing.cursor = key_text.chars().count();
                    }
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.user_settings.as_ref())
                        .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == "profile_mgr:recovery_key_display"))
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        self.panel_app.user_settings_state.recovery_key_display_editing.cursor = new_cursor;
                        self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
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
                    let new_name = self.panel_app.user_settings_state.profile_rename_editing.text.trim().to_string();
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
                    self.panel_app.user_settings_state.profile_rename_editing.text.clear();
                    self.panel_app.user_settings_state.profile_rename_editing.cursor = 0;
                    self.panel_app.user_settings_state.profile_rename_target_id = None;
                    eprintln!("[ChartApp] profile_rename_cancel");
                }
                "profile_new" => {
                    let uss = &mut self.panel_app.user_settings_state;
                    uss.show_new_profile_dialog = true;
                    uss.new_profile_name_editing.text = String::new();
                    uss.new_profile_name_editing.cursor = 0;
                    uss.new_profile_name_editing.selection_start = None;
                    uss.new_profile_name_focused = true;
                    eprintln!("[ChartApp] profile_new: opening dialog");
                }
                "profile_new_confirm" => {
                    let name = self.panel_app.user_settings_state.new_profile_name_editing.text.trim().to_string();
                    if !name.is_empty() {
                        self.pending_updater_cmd = Some(format!("profile_create:{}", name));
                        self.panel_app.user_settings_state.show_new_profile_dialog = false;
                        self.panel_app.user_settings_state.new_profile_name_focused = false;
                        self.panel_app.user_settings_state.new_profile_name_editing.text.clear();
                        self.panel_app.user_settings_state.new_profile_name_editing.cursor = 0;
                        eprintln!("[ChartApp] profile_new_confirm: creating profile '{}'", name);
                    }
                }
                "profile_new_cancel" => {
                    self.panel_app.user_settings_state.show_new_profile_dialog = false;
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.new_profile_name_editing.text.clear();
                    self.panel_app.user_settings_state.new_profile_name_editing.cursor = 0;
                    eprintln!("[ChartApp] profile_new_cancel");
                }
                rest if rest.starts_with("vault_picker_profile:") => {
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;
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
                    if !target_has_vault {
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                        eprintln!("[ChartApp] vault_picker: passphrase setup for unencrypted profile {}", profile_id);
                    } else {
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.panel_app.user_settings_state.vault_unlock_attempts = 0;
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                        eprintln!("[ChartApp] vault_picker: passphrase prompt for profile {}", profile_id);
                    }
                    // Switch to profile_manager modal to show the passphrase page
                    self.panel_app.user_settings_state.show_profile_manager = true;
                    self.panel_app.user_settings_state.show_welcome_wizard = false;
                    // Clear leaked state from wizard
                    self.panel_app.user_settings_state.confirm_passphrase_editing.clear();
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    self.panel_app.user_settings_state.new_passphrase_editing.clear();
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                }
                rest if rest.starts_with("profile_mgr:select:") => {
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;
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
                    if !target_has_vault {
                        // Unencrypted profile — show CreatePassphrase inline (NO hot-reload)
                        // After passphrase is set, main.rs will create vault in target dir then switch
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                        eprintln!("[ChartApp] profile_mgr: showing passphrase setup for unencrypted profile {}", profile_id);
                    } else if profile_id == self.panel_app.user_settings_state.runtime_profile_id {
                        if self.panel_app.user_settings_state.needs_vault_unlock {
                            // Current profile, vault locked — show unlock passphrase page
                            self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::UnlockPassphrase;
                            self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                            self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                            self.panel_app.user_settings_state.vault_unlock_error = None;
                            self.panel_app.user_settings_state.vault_unlock_attempts = 0;
                            self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                            self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
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
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                        eprintln!("[ChartApp] profile_mgr: showing passphrase prompt for profile {}", profile_id);
                    }
                }
                rest if rest.starts_with("profile_rename:") => {
                    let id = &rest["profile_rename:".len()..];
                    let uss = &mut self.panel_app.user_settings_state;
                    // Find the display name for this profile to pre-fill the input
                    let current_name = uss.available_profiles.iter()
                        .find(|(pid, _, _)| pid == id)
                        .map(|(_, name, _)| name.clone())
                        .unwrap_or_else(|| uss.profile_display_name.clone());
                    let cursor = current_name.chars().count();
                    uss.profile_rename_mode = true;
                    uss.profile_rename_editing.text = current_name;
                    uss.profile_rename_editing.cursor = cursor;
                    uss.profile_rename_editing.selection_start = None;
                    uss.profile_rename_focused = true;
                    uss.profile_rename_target_id = Some(id.to_string());
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
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;
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
                    } else if !target_has_vault {
                        // Unencrypted — show CreatePassphrase inline
                        self.panel_app.user_settings_state.show_profile_manager = true;
                        self.panel_app.user_settings_state.is_open = false;
                        self.panel_app.user_settings_state.profile_manager_page = ProfileManagerPage::CreatePassphrase;
                        self.panel_app.user_settings_state.profile_manager_target_id = profile_id.to_string();
                        self.panel_app.user_settings_state.profile_manager_target_name = target_name;
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
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
                        self.panel_app.user_settings_state.new_passphrase_editing.text.clear();
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = 0;
                        eprintln!("[ChartApp] profile_switch: passphrase prompt for {}", profile_id);
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
                            self.panel_app.open_tags_tabs();
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
                    let leaf_id = zengeld_chart::LeafId(lid);
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

}
