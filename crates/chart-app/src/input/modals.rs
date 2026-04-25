//! Modal and search routing: watchlist modal, search overlay, context menu,
//! inline actions, indicator set deploy/create, canvas click (drawing tool placement),
//! modal layer dismissal.

use crate::ChartApp;
use zengeld_chart::{
    ChartInputAction,
    ChartOutputAction,
    ExtendedLayoutHitTester,
    ScaleCornerButton,
    input::MouseButton,
    ScaleMode,
};
use zengeld_chart::ui::context_menu::ContextMenuTarget;
use zengeld_chart::ui::modal_state::{OpenModal, IndicatorCategoryFilter};

impl ChartApp {
    /// Handle a click on an inline primitive toolbar action.
    ///
    /// These actions are generated when a drawing primitive is selected and the
    /// user clicks one of the compact action buttons rendered inline in the
    /// control strip (`inline:settings`, `inline:color`, `inline:delete`, …).
    pub(super) fn handle_inline_action(&mut self, action: &str) {
        use zengeld_chart::drawing::primitives_v2::LineStyle;

        // The selected primitive index comes from the drawing manager, not from
        // a settings-state field, because the primitive may not be open in a
        // modal yet.
        let selected_idx = self.panel_app.panel_grid
            .active_window()
            .and_then(|w| w.drawing_manager.selected());

        let idx = match selected_idx {
            Some(i) => i,
            None => {
                eprintln!("[ChartApp] inline action '{}' ignored — no primitive selected", action);
                return;
            }
        };

        let screen_w = self.width as f64;
        let screen_h = self.height as f64;

        match action {
            // ── Settings ─────────────────────────────────────────────────────
            "inline:settings" => {
                self.panel_app.primitive_settings_state.open(idx);
                eprintln!("[ChartApp] inline: opened primitive settings for #{}", idx);
            }

            // ── Stroke color ─────────────────────────────────────────────────
            "inline:color" => {
                let current_color = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx))
                    .map(|d| d.color.stroke.clone());

                // Look up the button rect from the input coordinator so the
                // color picker can be anchored beneath the clicked button.
                let widget_id_str = "ilb:inline:color".to_string();
                let rect = self.input_coordinator.borrow_mut().widget_rect(
                    &uzor::input::WidgetId(widget_id_str),
                );
                let (ax, ay, aw, ah) = rect
                    .map(|r| (r.x, r.y, r.width, r.height))
                    .unwrap_or((0.0, 0.0, 0.0, 0.0));

                self.panel_app.primitive_settings_state.open_color_picker_smart(
                    "stroke_color", ax, ay, aw, ah, screen_w, screen_h,
                    current_color.as_deref(),
                );
                eprintln!("[ChartApp] inline: opened stroke color picker for #{}", idx);
            }

            // ── Text color ───────────────────────────────────────────────────
            "inline:text_color" => {
                let current_color = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx))
                    .and_then(|d| d.text.and_then(|t| t.color));

                let widget_id_str = "ilb:inline:text_color".to_string();
                let rect = self.input_coordinator.borrow_mut().widget_rect(
                    &uzor::input::WidgetId(widget_id_str),
                );
                let (ax, ay, aw, ah) = rect
                    .map(|r| (r.x, r.y, r.width, r.height))
                    .unwrap_or((0.0, 0.0, 0.0, 0.0));

                self.panel_app.primitive_settings_state.open_color_picker_smart(
                    "text_color", ax, ay, aw, ah, screen_w, screen_h,
                    current_color.as_deref(),
                );
                eprintln!("[ChartApp] inline: opened text color picker for #{}", idx);
            }

            // ── Delete ───────────────────────────────────────────────────────
            "inline:delete" => {
                // Capture the primitive ID before deletion so alerts can be cleaned up.
                let prim_id = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx))
                    .map(|d| d.id);
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.delete_selected();
                    eprintln!("[ChartApp] inline: deleted selected primitive");
                }
                self.sync_drawing_back_to_group();
                if let Some(pid) = prim_id {
                    self.alert_manager.remove_alerts_for_drawing(pid);
                }
                self.autosave_snapshot();
            }

            // ── Lock toggle ──────────────────────────────────────────────────
            "inline:lock" => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.toggle_selected_lock();
                    eprintln!("[ChartApp] inline: toggled lock on selected primitive");
                }
                self.sync_drawing_back_to_group();
            }

            // ── Line style cycle ─────────────────────────────────────────────
            "inline:style" => {
                let data_opt = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx));

                if let Some(mut data) = data_opt {
                    data.style = match data.style {
                        LineStyle::Solid        => LineStyle::Dashed,
                        LineStyle::Dashed       => LineStyle::Dotted,
                        LineStyle::Dotted       => LineStyle::LargeDashed,
                        LineStyle::LargeDashed  => LineStyle::SparseDotted,
                        LineStyle::SparseDotted => LineStyle::Solid,
                    };
                    let style_str = data.style.as_str().to_string();
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] inline: cycled line style to {}", style_str);
                }
                self.snapshot_primitive_settings_to_user_manager(idx);
            }

            // ── Width cycle ──────────────────────────────────────────────────
            "inline:width" => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.increase_selected_width();
                    eprintln!("[ChartApp] inline: increased line width");
                }
                self.sync_drawing_back_to_group();
                self.snapshot_primitive_settings_to_user_manager(idx);
            }

            // ── Name label — absorb, no action ──────────────────────────────
            "inline:name" => {
                eprintln!("[ChartApp] inline: name label clicked (no-op)");
            }

            // ── Alert — open alert settings modal for selected primitive ────
            "inline:alert" => {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(prim) = window.drawing_manager.selected_primitive() {
                        let prim_id = prim.data().id;
                        let label = prim.display_name().to_string();
                        let pts = prim.points();
                        let price = if !pts.is_empty() {
                            pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64
                        } else {
                            window.bars.last().map(|b| b.close).unwrap_or(0.0)
                        };
                        let symbol = window.symbol.clone();
                        let source = alerts::AlertSource::Drawing { primitive_id: prim_id, label };
                        self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                        self.panel_app.alert_settings_state.pin_initial_position(
                            self.content_rect.width, self.content_rect.height,
                        );
                        eprintln!("[ChartApp] inline alert: opened modal for primitive {}", prim_id);
                    }
                }
            }

            // ── More — not yet implemented ───────────────────────────────────
            "inline:more" => {
                eprintln!("[ChartApp] inline: 'inline:more' not yet implemented");
            }

            // ── Style chevron — toggle style dropdown ────────────────────────
            "inline:style_menu" => {
                let was_open = self.panel_app.toolbar_state.open_inline_style_dropdown;
                // Close both dropdowns first, then toggle
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
                if !was_open {
                    self.panel_app.toolbar_state.open_inline_style_dropdown = true;
                }
                eprintln!("[ChartApp] inline: style dropdown toggled to {}", self.panel_app.toolbar_state.open_inline_style_dropdown);
            }

            // ── Width chevron — toggle width dropdown ────────────────────────
            "inline:width_menu" => {
                let was_open = self.panel_app.toolbar_state.open_inline_width_dropdown;
                // Close both dropdowns first, then toggle
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
                if !was_open {
                    self.panel_app.toolbar_state.open_inline_width_dropdown = true;
                }
                eprintln!("[ChartApp] inline: width dropdown toggled to {}", self.panel_app.toolbar_state.open_inline_width_dropdown);
            }

            // ── Background absorber — close inline dropdown ──────────────────
            "inline_dropdown:__bg__" => {
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ if action.starts_with("inline:style_option:") => {
                // ── Style option selected ────────────────────────────────────
                use zengeld_chart::drawing::primitives_v2::LineStyle;
                let value = action.strip_prefix("inline:style_option:").unwrap_or("");
                let new_style = match value {
                    "solid"        => Some(LineStyle::Solid),
                    "dashed"       => Some(LineStyle::Dashed),
                    "dotted"       => Some(LineStyle::Dotted),
                    "large_dashed" => Some(LineStyle::LargeDashed),
                    "sparse_dotted" => Some(LineStyle::SparseDotted),
                    _ => None,
                };
                if let Some(style) = new_style {
                    if let Some(mut data) = self.panel_app.panel_grid
                        .active_window()
                        .and_then(|w| w.drawing_manager.get_data_at(idx))
                    {
                        data.style = style;
                        let style_str = data.style.as_str().to_string();
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.set_data_at(idx, &data);
                        }
                        self.sync_drawing_back_to_group();
                        eprintln!("[ChartApp] inline: set line style to {}", style_str);
                    }
                }
                self.snapshot_primitive_settings_to_user_manager(idx);
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ if action.starts_with("inline:width_option:") => {
                // ── Width option selected ────────────────────────────────────
                let value = action.strip_prefix("inline:width_option:").unwrap_or("");
                if let Ok(new_width) = value.parse::<f64>() {
                    if let Some(mut data) = self.panel_app.panel_grid
                        .active_window()
                        .and_then(|w| w.drawing_manager.get_data_at(idx))
                    {
                        data.width = new_width;
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.set_data_at(idx, &data);
                        }
                        self.sync_drawing_back_to_group();
                        eprintln!("[ChartApp] inline: set line width to {}", new_width);
                    }
                }
                self.snapshot_primitive_settings_to_user_manager(idx);
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ => {
                eprintln!("[ChartApp] inline: unhandled action '{}'", action);
            }
        }
    }

    /// Handle clicks on widgets registered with the "wl_modal:" prefix.
    pub(super) fn handle_watchlist_modal_click(&mut self, rest: &str, x: f64, _y: f64) {
        match rest {
            "close" => {
                self.watchlist_modal.close();
                eprintln!("[WatchlistModal] closed");
            }
            "modal_bg" | "header_drag" | "list_scroll" => {
                // Absorbed — no-op for backdrop clicks, drag zone, scroll zone
            }
            "search_input" => {
                // Click-to-cursor using pre-computed character positions
                if let Some(ref wl) = self.last_watchlist_modal_result {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                        &wl.search_char_positions,
                        x,
                    );
                    self.watchlist_modal.search_editing.cursor = new_cursor;
                    self.watchlist_modal.search_editing.selection_start = None;
                    self.watchlist_modal.search_editing.reset_blink(0);
                }
            }
            _ if rest.starts_with("tab:") => {
                use zengeld_chart::ui::modal_settings::WatchlistModalTab;
                let tab = &rest["tab:".len()..];
                match tab {
                    "overview" => self.watchlist_modal.active_tab = WatchlistModalTab::Overview,
                    "groups"   => self.watchlist_modal.active_tab = WatchlistModalTab::Groups,
                    "settings" => self.watchlist_modal.active_tab = WatchlistModalTab::Settings,
                    _ => {}
                }
                eprintln!("[WatchlistModal] tab: {}", tab);
            }
            _ if rest.starts_with("item:") => {
                let composite = &rest["item:".len()..];
                // Parse composite key "SYMBOL:exchange:account_type" — split from the
                // right twice so that symbols containing ':' are handled correctly.
                let (sym_part, exchange_part, wl_at_label) =
                    if let Some(at_pos) = composite.rfind(':') {
                        let at_part = &composite[at_pos + 1..];
                        let remainder = &composite[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (
                                &remainder[..ex_pos],
                                &remainder[ex_pos + 1..],
                                at_part.to_string(),
                            )
                        } else {
                            // Only one colon — treat as "SYMBOL:exchange", look up account_type.
                            let at = self.sidebar_state.watchlist_manager.active_list()
                                .and_then(|list| {
                                    list.all_symbols().iter()
                                        .find(|ws| ws.symbol == remainder && ws.exchange == at_part)
                                        .map(|ws| ws.account_type.clone())
                                })
                                .unwrap_or_else(|| "S".to_string());
                            (remainder, at_part, at)
                        }
                    } else {
                        (composite, self.active_exchange.as_str(), "S".to_string())
                    };
                // Resolve ExchangeId from string.
                let resolved_exchange = self.exchange_symbols
                    .keys()
                    .find(|eid| eid.as_str() == exchange_part)
                    .copied()
                    .unwrap_or(self.active_exchange);
                // Switch the active chart to this symbol+exchange.
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    let old_sym = window.symbol.clone();
                    let old_exchange = window.exchange.clone();
                    let old_account_type = window.account_type.clone();
                    window.snapshot_drawings_for_symbol(&old_sym, &old_exchange, &old_account_type);
                    window.symbol = sym_part.to_string();
                    window.exchange = exchange_part.to_string();
                    window.account_type = wl_at_label.clone();
                    window.update_title();
                    window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
                    window.bars.clear();
                    window.viewport.bar_count = 0;
                    window.pending_symbol_load = true;
                    window.drawing_manager.clear_all_primitives();
                    window.restore_drawings_for_symbol(sym_part, exchange_part, &wl_at_label);
                }
                self.active_exchange = resolved_exchange;
                // Unsubscribe only the old symbol's trade stream, leaving other
                // windows' streams intact.
                if !old_trade_symbol.is_empty() {
                    self.bridge.unsubscribe_trades(old_trade_exchange, &old_trade_symbol, old_trade_at);
                }
                let eid_str = resolved_exchange.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (watchlist modal item click)", eid_str);
                } else {
                    let wl_at = crate::account_type_from_label(&wl_at_label);
                    self.bridge.ensure_connector(resolved_exchange);
                    self.bridge.request_bars(resolved_exchange, sym_part, &timeframe, wl_at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), false);
                }
                self.autosave_snapshot();
                eprintln!("[WatchlistModal] symbol selected: {} @ {}", sym_part, exchange_part);
                self.watchlist_modal.close();
            }
            _ if rest.starts_with("delete:") => {
                let key = &rest["delete:".len()..];
                // Parse composite key "SYMBOL:exchange:account_type" — split from the
                // right twice so that symbols containing ':' are handled correctly.
                let (symbol, exchange_owned, account_type_owned): (&str, String, String) =
                    if let Some(at_pos) = key.rfind(':') {
                        let at_part = &key[at_pos + 1..];
                        let remainder = &key[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (
                                &remainder[..ex_pos],
                                remainder[ex_pos + 1..].to_string(),
                                at_part.to_string(),
                            )
                        } else {
                            // Only one colon — "SYMBOL:exchange", look up account_type.
                            let at = self.sidebar_state.watchlist_manager.active_list()
                                .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == remainder && ws.exchange == at_part).map(|ws| ws.account_type.clone()))
                                .unwrap_or_default();
                            (remainder, at_part.to_string(), at)
                        }
                    } else {
                        // No colon — plain symbol, look up exchange + account_type.
                        let ex = self.sidebar_state.watchlist_manager.active_list()
                            .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == key).map(|ws| ws.exchange.clone()))
                            .unwrap_or_else(|| self.active_exchange.as_str().to_string());
                        let at = self.sidebar_state.watchlist_manager.active_list()
                            .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == key).map(|ws| ws.account_type.clone()))
                            .unwrap_or_default();
                        (key, ex, at)
                    };
                let exchange = exchange_owned.as_str();
                let account_type = account_type_owned.as_str();
                // Remove symbol from snapshot (if active) before removing from list.
                if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                    if let Some(ref mut snap) = list.order_snapshot {
                        snap.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == account_type));
                    }
                }
                self.sidebar_state.watchlist_manager.remove_symbol(symbol, exchange, account_type);
                self.watchlist_actions.push(crate::WatchlistAction::Remove { symbol: symbol.to_string(), exchange: exchange.to_string(), account_type: account_type.to_string() });
                self.watchlists_dirty = true;
                self.persist_watchlists();
                eprintln!("[WatchlistModal] symbol removed: {}:{}:{}", symbol, exchange, account_type);
            }
            // Switch active watchlist
            _ if rest.starts_with("group:") => {
                let id_str = &rest["group:".len()..];
                if let Ok(id) = id_str.parse::<u64>() {
                    self.sidebar_state.watchlist_manager.active_list_id = id;
                    self.watchlist_actions.push(crate::WatchlistAction::SetActiveList { id });
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[WatchlistModal] switched active list to id={}", id);
                }
            }
            // Delete a watchlist group (list)
            _ if rest.starts_with("group_delete:") => {
                let id_str = &rest["group_delete:".len()..];
                if let Ok(id) = id_str.parse::<u64>() {
                    let deleted = self.sidebar_state.watchlist_manager.delete_list(id);
                    self.watchlist_actions.push(crate::WatchlistAction::DeleteList { id });
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[WatchlistModal] delete list id={} -> {}", id, deleted);
                }
            }
            // Create new watchlist — open name input modal with auto-generated name
            "group_add" => {
                let max_n = self.sidebar_state.watchlist_manager.lists.iter()
                    .filter_map(|l| l.name.strip_prefix("Watchlist "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Watchlist {}", max_n + 1);
                self.wl_group_name_input.open_create_new_with_name(&default_name);
                eprintln!("[WatchlistModal] opening group name input for create new: '{}'", default_name);
            }
            // Rename a watchlist — open name input modal pre-filled with current name
            _ if rest.starts_with("group_rename:") => {
                let id_str = &rest["group_rename:".len()..];
                if let Ok(id) = id_str.parse::<u64>() {
                    if let Some(list) = self.sidebar_state.watchlist_manager.lists.iter().find(|l| l.id == id) {
                        let name = list.name.clone();
                        self.wl_group_name_input.open_rename(id, &name);
                        eprintln!("[WatchlistModal] opening group name input for rename id={}", id);
                    }
                }
            }
            _ => {
                eprintln!("[WatchlistModal] unhandled: {}", rest);
            }
        }
    }

    /// Handle actions from the indicator search sidebar: sets tab, deploy set,
    /// and create set.
    ///
    /// Widget IDs handled:
    /// - `"sets_tab"` — toggle the Indicator Sets view
    /// - `"set:{id}"` — deploy the named set (create all its indicators)
    /// - `"set_create"` — open name-input modal to save current indicators as a set
    pub(super) fn handle_indicator_search_action(&mut self, rest: &str) {
        match rest {
            "sets_tab" => {
                self.modal_state.toggle_indicator_sets();
                eprintln!("[ChartApp] ind_search: toggled sets view (now={})", self.modal_state.show_indicator_sets);
            }
            "set_create" => {
                // Compute next available name: "My Indicator Set 1", "My Indicator Set 2", etc.
                let max_n = self.panel_app.template_manager.indicator_sets
                    .iter()
                    .filter_map(|s| s.name.strip_prefix("My Indicator Set "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("My Indicator Set {}", max_n + 1);

                let current_time_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                self.panel_app.preset_name_input.open_create_indicator_set(
                    &default_name,
                    current_time_ms,
                );
                eprintln!("[ChartApp] ind_search: opening create indicator set modal");
            }
            _ if rest.starts_with("set_delete:") => {
                let set_id = &rest["set_delete:".len()..];
                eprintln!("[ChartApp] deleted indicator set: {}", set_id);
                self.template_actions.push(crate::TemplateAction::RemoveIndicatorSet { id: set_id.to_string() });
            }
            _ if rest.starts_with("set:") => {
                let set_id = &rest["set:".len()..];
                self.deploy_indicator_set(set_id);
            }
            _ => {
                eprintln!("[ChartApp] ind_search: unhandled action: {}", rest);
            }
        }
    }

    /// Deploy an indicator set by its ID: replace all current indicators with
    /// the set's indicators (exclusive — like a preset, only one set active).
    pub(super) fn deploy_indicator_set(&mut self, set_id: &str) {
        let set = self.panel_app.template_manager.indicator_sets
            .iter()
            .find(|s| s.id == set_id)
            .cloned();

        let Some(set) = set else {
            eprintln!("[ChartApp] deploy_indicator_set: set not found: {}", set_id);
            return;
        };

        let symbol = self.panel_app.panel_grid.active_window()
            .map(|w| w.symbol.clone())
            .unwrap_or_default();

        // Remove all existing indicators (exclusive deployment, like preset)
        let existing_ids: Vec<u64> = self.indicator_manager
            .get_instances_for_symbol(&symbol)
            .iter()
            .map(|inst| inst.id)
            .collect();
        for id in &existing_ids {
            self.indicator_manager.remove_instance(*id);
        }
        // Clear orphan pre_tag_indicator_ids for every window so the Object Tree
        // does not show phantom items after a bulk indicator replace.
        for window in self.panel_app.panel_grid.windows_mut().values_mut() {
            window.pre_tag_indicator_ids.retain(|iid| !existing_ids.contains(iid));
        }
        // Clear stale overlay states
        self.panel_app.indicator_overlay_states.clear();
        eprintln!("[ChartApp] deploy_indicator_set: removed {} existing indicators", existing_ids.len());

        // Create new indicators from the set with full params/outputs
        for tmpl in &set.indicators {
            if let Some(new_id) = self.indicator_manager.create_instance(&tmpl.type_id, &symbol) {
                if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                        if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                            inst.window_id = Some(chart_id.0);
                        }
                    }
                }
                // Apply saved params, output styles, and visibility from the template
                if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                    // Restore instance-level visibility
                    inst.visible = tmpl.visible;
                    // Restore params
                    for (k, v) in &tmpl.params {
                        if let Ok(iv) = serde_json::from_value(v.clone()) {
                            inst.params.insert(k.clone(), iv);
                        }
                    }
                    // Restore output styles
                    for (key, out_style) in &tmpl.outputs {
                        let entry = inst.outputs.entry(key.clone())
                            .or_insert_with(Default::default);
                        if let Some(ref c) = out_style.color {
                            entry.color = Some(c.clone());
                        }
                        if let Some(lw) = out_style.line_width {
                            entry.line_width = Some(lw);
                        }
                        if let Some(vis) = out_style.visible {
                            entry.visible = vis;
                        }
                    }
                }
                self.push_undo_command(zengeld_chart::Command::AddIndicator {
                    instance_id: new_id,
                    type_id: tmpl.type_id.clone(),
                    params_json: String::new(),
                });
            }
        }

        let bars = self.panel_app.panel_grid.active_window()
            .map(|w| w.bars.clone())
            .unwrap_or_default();
        self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
        self.sync_sub_panes_from_manager();
        self.autosave_snapshot();
        self.sidebar_data_dirty = true;
        self.modal_state.close();
        eprintln!("[ChartApp] deployed indicator set '{}' ({} indicators, replaced {})", set.name, set.indicators.len(), existing_ids.len());
    }

    /// Save the current active indicators as a named indicator set.
    ///
    /// Called when the user confirms a name in the Create Indicator Set modal.
    /// Captures a full snapshot of each indicator's params and output styles.
    pub(super) fn execute_create_indicator_set(&mut self, name: String) {
        use zengeld_chart::templates::{IndicatorTemplate, OutputStyleConfig};

        let symbol = self.panel_app.panel_grid.active_window()
            .map(|w| w.symbol.clone())
            .unwrap_or_default();

        let instances = self.indicator_manager.get_instances_for_symbol(&symbol);
        let templates: Vec<IndicatorTemplate> = instances
            .iter()
            .map(|inst| {
                // Snapshot params: convert IndicatorValue → serde_json::Value
                let params: std::collections::HashMap<String, serde_json::Value> = inst.params
                    .iter()
                    .filter_map(|(k, v)| {
                        serde_json::to_value(v).ok().map(|jv| (k.clone(), jv))
                    })
                    .collect();

                // Snapshot output styles (color, line_width, visible)
                let outputs: std::collections::HashMap<String, OutputStyleConfig> = inst.outputs
                    .iter()
                    .map(|(k, out)| {
                        (k.clone(), OutputStyleConfig {
                            color: out.color.clone(),
                            line_width: out.line_width,
                            visible: Some(out.visible),
                        })
                    })
                    .collect();

                IndicatorTemplate::new(&inst.type_id, &inst.name, inst.visible, params, outputs)
            })
            .collect();

        let set = zengeld_chart::templates::indicator_set::IndicatorSet::from_templates(
            &name,
            templates,
        );
        eprintln!("[ChartApp] created indicator set '{}' from {} active indicators", name, instances.len());
        self.template_actions.push(crate::TemplateAction::AddIndicatorSet(set));
    }

    /// Handle clicks on widgets registered with the "modal_search:" prefix.
    pub(super) fn handle_search_modal_click(&mut self, rest: &str, x: f64, _y: f64) {
        match rest {
            "close" => {
                self.modal_state.close();
                eprintln!("[ChartApp] search modal closed");
            }
            "modal_bg" => {
                // Click inside modal body — no-op (prevents backdrop close)
            }
            "search_input" => {
                // Activate text editing on the search input, positioning cursor at click point
                let char_positions = self.search_modal_result.as_ref()
                    .map(|ms| ms.search_char_positions.clone());
                if let Some(positions) = char_positions {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&positions, x);
                    // Start editing if not already
                    if self.modal_state.editing_text.is_none() {
                        self.modal_state.start_editing(0);
                    }
                    if let Some(ref mut edit) = self.modal_state.editing_text {
                        edit.cursor = new_cursor;
                        edit.selection_start = None;
                    }
                } else {
                    self.modal_state.start_editing(0);
                }
                eprintln!("[ChartApp] search input activated");
            }
            _ if rest.starts_with("star:") => {
                let composite = &rest["star:".len()..];
                // Composite key format: "SYMBOL:exchange_id:account_type" — split from the
                // right twice so that symbols containing ':' (e.g. "tETH2X:USD") are handled
                // correctly.
                let (sym_part, exchange_part, at_label) =
                    if let Some(at_pos) = composite.rfind(':') {
                        let at_part = &composite[at_pos + 1..];
                        let remainder = &composite[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (&remainder[..ex_pos], &remainder[ex_pos + 1..], at_part.to_string())
                        } else {
                            (remainder, self.active_exchange.as_str(), at_part.to_string())
                        }
                    } else {
                        (composite, self.active_exchange.as_str(), "S".to_string())
                    };
                // Queue action for App to apply on AppState (single source of truth).
                self.watchlist_actions.push(crate::WatchlistAction::Toggle {
                    symbol: sym_part.to_string(),
                    exchange: exchange_part.to_string(),
                    account_type: at_label,
                });
                self.watchlists_dirty = true;
                eprintln!("[ChartApp] star toggle queued: {}:{}", sym_part, exchange_part);
            }
            _ if rest.starts_with("item:") => {
                let item_id = &rest["item:".len()..];
                eprintln!("[ChartApp] search modal item selected: {}", item_id);

                // Parse composite key "SYMBOL:exchange_id:account_type" — split from the
                // right twice so that symbols containing ':' (e.g. "tETH2X:USD") are handled
                // correctly.
                let (symbol_part, exchange_id_part, item_at_label) =
                    if let Some(at_pos) = item_id.rfind(':') {
                        let at_part = &item_id[at_pos + 1..];
                        let remainder = &item_id[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (
                                &remainder[..ex_pos],
                                &remainder[ex_pos + 1..],
                                at_part.to_string(),
                            )
                        } else {
                            // Only one colon — treat as "SYMBOL:exchange", no account_type.
                            (remainder, at_part, "S".to_string())
                        }
                    } else {
                        // No colon — treat the whole string as the symbol and fall back to
                        // the current active exchange.
                        (item_id, "", "S".to_string())
                    };

                // Resolve the exchange from the parsed exchange_id string.
                // Walk the known exchange_symbols keys to find a matching entry so we
                // never need a hard-coded string→ExchangeId mapping.
                let resolved_exchange = if exchange_id_part.is_empty() {
                    self.active_exchange
                } else {
                    self.exchange_symbols
                        .keys()
                        .find(|eid| eid.as_str() == exchange_id_part)
                        .copied()
                        .unwrap_or(self.active_exchange)
                };

                match self.modal_state.current {
                    OpenModal::SymbolSearch => {
                        // Capture previous symbol, exchange, and account_type BEFORE changing.
                        let previous_symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        let old_trade_exchange = self.active_exchange;
                        let old_trade_at = self.panel_app.panel_grid.active_window()
                            .map(|w| crate::account_type_from_label(&w.account_type))
                            .unwrap_or(digdigdig3::AccountType::Spot);
                        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                        let new_symbol_str = symbol_part.to_string();
                        // Set symbol, clear bars, and request data asynchronously.
                        let timeframe = self.panel_app.panel_grid.active_window()
                            .map(|w| w.timeframe.clone())
                            .unwrap_or_default();
                        // account_type is embedded in the item key — use it directly.
                        let search_at_label = item_at_label.clone();
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            // Snapshot current drawings before switching symbol
                            let old_sym = window.symbol.clone();
                            let old_exchange = window.exchange.clone();
                            let old_account_type = window.account_type.clone();
                            window.snapshot_drawings_for_symbol(&old_sym, &old_exchange, &old_account_type);
                            window.symbol = symbol_part.to_string();
                            window.exchange = resolved_exchange.as_str().to_string();
                            window.account_type = search_at_label.clone();
                            window.update_title();
                            window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
                            window.bars.clear();
                            window.viewport.bar_count = 0;
                            window.viewport.view_start = 0.0;
                            window.pending_symbol_load = true;
                            window.drawing_manager.clear_all_primitives();
                            window.restore_drawings_for_symbol(symbol_part, resolved_exchange.as_str(), &search_at_label);
                        }
                        // Switch to the exchange that owns this symbol and request bars.
                        self.active_exchange = resolved_exchange;
                        // Unsubscribe only the old symbol's trade stream, leaving other
                        // windows' streams intact.
                        if !previous_symbol.is_empty() {
                            self.bridge.unsubscribe_trades(old_trade_exchange, &previous_symbol, old_trade_at);
                        }
                        let eid_str = resolved_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (search symbol select)", eid_str);
                        } else {
                            let search_at = crate::account_type_from_label(&search_at_label);
                            self.bridge.ensure_connector(resolved_exchange);
                            self.bridge.request_bars(resolved_exchange, symbol_part, &timeframe, search_at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), false);
                        }
                        // Record ChangeSymbol if it actually changed.
                        if previous_symbol != new_symbol_str {
                            self.push_undo_command(zengeld_chart::Command::ChangeSymbol {
                                previous_symbol: previous_symbol.clone(),
                                new_symbol: new_symbol_str.clone(),
                            });
                            eprintln!("[ChartApp] Recorded ChangeSymbol {} -> {}", previous_symbol, new_symbol_str);
                            // Propagate new symbol to all leaves in the same sync group.
                            if let Some(leaf) = active_leaf {
                                self.propagate_symbol_to_sync_group(leaf, &new_symbol_str);
                            }
                        }
                        // Recalculate all indicators for the new symbol.
                        let (sym, bars) = self.panel_app.panel_grid.active_window()
                            .map(|w| (w.symbol.clone(), w.bars.clone()))
                            .unwrap_or_default();
                        self.indicator_manager.calculate_all_for_symbol(&sym, &bars);
                        self.sync_sub_panes_from_manager();
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                        self.modal_state.close();
                    }
                    OpenModal::CompareSearch => {
                        // Add the selected symbol as a compare overlay on the active chart window.
                        // ChartWindow::add_compare_symbol() fetches bars via the attached
                        // DemoDataProvider (which generates synthetic data for any symbol) and
                        // adds a CompareSeries with percentage normalization already handled.
                        let added_series = if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let added = window.add_compare_symbol(symbol_part);
                            eprintln!(
                                "[ChartApp] Compare symbol {}: {}",
                                symbol_part,
                                if added { "added" } else { "already present or unavailable" }
                            );
                            if added {
                                // Capture the newly-added series for the undo record.
                                window.compare_overlay.get_series(symbol_part).cloned()
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        // Record AddCompareSeries so the user can undo adding this overlay.
                        if let Some(series) = added_series {
                            self.push_undo_command(zengeld_chart::Command::AddCompareSeries {
                                series: series.without_bars(),
                            });
                            eprintln!("[ChartApp] Recorded AddCompareSeries {}", symbol_part);
                            self.autosave_snapshot();
                        }
                        self.modal_state.close();
                    }
                    OpenModal::IndicatorSearch => {
                        // Create indicator instance from catalog.
                        let symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        let type_id_str = item_id.to_string();
                        // Determine auto-color for overlay duplicates BEFORE create_instance
                        // (borrowck: create_instance needs &mut, query needs &).
                        let auto_color: Option<String> = self
                            .panel_app
                            .panel_grid
                            .docking()
                            .active_leaf()
                            .and_then(|leaf| self.panel_app.panel_grid.chart_id_for_leaf(leaf))
                            .and_then(|cid| {
                                self.indicator_manager.pick_overlay_color(item_id, &symbol, cid.0)
                            });
                        if let Some(new_id) = self.indicator_manager.create_instance(item_id, &symbol) {
                            // Apply auto-color when this is a duplicate overlay indicator.
                            if let Some(ref color) = auto_color {
                                if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                                    for oc in inst.outputs.values_mut() {
                                        oc.color = Some(color.clone());
                                    }
                                }
                            }
                            // Set window_id for the new instance so it's scoped to this window.
                            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                                if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                                    if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                                        inst.window_id = Some(chart_id.0);
                                    }
                                    // If this window is in a sync group and indicators are synced,
                                    // track the config in the group and push to peers.
                                    if let Some(group_id) = self.panel_app.panel_grid
                                        .window_for_leaf(active_leaf)
                                        .and_then(|w| w.group_id)
                                    {
                                        let sync_on = self.panel_app.tag_manager.group(group_id)
                                            .map(|g| g.sync_flags.sync_indicators)
                                            .unwrap_or(true);
                                        if sync_on {
                                            let inst_pane = self.indicator_manager
                                                .get_instance(new_id)
                                                .map(|i| i.pane as u32)
                                                .unwrap_or(0);
                                            let inst_name = self.indicator_manager
                                                .get_instance(new_id)
                                                .map(|i| i.name.clone())
                                                .unwrap_or_else(|| item_id.to_string());
                                            if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                                                group.indicator_configs.push(
                                                    zengeld_chart::tag_manager::IndicatorGroupConfig {
                                                        id: new_id,
                                                        type_id: item_id.to_string(),
                                                        name: inst_name,
                                                        params: std::collections::HashMap::new(),
                                                        pane: inst_pane,
                                                        visible: true,
                                                        symbol: symbol.clone(),
                                                    },
                                                );
                                                eprintln!(
                                                    "[TagManager] Added indicator config id={} '{}' to group {:?}",
                                                    new_id, item_id, group_id
                                                );
                                            }
                                            // Sync the new indicator to all peer windows in the group.
                                            self.sync_group_indicator_to_peers(group_id, item_id, active_leaf);
                                        } else {
                                            // sync_indicators is OFF — this indicator belongs to this
                                            // window only. Record it in pre_tag_indicator_ids so that
                                            // if the window is later desynced (untagged), the desync
                                            // logic does NOT treat it as a group indicator and delete it.
                                            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(active_leaf) {
                                                window.pre_tag_indicator_ids.push(new_id);
                                                eprintln!(
                                                    "[TagManager] sync_indicators=off: recorded id={} in pre_tag_indicator_ids for leaf {:?}",
                                                    new_id, active_leaf
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            // Record AddIndicator command.
                            self.push_undo_command(zengeld_chart::Command::AddIndicator {
                                instance_id: new_id,
                                type_id: type_id_str.clone(),
                                params_json: String::new(),
                            });
                            eprintln!("[ChartApp] Recorded AddIndicator {} id={}", type_id_str, new_id);
                            // Recalculate the new indicator with current bars.
                            let bars: Option<Vec<zengeld_chart::Bar>> = self.panel_app
                                .panel_grid
                                .active_window()
                                .map(|w| w.bars.clone());
                            if let Some(bars) = bars {
                                self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
                            }
                            self.sync_sub_panes_from_manager();
                            self.autosave_snapshot();
                            self.sidebar_data_dirty = true;
                            eprintln!("[ChartApp] Created indicator instance: {} (id={})", item_id, new_id);
                        }
                        self.modal_state.close();
                    }
                    _ => {}
                }
            }
            _ if rest.starts_with("category:") => {
                if let Ok(idx) = rest["category:".len()..].parse::<usize>() {
                    if let Some(filter) = IndicatorCategoryFilter::from_index(idx) {
                        self.modal_state.set_category_filter(filter);
                        eprintln!("[ChartApp] search category filter: {:?}", filter);
                    }
                }
            }
            "scrollbar_track" => {
                eprintln!("[ChartApp] scrollbar track clicked");
            }
            _ => {
                eprintln!("[ChartApp] search modal unhandled: {}", rest);
            }
        }
    }

    /// Handle a click that landed on the chart canvas (not a widget).
    pub(super) fn handle_canvas_click(&mut self, x: f64, y: f64) {
        // 1. Check scale corner buttons first.
        match self.scale_corner_zones.hit_test(x, y) {
            ScaleCornerButton::AutoManual => {
                let current_mode = self.panel_app.panel_grid
                    .active_window()
                    .map(|w| w.price_scale.scale_mode)
                    .unwrap_or(ScaleMode::Auto);
                let next_mode = current_mode.next();
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.price_scale.scale_mode = next_mode;
                    if next_mode.is_follow() {
                        window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                    }
                    if next_mode.is_auto_y() {
                        window.calc_auto_scale();
                    }
                    let is_auto = next_mode.is_auto_y();
                    for sp in &mut window.sub_panes {
                        // update_sub_pane_ranges() already bakes in symmetrization
                        // and 5% padding, so price_min/price_max already match
                        // what is displayed.  Just flip the flag.
                        sp.auto_scale = is_auto;
                    }
                }
                // Propagate scale_mode change to sync-group peers.
                if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    let viewport_state = self.panel_app.panel_grid
                        .active_window()
                        .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                    if let Some((view_start, bar_spacing)) = viewport_state {
                        self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, Some(next_mode));
                    }
                }
                return;
            }
            ScaleCornerButton::Mode => {
                // Cycle lin/log/% mode via existing output action.
                self.process_output_actions(vec![ChartOutputAction::TogglePriceScaleMode]);
                return;
            }
            ScaleCornerButton::None => {}
        }

        // 2. Sub-pane overlay button check — clicks on delete/hide/move-up/expand
        //    buttons are handled here before any other canvas logic.
        //
        //    IMPORTANT: `pane_index` from hit_test is the index in overlay_results
        //    (which may be filtered, e.g. only the maximized pane).  We resolve
        //    the actual sub-pane via `instance_id` from the overlay result.
        {
            let overlay_results_btn = self.panel_app.panel_grid.active_window()
                .map(|w| w.sub_pane_overlay_results.clone())
                .unwrap_or_default();
            let extended_btn = self.build_extended_layout();
            let tester_btn = ExtendedLayoutHitTester::new(&extended_btn)
                .with_overlays(&overlay_results_btn);
            use zengeld_chart::input::ChartHitTester;
            if let zengeld_chart::engine::input::HitResult::SubPaneOverlayButton { pane_index, button } = tester_btn.hit_test(x, y) {
                // Resolve instance_id from overlay results (correct even when maximized).
                let instance_id = overlay_results_btn.get(pane_index)
                    .map(|o| o.instance_id)
                    .unwrap_or(0);
                if instance_id == 0 { return; }

                // Find the real index in window.sub_panes by instance_id.
                let real_index = self.panel_app.panel_grid.active_window()
                    .and_then(|w| w.sub_panes.iter().position(|p| p.instance_id == instance_id));

                use zengeld_chart::ui::modal_settings::SubPaneButton;
                match button {
                    SubPaneButton::Delete => {
                        self.delete_indicator_instance(instance_id);
                    }
                    SubPaneButton::Hide => {
                        // Single source of truth: instance.visible controls
                        // both overlay and sub-pane indicator visibility.
                        // sub_pane.hidden is not used — render filters by instance.visible.
                        self.indicator_manager.toggle_visibility(instance_id);
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                    SubPaneButton::MoveUp => {
                        if let Some(idx) = real_index {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                let len = window.sub_panes.len();
                                if idx < len {
                                    let is_above = window.sub_panes[idx].above_main;
                                    if is_above {
                                        // Already above main — swap within above group if possible.
                                        if idx > 0 {
                                            window.sub_panes.swap(idx, idx - 1);
                                        }
                                    } else {
                                        // Below main: check if this is the first below-main pane.
                                        let first_below = window.sub_panes[..idx]
                                            .iter()
                                            .all(|p| p.above_main);
                                        if first_below {
                                            // Promote to above-main: set flag and move to end of
                                            // above group (last above = closest to main chart).
                                            window.sub_panes[idx].above_main = true;
                                            let above_end = window.sub_panes[..idx]
                                                .iter()
                                                .filter(|p| p.above_main)
                                                .count();
                                            // above_end is the count of above panes before idx;
                                            // since we just set [idx].above_main = true the pane
                                            // should sit at position above_end (end of the above
                                            // group).  Rotate it from idx down to above_end.
                                            window.sub_panes[above_end..=idx].rotate_right(1);
                                        } else {
                                            // Normal swap within below group.
                                            window.sub_panes.swap(idx, idx - 1);
                                        }
                                    }
                                    for (i, pane) in window.sub_panes.iter_mut().enumerate() {
                                        pane.index = i;
                                    }
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::MoveDown => {
                        if let Some(idx) = real_index {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                let len = window.sub_panes.len();
                                if idx < len {
                                    let is_above = window.sub_panes[idx].above_main;
                                    if !is_above {
                                        // Already below main — swap within below group if possible.
                                        if idx + 1 < len {
                                            window.sub_panes.swap(idx, idx + 1);
                                        }
                                    } else {
                                        // Above main: check if this is the last above-main pane.
                                        let last_above = window.sub_panes[idx + 1..]
                                            .iter()
                                            .all(|p| !p.above_main);
                                        if last_above {
                                            // Demote to below-main: set flag and move to position 0
                                            // in the below group (first below = closest to main
                                            // chart).
                                            window.sub_panes[idx].above_main = false;
                                            // Count how many above-main panes are at positions
                                            // 0..idx (they stay in place).  The pane should move
                                            // to position idx (first below slot) — it's already
                                            // there since all after idx are below-main, so a
                                            // rotate_left on [idx..] places it at the start of the
                                            // below group.
                                            // Actually: positions [idx+1..] are all below_main,
                                            // so rotating [idx..] left by 1 moves idx to end.
                                            // We want it at position idx (start of below group),
                                            // so no rotate needed — it's already at the boundary.
                                        } else {
                                            // Normal swap within above group.
                                            window.sub_panes.swap(idx, idx + 1);
                                        }
                                    }
                                    for (i, pane) in window.sub_panes.iter_mut().enumerate() {
                                        pane.index = i;
                                    }
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::Expand => {
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            for pane in window.sub_panes.iter_mut() {
                                if pane.instance_id == instance_id {
                                    pane.pre_maximize_height_ratio = pane.height_ratio;
                                    pane.maximized = true;
                                } else {
                                    pane.maximized = false;
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::Restore => {
                        if let Some(idx) = real_index {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                if let Some(sub_pane) = window.sub_panes.get_mut(idx) {
                                    sub_pane.maximized = false;
                                    sub_pane.height_ratio = sub_pane.pre_maximize_height_ratio;
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::IndicatorEye => {
                        self.indicator_manager.toggle_visibility(instance_id);
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                    SubPaneButton::IndicatorAlert => {
                        // TODO: open alert creation for this indicator
                    }
                    SubPaneButton::IndicatorSettings => {
                        self.panel_app.indicator_settings_state.open(instance_id);
                    }
                    SubPaneButton::IndicatorDelete => {
                        self.delete_indicator_instance(instance_id);
                    }
                }
                return;
            }
        }

        // 3. Drawing tool check — if a tool is active, convert the click to
        //    chart-local data coordinates and forward to DrawingManager.on_click().
        //
        //    Handles both main chart and sub-pane areas.  Sub-panes share the
        //    X-axis (bar index) with the main chart but have their own Y-axis
        //    (price range = indicator values, e.g. RSI 0-100).
        let tool_id_opt = self.panel_app.toolbar_state.active_tool_id.clone();
        if let Some(ref _tool_id) = tool_id_opt {
            // Build the extended layout so we get the exact same main_chart rect
            // that render_full_chart_panel uses.  This accounts for sub-panes
            // (RSI, MACD, …) that reduce the main chart height below
            // viewport.chart_height.
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;

            let is_freehand = self.panel_app.panel_grid.active_window()
                .map(|w| w.drawing_manager.is_freehand_tool())
                .unwrap_or(false);

            // Freehand tools (brush/highlighter) only work on drag — skip click.
            if is_freehand {
                return;
            }

            // Determine which area was clicked: main chart or a sub-pane.
            let local_x_main = x - chart_rect.x;
            let local_y_main = y - chart_rect.y;
            let in_main_chart = local_x_main >= 0.0 && local_x_main <= chart_rect.width
                && local_y_main >= 0.0 && local_y_main <= chart_rect.height;

            // Check sub-panes if not in main chart.
            let sub_pane_hit: Option<(usize, u64, f64, f64)> = if !in_main_chart {
                extended.sub_panes.iter().enumerate().find_map(|(pane_idx, pane_layout)| {
                    let content = pane_layout.content;
                    let local_x = x - content.x;
                    let local_y = y - content.y;
                    if local_x >= 0.0 && local_x <= content.width
                        && local_y >= 0.0 && local_y <= content.height
                    {
                        Some((pane_idx, pane_layout.instance_id, local_x, local_y))
                    } else {
                        None
                    }
                })
            } else {
                None
            };

            if in_main_chart {
                // Main chart click.
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .map(|w| (w.price_scale.price_min, w.price_scale.price_max))
                    .unwrap_or((0.0, 100.0));

                let chart_height = chart_rect.height;
                let bar = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        // Snap to bar center (matching crosshair coordinate system).
                        if let Some(idx) = w.viewport.x_to_bar(local_x_main) {
                            idx as f64
                        } else {
                            w.viewport.x_to_bar_f64(local_x_main)
                        }
                    })
                    .unwrap_or(0.0);
                let price_range = price_max - price_min;
                let raw_price = if chart_height > 0.0 {
                    price_max - (local_y_main / chart_height) * price_range
                } else {
                    price_min
                };

                // If magnet mode is active, call calculate_magnet_snap() directly
                // (like the terminal does) rather than relying on crosshair state.
                let price = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        if w.crosshair.is_magnet() {
                            let bar_idx = w.viewport.x_to_bar(local_x_main);
                            let (snapped_price, _) = w.calculate_magnet_snap(
                                bar_idx, raw_price, chart_height, price_min, price_max,
                            );
                            snapped_price
                        } else {
                            raw_price
                        }
                    })
                    .unwrap_or(raw_price);

                // Reset pane context to main chart.
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.set_current_pane(None);
                    if window.drawing_manager.current_tool().map(|t| t != _tool_id).unwrap_or(true) {
                        window.drawing_manager.set_tool(Some(_tool_id));
                    }
                }

                // Dispatch click and record undo if a primitive was just completed.
                eprintln!("[CANVAS_CLICK] screen=({:.0},{:.0}) -> bar={:.2}, price={:.12e}, price_range=[{:.12e}..{:.12e}]", x, y, bar, price, price_min, price_max);
                let primitive_created = self.panel_app.panel_grid.active_window_mut()
                    .map(|w| w.drawing_manager.on_click(bar, price))
                    .unwrap_or(false);

                if primitive_created {
                    // Populate point_timestamps immediately so TF switching keeps
                    // the primitive anchored to the correct time position.
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let bars = window.bars.clone();
                        window.drawing_manager.update_all_timestamps_from_bars(&bars);
                    }
                    let prim_count = self.panel_app.panel_grid.active_window()
                        .map(|w| w.drawing_manager.primitives().len()).unwrap_or(0);
                    eprintln!("[ChartApp] Primitive created (main) at bar={:.2}, price={:.12e}, total_primitives={}", bar, price, prim_count);
                    // For grouped windows: move the completed primitive to TagManager so
                    // all group members see it via the per-frame render-cache sync.
                    // For standalone windows: keep it in drawing_manager as before.
                    if !self.intercept_completed_primitive_to_group() {
                        // Standalone path — record undo and propagate to color-tag peers.
                        // Extract snapshot before mutable borrow for push_undo_command.
                        let create_snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let idx = w.drawing_manager.last_index()?;
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = create_snapshot {
                            self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            eprintln!("[ChartApp] Recorded CreatePrimitive (main) at index {}", idx);
                        }
                        // Propagate the new primitive to sync group peers.
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_new_primitive_to_sync_group(active_leaf);
                            // Clear the in-progress preview on peers (state is now Idle).
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    } else {
                        // Grouped path — record undo for the primitive placed into the group,
                        // then clear the in-progress preview on peers.
                        let group_create_cmd = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.group_id)
                            .and_then(|gid| {
                                let group = self.panel_app.tag_manager.group(gid)?;
                                let idx = group.primitives.len().saturating_sub(1);
                                let prim = group.primitives.get(idx)?;
                                Some(zengeld_chart::Command::CreatePrimitive {
                                    index: idx,
                                    type_id: prim.type_id().to_string(),
                                    points: prim.points().to_vec(),
                                    data: prim.data().clone(),
                                })
                            });
                        if let Some(cmd) = group_create_cmd {
                            self.push_undo_command(cmd);
                            eprintln!("[ChartApp] Recorded CreatePrimitive (main, grouped) via group.primitives");
                        }
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    }
                    // Save after both grouped and standalone paths — intercept may have moved
                    // the primitive to TagManager, so snapshot must be taken after transfer.
                    self.sidebar_data_dirty = true;
                    self.autosave_snapshot();
                } else if self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.is_drawing())
                    .unwrap_or(false)
                {
                    eprintln!("[ChartApp] Drawing point added (main) at bar={:.2}, price={:.2}", bar, price);
                    // Propagate in-progress drawing state so peers show the preview.
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                }

                // Re-check if tool was completed (primed tools deactivate after creation).
                let tool_cleared = self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.current_tool().is_none())
                    .unwrap_or(false);
                if tool_cleared {
                    self.panel_app.toolbar_state.active_tool_id = None;
                    self.panel_app.toolbar_state.primed_id = Some("cursor_tools".to_string());
                    eprintln!("[ChartApp] Primed tool completed, switched to cursor mode");
                }

                return;
            } else if let Some((pane_idx, instance_id, local_x, local_y)) = sub_pane_hit {
                // Sub-pane click — use the pane's own Y-axis.
                let pane_layout = &extended.sub_panes[pane_idx];
                let pane_height = pane_layout.content.height;

                // Get price range for this sub-pane from window state.
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .and_then(|w| {
                        w.sub_panes.iter()
                            .find(|sp| sp.instance_id == instance_id)
                            .map(|sp| (sp.price_min, sp.price_max))
                    })
                    .unwrap_or((0.0, 100.0));

                let bar = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        // Snap to bar center (matching crosshair coordinate system).
                        if let Some(idx) = w.viewport.x_to_bar(local_x) {
                            idx as f64
                        } else {
                            w.viewport.x_to_bar_f64(local_x)
                        }
                    })
                    .unwrap_or(0.0);
                let price_range = price_max - price_min;
                let price = if pane_height > 0.0 {
                    price_max - (local_y / pane_height) * price_range
                } else {
                    price_min
                };

                // Set pane context so the new primitive is tagged to this sub-pane.
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.set_current_pane(Some(instance_id));
                    if window.drawing_manager.current_tool().map(|t| t != _tool_id).unwrap_or(true) {
                        window.drawing_manager.set_tool(Some(_tool_id));
                    }
                }

                // Dispatch click and record undo if a primitive was just completed.
                let primitive_created = self.panel_app.panel_grid.active_window_mut()
                    .map(|w| w.drawing_manager.on_click(bar, price))
                    .unwrap_or(false);

                if primitive_created {
                    // Populate point_timestamps immediately so TF switching keeps
                    // the primitive anchored to the correct time position.
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let bars = window.bars.clone();
                        window.drawing_manager.update_all_timestamps_from_bars(&bars);
                    }
                    eprintln!("[ChartApp] Primitive created (sub-pane #{}) at bar={:.2}, price={:.2}", pane_idx, bar, price);
                    // For grouped windows: move the completed primitive to TagManager.
                    // For standalone windows: keep in drawing_manager and propagate to peers.
                    if !self.intercept_completed_primitive_to_group() {
                        // Standalone path — extract snapshot then push undo command.
                        let create_snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let idx = w.drawing_manager.last_index()?;
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = create_snapshot {
                            self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            eprintln!("[ChartApp] Recorded CreatePrimitive (sub-pane #{}) at index {}", pane_idx, idx);
                        }
                        // Propagate the new primitive to sync group peers.
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_new_primitive_to_sync_group(active_leaf);
                            // Clear the in-progress preview on peers (state is now Idle).
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    } else {
                        // Grouped path — record undo for the primitive placed into the group,
                        // then clear the in-progress preview on peers.
                        let group_create_cmd = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.group_id)
                            .and_then(|gid| {
                                let group = self.panel_app.tag_manager.group(gid)?;
                                let idx = group.primitives.len().saturating_sub(1);
                                let prim = group.primitives.get(idx)?;
                                Some(zengeld_chart::Command::CreatePrimitive {
                                    index: idx,
                                    type_id: prim.type_id().to_string(),
                                    points: prim.points().to_vec(),
                                    data: prim.data().clone(),
                                })
                            });
                        if let Some(cmd) = group_create_cmd {
                            self.push_undo_command(cmd);
                            eprintln!("[ChartApp] Recorded CreatePrimitive (sub-pane #{}, grouped) via group.primitives", pane_idx);
                        }
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    }
                    // Save after both grouped and standalone paths — intercept may have moved
                    // the primitive to TagManager, so snapshot must be taken after transfer.
                    self.sidebar_data_dirty = true;
                    self.autosave_snapshot();
                } else if self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.is_drawing())
                    .unwrap_or(false)
                {
                    eprintln!("[ChartApp] Drawing point added (sub-pane #{}) at bar={:.2}, price={:.2}", pane_idx, bar, price);
                    // Propagate in-progress drawing state so peers show the preview.
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                }

                // Sync toolbar if tool completed.
                let tool_cleared = self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.current_tool().is_none())
                    .unwrap_or(false);
                if tool_cleared {
                    self.panel_app.toolbar_state.active_tool_id = None;
                    self.panel_app.toolbar_state.primed_id = Some("cursor_tools".to_string());
                    eprintln!("[ChartApp] Primed tool completed (sub-pane), switched to cursor mode");
                }

                return;
            } else {
                // Click outside both main chart and all sub-panes — ignore.
                return;
            }
        }

        // 3. Normal chart canvas click — no drawing tool active.
        //    Check if the click hits any primitive (drawing_manager.hit_test).
        //    Check main chart first, then sub-panes.
        {
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;

            // -- Main chart primitive hit test --
            if local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height
            {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    // Use corrected viewport dimensions from the layout so that
                    // hit-test coordinate mapping matches the render path exactly.
                    let mut corrected_vp = window.viewport.clone();
                    corrected_vp.chart_width = chart_rect.width;
                    corrected_vp.chart_height = chart_rect.height;
                    if let Some(prim_idx) = window.drawing_manager.hit_test(
                        local_x, local_y, &corrected_vp, &window.price_scale,
                    ) {
                        // Select the primitive, clear indicator selection, and return.
                        self.selected_indicator_id = None;
                        // Deselect all other windows first so control points don't accumulate.
                        let active_cid = self.panel_app.panel_grid.active_chart_id();
                        for (cid, win) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                            if Some(*cid) != active_cid {
                                win.drawing_manager.deselect();
                            }
                        }
                        if let Some(win) = self.panel_app.panel_grid.active_window_mut() {
                            win.drawing_manager.set_current_pane(None);
                            win.drawing_manager.select_by_index(prim_idx);
                            eprintln!("[ChartApp] Primitive selected (main): index={}", prim_idx);
                        }
                        return;
                    }
                }
            }

            // -- Sub-pane primitive hit test --
            for (pane_idx, pane_layout) in extended.sub_panes.iter().enumerate() {
                let content = pane_layout.content;
                let plx = x - content.x;
                let ply = y - content.y;
                if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                    continue;
                }
                let instance_id = pane_layout.instance_id;
                // Build a temporary PriceScale for the sub-pane's Y-axis.
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .and_then(|w| {
                        w.sub_panes.iter()
                            .find(|sp| sp.instance_id == instance_id)
                            .map(|sp| (sp.price_min, sp.price_max))
                    })
                    .unwrap_or((0.0, 100.0));
                let sub_price_scale = zengeld_chart::PriceScale::new(price_min, price_max);
                // Use a viewport with pane height so hit_test_in_pane coordinate conversion is correct.
                let sub_viewport = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        let mut vp = w.viewport.clone();
                        vp.chart_height = content.height;
                        vp
                    });
                if let (Some(sub_viewport), Some(window)) = (sub_viewport, self.panel_app.panel_grid.active_window()) {
                    if let Some(prim_idx) = window.drawing_manager.hit_test_in_pane(
                        plx, ply, instance_id, &sub_viewport, &sub_price_scale,
                    ) {
                        // Select the primitive, clear indicator selection, and return.
                        self.selected_indicator_id = None;
                        // Deselect all other windows first so control points don't accumulate.
                        let active_cid = self.panel_app.panel_grid.active_chart_id();
                        for (cid, win) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                            if Some(*cid) != active_cid {
                                win.drawing_manager.deselect();
                            }
                        }
                        if let Some(win) = self.panel_app.panel_grid.active_window_mut() {
                            win.drawing_manager.set_current_pane(Some(instance_id));
                            win.drawing_manager.select_by_index(prim_idx);
                            eprintln!("[ChartApp] Primitive selected (sub-pane #{}): index={}", pane_idx, prim_idx);
                        }
                        return;
                    }
                }
            }

            // -- Sub-pane indicator hit test --
            for sp in &extended.sub_panes {
                let pane_rect = sp.content;
                let plx = x - pane_rect.x;
                let ply = y - pane_rect.y;
                if plx < 0.0 || plx > pane_rect.width || ply < 0.0 || ply > pane_rect.height {
                    continue;
                }
                let instance_id = sp.instance_id;
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .and_then(|w| {
                        w.sub_panes.iter()
                            .find(|p| p.instance_id == instance_id)
                            .map(|p| (p.price_min, p.price_max))
                    })
                    .unwrap_or((0.0, 100.0));
                let hit = self.panel_app.panel_grid.active_window()
                    .map(|window| {
                        self.indicator_manager.hit_test_sub_pane(
                            instance_id,
                            plx, ply,
                            &window.viewport,
                            price_min, price_max,
                            pane_rect.height,
                            8.0,
                        )
                    })
                    .unwrap_or(false);
                if hit {
                    if self.selected_indicator_id == Some(instance_id) {
                        self.selected_indicator_id = None;
                    } else {
                        self.selected_indicator_id = Some(instance_id);
                        for win in self.panel_app.panel_grid.windows_mut().values_mut() {
                            win.drawing_manager.deselect();
                        }
                    }
                    eprintln!("[ChartApp] Sub-pane indicator hit: id={}", instance_id);
                    return;
                }
            }

            // -- Overlay indicator hit test (main chart only) --
            if local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height
            {
                let ind_hit = self.panel_app.panel_grid.active_window()
                    .and_then(|window| {
                        self.indicator_manager.hit_test_overlay(
                            local_x,
                            local_y,
                            &window.symbol,
                            &window.viewport,
                            &window.price_scale,
                            chart_rect.height,
                            8.0,
                        )
                    });
                if let Some(ind_id) = ind_hit {
                    self.selected_indicator_id = Some(ind_id);
                    // Deselect any drawing primitive across ALL windows when an indicator is selected.
                    for win in self.panel_app.panel_grid.windows_mut().values_mut() {
                        win.drawing_manager.deselect();
                    }
                    eprintln!("[ChartApp] Indicator selected: id={}", ind_id);
                    return;
                }
            }
        }

        // No primitive or indicator hit — deselect everything across ALL windows.
        self.selected_indicator_id = None;
        for window in self.panel_app.panel_grid.windows_mut().values_mut() {
            window.drawing_manager.deselect();
        }

        let extended = self.build_extended_layout();
        let overlay_results_cc = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(&extended)
            .with_overlays(&overlay_results_cc);
        let actions = self.input_handler.process_action(
            ChartInputAction::Click {
                x,
                y,
                button: MouseButton::Left,
            },
            &hit_tester,
        );
        self.process_output_actions(actions);
    }

    /// Close only the topmost modal layer (layered close).
    ///
    /// Called when the user clicks on a modal backdrop.  Instead of closing all
    /// open modals at once, we pop exactly one layer — the one with the highest
    /// Z-order — so that clicking outside a colour-picker dismisses only the
    /// picker and leaves the parent settings modal open.
    pub(super) fn close_topmost_modal_layer(&mut self) {
        // Borrow, clone the layer id, then drop the RefMut before calling any
        // &mut self methods that would conflict with the borrow.
        let layer_id_opt = self.input_coordinator.borrow_mut().topmost_modal_layer().cloned();
        if let Some(layer_id) = layer_id_opt {
            let name = layer_id.0.as_str();
            eprintln!("[ChartApp] close_topmost_modal_layer: {}", name);
            match name {
                // Color picker layers — close one level (L2→L1 or L1→Closed).
                "color_picker_primitive" => {
                    self.panel_app.primitive_settings_state.close_color_picker_one_level();
                }
                "color_picker_indicator" => {
                    self.panel_app.indicator_settings_state.close_color_picker_one_level();
                }
                "color_picker_chart" => {
                    self.panel_app.chart_settings_state.close_color_picker_one_level();
                }
                "color_picker_panel" => {
                    self.panel_app.close_panel_color_tag_picker_one_level();
                }
                // L2 settings modals.
                "primitive_settings" => {
                    self.panel_app.primitive_settings_state.close();
                }
                "chart_settings" => {
                    self.panel_app.chart_settings_state.close();
                }
                "indicator_settings" => {
                    self.panel_app.indicator_settings_state.close();
                }
                "alert_settings" => {
                    self.panel_app.alert_settings_state.close();
                }
                "modal_search" => {
                    self.modal_state.close();
                }
                // Top-level modals — close when user clicks outside them.
                "overlay_settings" => {
                    self.panel_app.close_overlay_settings();
                    eprintln!("[ChartApp] close_topmost_modal_layer: overlay_settings closed");
                }
                "tags_tabs" => {
                    self.panel_app.close_tags_tabs();
                    eprintln!("[ChartApp] close_topmost_modal_layer: tags_tabs closed");
                }
                "chart_browser" => {
                    self.panel_app.chart_browser.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] close_topmost_modal_layer: chart_browser closed");
                }
                "user_settings" => {
                    self.panel_app.user_settings_state.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: user_settings closed");
                }
                "profile_manager" => {
                    // In skeleton mode (vault unlock required), profile manager cannot be dismissed.
                    if !self.panel_app.user_settings_state.needs_vault_unlock {
                        self.panel_app.user_settings_state.show_profile_manager = false;
                        self.panel_app.user_settings_state.profile_manager_page =
                            zengeld_chart::ui::modal_settings::ProfileManagerPage::ProfileList;
                    }
                    eprintln!("[ChartApp] close_topmost_modal_layer: profile_manager (needs_vault_unlock={})", self.panel_app.user_settings_state.needs_vault_unlock);
                }
                // Preset name input — close only this modal, not the search overlay underneath.
                "preset_name_input" => {
                    self.panel_app.preset_name_input.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: preset_name_input closed");
                }
                // Watchlist group name input L2 modal — close only this layer.
                "wl_group_name_input" => {
                    self.wl_group_name_input.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: wl_group_name_input closed");
                }
                // Watchlist modal — close when user clicks outside it.
                "watchlist_modal" => {
                    self.watchlist_modal.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: watchlist_modal closed");
                }
                _ => {
                    // Unknown layer — fall back to closing everything so the UI
                    // never gets stuck in an unrecoverable state.
                    eprintln!("[ChartApp] close_topmost_modal_layer: unknown layer '{}', closing all", name);
                    self.close_open_modals();
                }
            }
        }
    }

    /// Close all open modals (state lives inside panel_app at checkpoint).
    pub(super) fn close_open_modals(&mut self) {
        // Close search/compare/indicator-search overlays (SymbolSearch, CompareSearch, IndicatorSearch).
        self.modal_state.close();
        self.panel_app.primitive_settings_state.close();
        self.panel_app.chart_settings_state.close();
        self.panel_app.indicator_settings_state.close();
        // Close top-level modals that are managed outside of modal_state.
        self.panel_app.close_overlay_settings();
        self.panel_app.close_tags_tabs();
        if self.panel_app.chart_browser.is_open {
            self.panel_app.chart_browser.close();
            self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
        }
    }

    /// Close transient overlays (toolbar dropdowns, context menu, sidebar
    /// dropdowns, agent popups) that should dismiss on any "outside" click.
    /// Called when click lands on canvas (or any non-overlay widget) so that
    /// dropdowns/context-menu stay short-lived even when chart:pane is now a
    /// registered BlackboxPanel widget that consumes the click event.
    pub(super) fn close_transient_overlays(&mut self) {
        self.panel_app.toolbar_state.open_dropdown_id = None;
        self.panel_app.toolbar_state.hovered_dropdown_item = None;
        self.panel_app.toolbar_state.open_inline_style_dropdown = false;
        self.panel_app.toolbar_state.open_inline_width_dropdown = false;
        self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
        self.panel_app.context_menu_state.close();
        self.modal_state.close();
        self.sidebar_state.watchlist_config_dropdown_open = false;
        self.sidebar_state.watchlist_color_picker_open = None;
        self.sidebar_state.slot_spawn_dropdown = None;
        self.sidebar_state.agent_model_dropdown = None;
        self.sidebar_state.agent_perm_dropdown = None;
        self.sidebar_state.agent_sessions_dropdown = None;
    }

    /// Handle a context menu item click.
    pub(super) fn on_context_menu_action(&mut self, action: &str) {
        let target = self.panel_app.context_menu_state.target.clone();
        self.panel_app.context_menu_state.close();

        match target {
            ContextMenuTarget::Primitive(idx) => {
                match action {
                    "settings" => {
                        self.panel_app.primitive_settings_state.open(idx);
                        eprintln!("[ChartApp] Opened primitive settings for #{}", idx);
                    }
                    "delete" => {
                        // Capture state BEFORE deletion so undo can recreate.
                        let snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((type_id, points, data))
                            });
                        if let Some((type_id, points, data)) = snapshot {
                            let prim_id = data.id;
                            self.push_undo_command(zengeld_chart::Command::DeletePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            eprintln!("[ChartApp] Recorded DeletePrimitive at index {}", idx);
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                window.drawing_manager.remove(idx);
                                eprintln!("[ChartApp] Deleted primitive #{}", idx);
                            }
                            self.alert_manager.remove_alerts_for_drawing(prim_id);
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                        } else {
                            // No undo snapshot — still get the ID for alert cleanup before removing.
                            let prim_id = self.panel_app.panel_grid
                                .active_window()
                                .and_then(|w| w.drawing_manager.get_data_at(idx))
                                .map(|d| d.id);
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                window.drawing_manager.remove(idx);
                                eprintln!("[ChartApp] Deleted primitive #{} (no undo snapshot)", idx);
                            }
                            if let Some(pid) = prim_id {
                                self.alert_manager.remove_alerts_for_drawing(pid);
                            }
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                        }
                    }
                    "clone" => {
                        // clone_primitive returns Option<usize> (new index).
                        let cloned_idx = self.panel_app.panel_grid.active_window_mut()
                            .and_then(|w| w.drawing_manager.clone_primitive(idx));
                        if let Some(new_idx) = cloned_idx {
                            // Apply +20px right, -20px up offset to the cloned primitive.
                            // Convert screen pixels to bar/price coordinates using the viewport.
                            if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                                let bar_spacing = w.viewport.bar_spacing;
                                let chart_height = w.viewport.chart_height;
                                let price_range = w.price_scale.price_max - w.price_scale.price_min;
                                // 20px right = 20 / bar_spacing bars
                                let bar_delta = 20.0 / bar_spacing.max(0.001);
                                // 20px up = 20 / chart_height * price_range (up = positive price)
                                let price_delta = if chart_height > 0.0 && price_range > 0.0 {
                                    20.0 / chart_height * price_range
                                } else {
                                    0.0
                                };
                                w.drawing_manager.translate_at(new_idx, bar_delta, price_delta);
                                let bars = w.bars.clone();
                                w.drawing_manager.update_all_timestamps_from_bars(&bars);
                                w.drawing_manager.select_by_index(new_idx);
                            }
                            // Capture the NEW primitive's data for undo.
                            let snapshot = self.panel_app.panel_grid.active_window()
                                .and_then(|w| {
                                    let type_id = w.drawing_manager.get_type_id_at(new_idx)?;
                                    let points = w.drawing_manager.get_points_at(new_idx)?;
                                    let data = w.drawing_manager.get_data_at(new_idx)?;
                                    Some((type_id, points, data))
                                });
                            if let Some((type_id, points, data)) = snapshot {
                                self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                                    index: new_idx,
                                    type_id,
                                    points,
                                    data,
                                });
                                eprintln!("[ChartApp] Recorded CreatePrimitive (clone) at index {}", new_idx);
                            }
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                            eprintln!("[ChartApp] Cloned primitive #{} -> #{}", idx, new_idx);
                        } else {
                            eprintln!("[ChartApp] Clone primitive #{} returned None", idx);
                        }
                    }
                    "toggle_lock" => {
                        // Read current state BEFORE mutating.
                        let previous = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.drawing_manager.get_data_at(idx))
                            .map(|d| d.locked)
                            .unwrap_or(false);
                        self.push_undo_command(zengeld_chart::Command::SetPrimitiveLock {
                            index: idx,
                            locked: !previous,
                            previous,
                        });
                        eprintln!("[ChartApp] Recorded SetPrimitiveLock at index {} locked={}", idx, !previous);
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.toggle_lock_primitive(idx);
                        }
                        self.sidebar_data_dirty = true;
                    }
                    "toggle_visibility" => {
                        // Read current state BEFORE mutating.
                        let previous = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.drawing_manager.get_data_at(idx))
                            .map(|d| d.visible)
                            .unwrap_or(true);
                        self.push_undo_command(zengeld_chart::Command::SetPrimitiveVisibility {
                            index: idx,
                            visible: !previous,
                            previous,
                        });
                        eprintln!("[ChartApp] Recorded SetPrimitiveVisibility at index {} visible={}", idx, !previous);
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.toggle_visibility(idx);
                        }
                        self.sidebar_data_dirty = true;
                    }
                    _ => {
                        eprintln!("[ChartApp] Unhandled primitive context action: {}", action);
                    }
                }
            }
            ContextMenuTarget::ChartBackground => {
                match action {
                    "chart_settings" => {
                        self.panel_app.chart_settings_state.toggle();
                        eprintln!("[ChartApp] Chart settings opened from context menu");
                    }
                    "reset_zoom" => {
                        self.process_output_actions(vec![ChartOutputAction::ResetTimeScale]);
                        eprintln!("[ChartApp] Reset zoom from context menu");
                    }
                    "screenshot" => {
                        self.request_screenshot();
                    }
                    "symbol_search" => {
                        self.modal_state.open(OpenModal::SymbolSearch);
                        self.modal_state.symbol_search_results =
                            crate::ChartApp::build_demo_symbol_results("", &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
                        self.modal_state.start_editing(0);
                        eprintln!("[ChartApp] symbol search opened from context menu");
                    }
                    "reset_cache" => {
                        self.pending_reset_cache = true;
                        eprintln!("[ChartApp] Reset cache requested");
                    }
                    "reset_storage" => {
                        self.pending_reset_storage = true;
                        eprintln!("[ChartApp] Reset storage requested");
                    }
                    _ => {
                        eprintln!("[ChartApp] Unhandled background context action: {}", action);
                    }
                }
            }
            ContextMenuTarget::Indicator(id) => {
                eprintln!("[ChartApp] Indicator context action: {} for id={}", action, id);
            }
            ContextMenuTarget::ColorTag(leaf_id) => {
                match action {
                    "desync" => {
                        self.perform_desync(leaf_id);
                    }
                    _ => {
                        eprintln!("[ChartApp] Unhandled color tag context action: {}", action);
                    }
                }
            }
            _ => {
                eprintln!("[ChartApp] Context menu action on unknown target: {}", action);
            }
        }
    }
}
