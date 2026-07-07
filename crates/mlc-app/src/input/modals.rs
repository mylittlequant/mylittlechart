//! Modal and search routing: watchlist modal, search overlay, context menu,
//! inline actions, indicator set deploy/create, canvas click (drawing tool placement),
//! modal layer dismissal.

use crate::ChartApp;
use mlc_core::{
    ChartInputAction,
    ChartOutputAction,
    ScaleCornerButton,
    input::MouseButton,
    ScaleMode,
    localize_primitive_name,
};
use mlc_app_layout::layout::ExtendedLayoutHitTester;
use mlc_app_layout::ui::context_menu::ContextMenuTarget;
use mlc_core::state::modal_state::{OpenModal, IndicatorCategoryFilter};

impl ChartApp {
    /// Handle a click on an inline primitive toolbar action.
    ///
    /// These actions are generated when a drawing primitive is selected and the
    /// user clicks one of the compact action buttons rendered inline in the
    /// control strip (`inline:settings`, `inline:color`, `inline:delete`, …).
    pub(super) fn handle_inline_action(&mut self, action: &str) {
        use mlc_core::drawing::primitives_v2::LineStyle;

        // The selected primitive id comes from the drawing manager, not from
        // a settings-state field, because the primitive may not be open in a
        // modal yet.
        let active_bid_inline = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        let selected_id = active_bid_inline
            .and_then(|bid| self.core.drawings().selected(bid));

        let prim_id = match selected_id {
            Some(i) => i,
            None => {
                eprintln!("[ChartApp] inline action '{}' ignored — no primitive selected", action);
                return;
            }
        };

        // Sync primitive_id into primitive_settings_state so apply_primitive_color
        // (and other downstream handlers that read .primitive_id) know which primitive
        // the inline toolbar is operating on. Without this, color changes from the
        // inline color picker silently no-op because primitive_id stays None until
        // the full primitive_settings modal is opened.
        self.panel_app.primitive_settings_state.primitive_id = Some(prim_id);

        let screen_w = self.width as f64;
        let screen_h = self.height as f64;

        match action {
            // ── Settings ─────────────────────────────────────────────────────
            "inline:settings" => {
                self.panel_app.primitive_settings_state.open(prim_id);
                eprintln!("[ChartApp] inline: opened primitive settings for #{}", prim_id);
            }

            // ── Stroke color ─────────────────────────────────────────────────
            "inline:color" => {
                let current_color = active_bid_inline
                    .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                    .map(|d| d.color.stroke.clone());

                // Look up the button rect from the input coordinator so the
                // color picker can be anchored beneath the clicked button.
                let widget_id_str = "ilb:inline:color".to_string();
                let rect = self.input_coordinator.borrow_mut().widget_rect(
                    &uzor::input::WidgetId::from(widget_id_str),
                );
                let (ax, ay, aw, ah) = rect
                    .map(|r| (r.x, r.y, r.width, r.height))
                    .unwrap_or((0.0, 0.0, 0.0, 0.0));

                self.panel_app.primitive_settings_state.open_color_picker_smart(
                    "stroke_color", ax, ay, aw, ah, screen_w, screen_h,
                    current_color.as_deref(),
                );
                eprintln!("[ChartApp] inline: opened stroke color picker for #{}", prim_id);
            }

            // ── Text color ───────────────────────────────────────────────────
            "inline:text_color" => {
                let current_color = active_bid_inline
                    .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                    .and_then(|d| d.text.and_then(|t| t.color));

                let widget_id_str = "ilb:inline:text_color".to_string();
                let rect = self.input_coordinator.borrow_mut().widget_rect(
                    &uzor::input::WidgetId::from(widget_id_str),
                );
                let (ax, ay, aw, ah) = rect
                    .map(|r| (r.x, r.y, r.width, r.height))
                    .unwrap_or((0.0, 0.0, 0.0, 0.0));

                self.panel_app.primitive_settings_state.open_color_picker_smart(
                    "text_color", ax, ay, aw, ah, screen_w, screen_h,
                    current_color.as_deref(),
                );
                eprintln!("[ChartApp] inline: opened text color picker for #{}", prim_id);
            }

            // ── Delete ───────────────────────────────────────────────────────
            "inline:delete" => {
                if let Some(bid) = active_bid_inline {
                    self.core.drawings_mut().delete(bid, prim_id);
                    eprintln!("[ChartApp] inline: deleted selected primitive");
                }
                self.sync_drawing_back_to_group();
                {
                    let removed = self.core.alerts_mut().clear_for_drawing(prim_id);
                    for alert_snapshot in removed {
                        self.push_undo_command(mlc_core::Command::DeleteAlert { snapshot: alert_snapshot });
                    }
                }
                self.autosave_snapshot();
            }

            // ── Lock toggle ──────────────────────────────────────────────────
            "inline:lock" => {
                if let Some(bid) = active_bid_inline {
                    self.core.drawings_mut().toggle_lock_primitive(bid, prim_id);
                    eprintln!("[ChartApp] inline: toggled lock on selected primitive");
                }
                self.sync_drawing_back_to_group();
            }

            // ── Line style cycle ─────────────────────────────────────────────
            "inline:style" => {
                let data_opt = active_bid_inline
                    .and_then(|bid| self.core.drawings().get_data(bid, prim_id));

                if let Some(mut data) = data_opt {
                    data.style = match data.style {
                        LineStyle::Solid        => LineStyle::Dashed,
                        LineStyle::Dashed       => LineStyle::Dotted,
                        LineStyle::Dotted       => LineStyle::LargeDashed,
                        LineStyle::LargeDashed  => LineStyle::SparseDotted,
                        LineStyle::SparseDotted => LineStyle::Solid,
                    };
                    let style_str = data.style.as_str().to_string();
                    if let Some(bid) = active_bid_inline {
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] inline: cycled line style to {}", style_str);
                }
                self.snapshot_primitive_settings_to_user_manager(prim_id);
            }

            // ── Width cycle ──────────────────────────────────────────────────
            "inline:width" => {
                if let Some(bid) = active_bid_inline {
                    if let Some(mut data) = self.core.drawings().get_data(bid, prim_id) {
                        data.width = (data.width + 1.0).clamp(1.0, 20.0);
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                    eprintln!("[ChartApp] inline: increased line width");
                }
                self.sync_drawing_back_to_group();
                self.snapshot_primitive_settings_to_user_manager(prim_id);
            }

            // ── Name label — absorb, no action ──────────────────────────────
            "inline:name" => {
                eprintln!("[ChartApp] inline: name label clicked (no-op)");
            }

            // ── Alert — open alert settings modal for selected primitive ────
            "inline:alert" => {
                // S2.7.1e: read symbol via branch_meta (B1).
                let inline_alert_symbol: String = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let inline_alert_price_fallback = self.panel_app.panel_grid.active_container()
                    .and_then(|_w| None::<&mlc_core::Bar>)
                    .map(|b| b.close)
                    .unwrap_or(0.0);
                if let Some(bid) = active_bid_inline {
                    let guard = self.core.drawings();
                    if let Some(prim) = guard.selected_primitive(bid) {
                        let label = localize_primitive_name(prim.type_id(), prim.display_name());
                        let pts = prim.points();
                        let price = if !pts.is_empty() {
                            pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64
                        } else {
                            inline_alert_price_fallback
                        };
                        let source = mlc_core::alert::AlertSource::Drawing { primitive_id: prim_id, label };
                        drop(guard);
                        self.panel_app.alert_settings_state.open_new(source, &inline_alert_symbol, price);
                        self.panel_app.alert_settings_state.pin_initial_position(
                            self.content_rect.width, self.content_rect.height,
                        );
                        eprintln!("[ChartApp] inline alert: opened modal for primitive {}", prim_id);
                    }
                }
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
                use mlc_core::drawing::primitives_v2::LineStyle;
                let value = action.strip_prefix("inline:style_option:").unwrap_or_default();
                let new_style = match value {
                    "solid"        => Some(LineStyle::Solid),
                    "dashed"       => Some(LineStyle::Dashed),
                    "dotted"       => Some(LineStyle::Dotted),
                    "large_dashed" => Some(LineStyle::LargeDashed),
                    "sparse_dotted" => Some(LineStyle::SparseDotted),
                    _ => None,
                };
                if let Some(style) = new_style {
                    if let Some(mut data) = active_bid_inline
                        .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                    {
                        data.style = style;
                        let style_str = data.style.as_str().to_string();
                        if let Some(bid) = active_bid_inline {
                            self.core.drawings_mut().set_data(bid, prim_id, &data);
                        }
                        self.sync_drawing_back_to_group();
                        eprintln!("[ChartApp] inline: set line style to {}", style_str);
                    }
                }
                self.snapshot_primitive_settings_to_user_manager(prim_id);
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ if action.starts_with("inline:width_option:") => {
                // ── Width option selected ────────────────────────────────────
                let value = action.strip_prefix("inline:width_option:").unwrap_or_default();
                if let Ok(new_width) = value.parse::<f64>() {
                    if let Some(mut data) = active_bid_inline
                        .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                    {
                        data.width = new_width;
                        if let Some(bid) = active_bid_inline {
                            self.core.drawings_mut().set_data(bid, prim_id, &data);
                        }
                        self.sync_drawing_back_to_group();
                        eprintln!("[ChartApp] inline: set line width to {}", new_width);
                    }
                }
                self.snapshot_primitive_settings_to_user_manager(prim_id);
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ => {
                eprintln!("[ChartApp] inline: unhandled action '{}'", action);
            }
        }
    }

    /// Handle clicks on widgets registered with the "wl_modal:" prefix.
    pub(super) fn handle_watchlist_modal_click(&mut self, rest: &str, x: f64, y: f64) {
        match rest {
            "close" => {
                self.standalone_panels.watchlist_modal.close();
                eprintln!("[WatchlistModal] closed");
            }
            "modal_bg" | "header_drag" | "list_scroll" => {
                // Absorbed — no-op for backdrop clicks, drag zone, scroll zone
            }
            "search_input" => {
                // Click-to-cursor — reposition caret. on_drag_end fires on mouseup
                // via the global path in mod.rs::on_drag_end (line ~3614).
                self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
            }
            _ if rest.starts_with("tab:") => {
                use mlc_app_layout::ui::modal_settings::WatchlistModalTab;
                let tab = &rest["tab:".len()..];
                match tab {
                    "overview" => self.standalone_panels.watchlist_modal.active_tab = WatchlistModalTab::Overview,
                    "groups"   => self.standalone_panels.watchlist_modal.active_tab = WatchlistModalTab::Groups,
                    "settings" => self.standalone_panels.watchlist_modal.active_tab = WatchlistModalTab::Settings,
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
                            let at = self.core.watchlists().find(self.watchlist_ui.active_list_id)
                                .and_then(|list| {
                                    list.all_symbols().iter()
                                        .find(|ws| ws.symbol == remainder && ws.exchange == at_part)
                                        .map(|ws| ws.account_type.as_label().to_string())
                                })
                                .unwrap_or_else(|| "S".to_string());
                            (remainder, at_part, at)
                        }
                    } else {
                        (composite, self.active_exchange.as_str(), "S".to_string())
                    };
                // Resolve ExchangeId from string.
                let resolved_exchange = self.core.exchange_symbols()
                    .find_by_label(exchange_part)
                    .unwrap_or(self.active_exchange);
                // Switch the active chart to this symbol+exchange.
                // Capture old trade-stream identity BEFORE mutating window state.
                // S2.7.1e: read old instrument fields via branch_meta (B1).
                let old_trade_exchange = self.active_exchange;
                let active_chart_id = self.panel_app.panel_grid.active_container_id();
                let old_trade_symbol: String = active_chart_id
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let old_trade_at = active_chart_id
                    .and_then(|cid| self.account_type_for_chart(cid))
                    .map(mlc_backend_data_bridge::account_type_to_dig3)
                    .unwrap_or(digdigdig3::AccountType::Spot);
                let timeframe = active_chart_id
                    .and_then(|cid| self.timeframe_for_chart(cid))
                    .unwrap_or_default();
                let old_sym = active_chart_id
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let old_exchange = active_chart_id
                    .and_then(|cid| self.exchange_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let old_account_type = active_chart_id
                    .and_then(|cid| self.account_type_for_chart(cid))
                    .map(|at| at.to_string())
                    .unwrap_or_else(|| "S".to_string());
                let active_bid_wl = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    if let Some(bid) = active_bid_wl {
                        let restore_snaps = window.take_drawings_snapshot_for_symbol(sym_part, exchange_part, &wl_at_label);
                        let new_address = mlc_core::instrument::InstrumentAddress::new(
                            exchange_part,
                            sym_part,
                            wl_at_label.parse().unwrap_or_default(),
                        );
                        let result = self.core.drawings_mut().swap_symbol(
                            bid, &new_address, restore_snaps.as_deref(),
                        );
                        let old_key = format!("{}:{}:{}", old_sym, old_exchange, old_account_type);
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
                            sym_part.to_string(),
                            exchange_part.to_string(),
                            wl_at_label.clone(),
                            timeframe.clone(),
                        );
                    }
                }
                // Propagate symbol change to all peers in the same sync group.
                let wl_active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                if let Some(leaf) = wl_active_leaf {
                    self.propagate_symbol_to_sync_group(leaf, sym_part, exchange_part, &wl_at_label);
                }
                self.active_exchange = resolved_exchange;
                // Unsubscribe only the old symbol's trade stream, leaving other
                // windows' streams intact.
                if !old_trade_symbol.is_empty() {
                    self.core.unsubscribe_trades(old_trade_exchange, &old_trade_symbol, old_trade_at);
                }
                let eid_str = resolved_exchange.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (watchlist modal item click)", eid_str);
                } else {
                    let wl_at = crate::account_type_from_label(&wl_at_label);
                    self.core.ensure_connector(resolved_exchange);
                    self.core.request_kline(resolved_exchange, sym_part, &timeframe, wl_at, None, Some(self.core.user().profile().domain.bar_count as usize), false);
                }
                self.autosave_snapshot();
                eprintln!("[WatchlistModal] symbol selected: {} @ {}", sym_part, exchange_part);
                self.standalone_panels.watchlist_modal.close();
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
                            let at = self.core.watchlists().find(self.watchlist_ui.active_list_id)
                                .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == remainder && ws.exchange == at_part).map(|ws| ws.account_type.as_label().to_string()))
                                .unwrap_or_default();
                            (remainder, at_part.to_string(), at)
                        }
                    } else {
                        // No colon — plain symbol, look up exchange + account_type.
                        let ex = self.core.watchlists().find(self.watchlist_ui.active_list_id)
                            .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == key).map(|ws| ws.exchange.clone()))
                            .unwrap_or_else(|| self.active_exchange.as_str().to_string());
                        let at = self.core.watchlists().find(self.watchlist_ui.active_list_id)
                            .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == key).map(|ws| ws.account_type.as_label().to_string()))
                            .unwrap_or_default();
                        (key, ex, at)
                    };
                let exchange = exchange_owned.as_str();
                let account_type = account_type_owned.as_str();
                let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                // Remove symbol from snapshot (if active) before removing from list.
                {
                    let active_list_id = self.watchlist_ui.active_list_id;
                    if let Some(ref mut snap) = self.watchlist_ui.list_mut(active_list_id).order_snapshot {
                        snap.retain(|s| *s != address);
                    }
                    self.core.watchlists_mut().remove_symbol(active_list_id, &address);
                }
                self.watchlist_actions.push(crate::WatchlistAction::Remove { symbol: symbol.to_string(), exchange: exchange.to_string(), account_type: account_type.to_string() });
                self.watchlists_dirty = true;
                self.persist_watchlists();
                eprintln!("[WatchlistModal] symbol removed: {}:{}:{}", symbol, exchange, account_type);
            }
            // Switch active watchlist
            _ if rest.starts_with("group:") => {
                let id_str = &rest["group:".len()..];
                if let Ok(id) = id_str.parse::<u64>() {
                    self.watchlist_ui.active_list_id = id;
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
                    let deleted = self.core.watchlists_mut().delete_list(id);
                    self.watchlist_actions.push(crate::WatchlistAction::DeleteList { id });
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[WatchlistModal] delete list id={} -> {}", id, deleted);
                }
            }
            // Create new watchlist — open name input modal with auto-generated name
            "group_add" => {
                let max_n = self.core.watchlists().lists().iter()
                    .filter_map(|l| l.name.strip_prefix("Watchlist "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Watchlist {}", max_n + 1);
                self.standalone_panels.wl_group_name_input.open_create_new_with_name(&default_name);
                {
                    let wgn_id = uzor::WidgetId::from(crate::text_input::WATCHLIST_GROUP_NAME);
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&wgn_id, &default_name);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&wgn_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::WATCHLIST_GROUP_NAME);
                }
                eprintln!("[WatchlistModal] opening group name input for create new: '{}'", default_name);
            }
            // Rename a watchlist — open name input modal pre-filled with current name
            _ if rest.starts_with("group_rename:") => {
                let id_str = &rest["group_rename:".len()..];
                if let Ok(id) = id_str.parse::<u64>() {
                    if let Some(list) = self.core.watchlists().find(id) {
                        let name = list.name.clone();
                        self.standalone_panels.wl_group_name_input.open_rename(id, &name);
                        {
                            let wgn_id = uzor::WidgetId::from(crate::text_input::WATCHLIST_GROUP_NAME);
                            self.input_coordinator.borrow_mut().text_fields_mut().set_text(&wgn_id, &name);
                            self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&wgn_id);
                            self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::WATCHLIST_GROUP_NAME);
                        }
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
                let max_n = self.core.templates().indicator_sets()
                    .iter()
                    .filter_map(|s| s.name.strip_prefix("My Indicator Set "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("My Indicator Set {}", max_n + 1);

                let current_time_ms = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                self.panel_app.preset_name_input.open_create_indicator_set(
                    &default_name,
                    current_time_ms,
                );
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
                {
                    let pni_id = uzor::WidgetId::from(crate::text_input::PRESET_NAME_INPUT);
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&pni_id, &default_name);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&pni_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::PRESET_NAME_INPUT);
                }
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
        let set = self.core.templates().indicator_sets()
            .iter()
            .find(|s| s.id == set_id)
            .cloned();

        let Some(set) = set else {
            eprintln!("[ChartApp] deploy_indicator_set: set not found: {}", set_id);
            return;
        };

        // Indicator ownership is by bubble — target the active chart's current bubble.
        let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);

        // Remove all existing indicators (exclusive deployment, like preset)
        let existing_ids: Vec<u64> = {
            let indicators_guard = self.core.indicators();
            active_bid
                .map(|bid| indicators_guard.instances_for_bubble(bid))
                .unwrap_or_default()
                .into_iter()
                .map(|inst| inst.id)
                .collect()
        };
        for id in &existing_ids {
            self.core.indicators_mut().remove(*id);
        }
        // Clear orphan pre_tag_indicator_ids from auto_bubble for every window so the
        // Object Tree does not show phantom items after a bulk indicator replace.
        {
            let auto_bids: Vec<mlc_core::ids::BubbleId> = self.panel_app.panel_grid
                .containers()
                .values()
                .map(|w| w.auto_tag_id)
                .filter(|&bid| bid != mlc_core::ids::BubbleId(0))
                .collect();
            for bid in auto_bids {
                self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                    auto_bub.pre_tag_indicator_ids.retain(|iid| !existing_ids.contains(iid));
                });
            }
        }
        // Clear stale overlay states
        self.panel_app.indicator_overlay_states.clear();
        eprintln!("[ChartApp] deploy_indicator_set: removed {} existing indicators", existing_ids.len());

        // Create new indicators from the set with full params/outputs
        for tmpl in &set.indicators {
            // Indicator lives in the chart's CURRENT bubble.
            let new_id_opt = self.core.indicators_mut().add(active_bid, &tmpl.type_id);
            if let Some(new_id) = new_id_opt {
                // Apply saved params, output styles, and visibility from the template
                {
                    let params: Vec<(String, mlc_backend_indicator_engine::IndicatorParamValue)> = tmpl.params
                        .iter()
                        .filter_map(|(k, v)| serde_json::from_value(v.clone()).ok().map(|iv| (k.clone(), iv)))
                        .collect();
                    let outputs: Vec<(String, mlc_core::templates::OutputStyleConfig)> = tmpl.outputs
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect();
                    self.core.indicators_mut().apply_template(new_id, tmpl.visible, params, outputs);
                }
                self.push_undo_command(mlc_core::Command::AddIndicator {
                    instance_id: new_id,
                    type_id: tmpl.type_id.clone(),
                    params_json: String::new(),
                });
            }
        }

        // Pull real bars from the chart's bubble — the cutover sweep had stubbed
        // this to `Vec::new()` which made the recalc a no-op.
        let bars = active_bid
            .and_then(|bid| self.with_bubble(bid, |b| b.bars.clone()))
            .unwrap_or_default();
        if let Some(bid) = active_bid {
            self.core.indicators_mut().recalc_bubble(bid, &bars);
        }
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
    pub(crate) fn execute_create_indicator_set(&mut self, name: String) {
        use mlc_core::templates::{IndicatorTemplate, OutputStyleConfig};

        // Indicator ownership is by bubble — target the active chart's current bubble.
        let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);

        let templates: Vec<IndicatorTemplate> = {
            let im = self.core.indicators();
            let instances = active_bid.map(|bid| im.instances_for_bubble(bid)).unwrap_or_default();
            instances
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
                .collect()
        };

        let templates_count = templates.len();
        let set = mlc_core::templates::indicator_set::IndicatorSet::from_templates(
            &name,
            templates,
        );
        eprintln!("[ChartApp] created indicator set '{}' from {} active indicators", name, templates_count);
        self.template_actions.push(crate::TemplateAction::AddIndicatorSet(set));
    }

    /// Handle clicks on widgets registered with the "modal_search:" prefix.
    pub(crate) fn handle_search_modal_click(&mut self, rest: &str, x: f64, y: f64) {
        match rest {
            "close" => {
                self.modal_state.close();
                eprintln!("[ChartApp] search modal closed");
            }
            "modal_bg" => {
                // Click inside modal body — no-op (prevents backdrop close)
            }
            "search_input" => {
                // Click-to-cursor — reposition caret. on_drag_end fires on mouseup
                // via the global path in mod.rs::on_drag_end (line ~3614).
                self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
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
                    self.core.exchange_symbols()
                        .find_by_label(exchange_id_part)
                        .unwrap_or(self.active_exchange)
                };

                match self.modal_state.current {
                    OpenModal::SymbolSearch => {
                        // Capture previous symbol, exchange, and account_type BEFORE changing.
                        // S2.7.1e: read via branch_meta (B1).
                        let active_chart_id = self.panel_app.panel_grid.active_container_id();
                        let previous_symbol: String = active_chart_id
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let old_trade_exchange = self.active_exchange;
                        let old_trade_at = active_chart_id
                            .and_then(|cid| self.account_type_for_chart(cid))
                            .map(mlc_backend_data_bridge::account_type_to_dig3)
                            .unwrap_or(digdigdig3::AccountType::Spot);
                        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                        let new_symbol_str = symbol_part.to_string();
                        // Set symbol, clear bars, and request data asynchronously.
                        let timeframe = active_chart_id
                            .and_then(|cid| self.timeframe_for_chart(cid))
                            .unwrap_or_default();
                        // account_type is embedded in the item key — use it directly.
                        let search_at_label = item_at_label.clone();
                        let old_sym_search = active_chart_id
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let old_exch_search = active_chart_id
                            .and_then(|cid| self.exchange_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let old_at_search = active_chart_id
                            .and_then(|cid| self.account_type_for_chart(cid))
                            .map(|at| at.to_string())
                            .unwrap_or_else(|| "S".to_string());
                        let active_bid_srch = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                            if let Some(bid) = active_bid_srch {
                                let restore_snaps = window.take_drawings_snapshot_for_symbol(symbol_part, resolved_exchange.as_str(), &search_at_label);
                                let new_address = mlc_core::instrument::InstrumentAddress::new(
                                    resolved_exchange.as_str(),
                                    symbol_part,
                                    search_at_label.parse().unwrap_or_default(),
                                );
                                let result = self.core.drawings_mut().swap_symbol(
                                    bid, &new_address, restore_snaps.as_deref(),
                                );
                                let old_key = format!("{}:{}:{}", old_sym_search, old_exch_search, old_at_search);
                                if result.old_primitives.is_empty() {
                                    window.symbol_drawings.remove(&old_key);
                                } else {
                                    window.symbol_drawings.insert(old_key, result.old_primitives);
                                }
                            }
                            window.begin_fresh_load();
                        }
                        // Phase 1.2: dual-write instrument into branch_meta + bubble.
                        if let Some(chart_id) = active_chart_id {
                            if let Some(bid) = self.bubble_for_chart(chart_id) {
                                self.core.bubbles_mut().set_bid_instrument_full(
                                    bid,
                                    symbol_part.to_string(),
                                    resolved_exchange.as_str().to_string(),
                                    search_at_label.clone(),
                                    timeframe.clone(),
                                );
                            }
                        }
                        // Switch to the exchange that owns this symbol and request bars.
                        self.active_exchange = resolved_exchange;
                        // Unsubscribe only the old symbol's trade stream, leaving other
                        // windows' streams intact.
                        if !previous_symbol.is_empty() {
                            self.core.unsubscribe_trades(old_trade_exchange, &previous_symbol, old_trade_at);
                        }
                        let eid_str = resolved_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (search symbol select)", eid_str);
                        } else {
                            let search_at = crate::account_type_from_label(&search_at_label);
                            self.core.ensure_connector(resolved_exchange);
                            self.core.request_kline(resolved_exchange, symbol_part, &timeframe, search_at, None, Some(self.core.user().profile().domain.bar_count as usize), false);
                        }
                        // Record ChangeSymbol if it actually changed.
                        if previous_symbol != new_symbol_str {
                            self.push_undo_command(mlc_core::Command::ChangeSymbol {
                                previous_symbol: previous_symbol.clone(),
                                new_symbol: new_symbol_str.clone(),
                            });
                            eprintln!("[ChartApp] Recorded ChangeSymbol {} -> {}", previous_symbol, new_symbol_str);
                            // Propagate new symbol+exchange+account_type to all leaves in the same sync group.
                            if let Some(leaf) = active_leaf {
                                self.propagate_symbol_to_sync_group(leaf, &new_symbol_str, resolved_exchange.as_str(), &search_at_label);
                            }
                        }
                        // Recalculate all indicators for the new bars in this window.
                        let recalc_bid_and_bars = self.panel_app.panel_grid.docking().active_leaf()
                            .and_then(|leaf| self.panel_app.panel_grid.container_id_for_leaf(leaf))
                            .and_then(|cid| self.bubble_for_chart(cid))
                            .and_then(|bid| self.with_bubble(bid, |b| b.bars.clone()).map(|bars| (bid, bars)));
                        if let Some((bid, bars)) = recalc_bid_and_bars {
                            self.core.indicators_mut().recalc_bubble(bid, &bars);
                        }
                        self.sync_sub_panes_from_manager();
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                        self.modal_state.close();
                    }
                    OpenModal::CompareSearch => {
                        self.compare_add_attempts = self.compare_add_attempts.saturating_add(1);
                        // Add the selected symbol as a compare overlay on the active chart window.
                        // `add_compare_symbol` calls data_provider.get_bars() which on a cache
                        // miss fires an async bridge.request_bars() and returns None → false.
                        // In that case we queue a PendingCompareAdd so react.rs can retry once
                        // the BarsLoaded event arrives.
                        let active_cmp_cid = self.panel_app.panel_grid.active_container_id();
                        let main_sym_for_cmp = active_cmp_cid
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let main_tf_for_cmp = active_cmp_cid
                            .and_then(|cid| self.timeframe_for_chart(cid))
                            .unwrap_or_else(mlc_core::state::Timeframe::h1);
                        let main_bars_for_cmp = self.active_chart_bars();
                        let added_series = if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                            let added = window.add_compare_symbol(symbol_part, &main_sym_for_cmp, &main_tf_for_cmp, &main_bars_for_cmp);
                            eprintln!(
                                "[ChartApp] Compare symbol {}: {}",
                                symbol_part,
                                if added { "added" } else { "queuing retry (bars not yet cached)" }
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
                            self.compare_add_success = self.compare_add_success.saturating_add(1);
                            self.push_undo_command(mlc_core::Command::AddCompareSeries {
                                series: series.without_bars(),
                            });
                            eprintln!("[ChartApp] Recorded AddCompareSeries {}", symbol_part);
                            self.autosave_snapshot();
                        } else if let Some(cid) = active_cmp_cid {
                            self.compare_add_queued = self.compare_add_queued.saturating_add(1);
                            // add_compare_symbol returned false — bars not in cache.
                            //
                            // The window's `data_provider` is hardwired to the window's
                            // OWN exchange + account_type (BTCUSDT@binance/Spot in the
                            // default seed), so calling `data_provider.get_bars` for a
                            // compare symbol that lives on a different exchange/AT
                            // would fire `bridge.request_bars` against the WRONG
                            // exchange/AT pair — the server returns nothing for
                            // ETH-USD-SWAP @ Binance@Spot, so no BarsLoaded ever fires.
                            //
                            // Fire the request explicitly here with the resolved
                            // exchange + account type from the compare modal pick.
                            // `force=true` so we bypass dedup-L1 / dedup-L2 —
                            // the user explicitly asked for this overlay, and
                            // a non-force call would silently no-op if any
                            // previous session subscribed the same kline key.
                            let cmp_at = crate::account_type_from_label(&item_at_label);
                            self.core.ensure_connector(resolved_exchange);
                            self.core.request_kline(
                                resolved_exchange,
                                symbol_part,
                                &main_tf_for_cmp,
                                cmp_at,
                                None,
                                Some(self.core.user().profile().domain.bar_count as usize),
                                true,
                            );
                            eprintln!(
                                "[Compare] Queued retry + bridge.request_bars for {}@{} ({})",
                                symbol_part,
                                resolved_exchange.as_str(),
                                item_at_label
                            );
                            self.pending_compare.push(crate::PendingCompareAdd {
                                chart_id: mlc_core::ChartId(cid.0),
                                symbol: symbol_part.to_string(),
                                exchange: resolved_exchange,
                                account_type_label: item_at_label.clone(),
                                deadline_frames: 600,
                            });
                        }
                        self.modal_state.close();
                    }
                    OpenModal::IndicatorSearch => {
                        // Opaque host-driven overlays (Volume Profile / OI Delta /
                        // Funding Rate / CVD) are catalog entries for discoverability
                        // only — they carry no `machine_id` in mli and are not
                        // computed through `IndicatorManager`. Intercept here and
                        // dispatch the existing Toggle event instead of falling
                        // through to the regular add() path below.
                        if let Some(event) = crate::overlay_toggle_for_catalog_id(item_id) {
                            self.process_chart_out_event(event);
                            self.modal_state.close();
                            return;
                        }
                        // Create indicator instance from catalog — bound to the active
                        // window, NOT to the symbol it currently displays.
                        let type_id_str = item_id.to_string();
                        // Approach A: if docking has no active leaf (can happen on wasm after
                        // deserialization restores the tree without replaying the active-leaf
                        // selection), fall back to the first registered leaf and activate it so
                        // the indicator is not silently dropped.
                        let active_leaf = {
                            let from_docking = self.panel_app.panel_grid.docking().active_leaf();
                            if from_docking.is_none() {
                                if let Some(fallback) = self.panel_app.panel_grid.first_leaf_id() {
                                    eprintln!(
                                        "[IndicatorSearch] docking().active_leaf() = None; \
                                         activating fallback leaf {:?} so indicator '{}' is not orphaned",
                                        fallback, type_id_str
                                    );
                                    self.panel_app.panel_grid.set_active_leaf(fallback);
                                    Some(fallback)
                                } else {
                                    eprintln!(
                                        "[IndicatorSearch] no leaves in panel_grid — \
                                         cannot assign indicator '{}'; it will be orphaned",
                                        type_id_str
                                    );
                                    None
                                }
                            } else {
                                from_docking
                            }
                        };
                        let active_container_id = active_leaf
                            .and_then(|leaf| self.panel_app.panel_grid.container_id_for_leaf(leaf));
                        let bid = active_container_id
                            .and_then(|cid| self.panel_app.panel_grid.containers().get(&cid))
                            .map(|w| w.current_tag_id);
                        // Determine auto-color for overlay duplicates BEFORE add()
                        // (borrowck: add() needs &mut, query needs &).
                        let auto_color: Option<String> = bid
                            .and_then(|b| self.core.indicators_mut().pick_overlay_color(item_id, b));
                        let new_id_opt = self.core.indicators_mut().add(bid, item_id);
                        if let Some(new_id) = new_id_opt {
                            crate::sglog!(
                                "[indicator-create] type='{}' new_id={} chart_id={:?} current_tag_id={:?}",
                                item_id, new_id, active_container_id, bid
                            );
                            // Apply auto-color when this is a duplicate overlay indicator.
                            if let Some(ref color) = auto_color {
                                self.core.indicators_mut().set_all_output_colors(new_id, color.clone());
                            }
                            if let Some(active_leaf) = active_leaf {
                                // If this window is in a sync group and indicators are synced,
                                // track the config in the group and push to peers.
                                if let Some(group_id) = self.panel_app.panel_grid
                                    .container_id_for_leaf(active_leaf)
                                    .and_then(|cid| self.bubble_for_chart(cid))
                                {
                                    let sync_on = self.with_bubble(group_id, |g| g.sync_flags.sync_indicators).unwrap_or(true);
                                    if sync_on {
                                        let (inst_pane, inst_name) = {
                                            let im = self.core.indicators();
                                            let inst_pane = im.pane_of(new_id)
                                                .map(|p| p as u32)
                                                .unwrap_or(0);
                                            let inst_name = im.name_of(new_id)
                                                .unwrap_or_else(|| item_id.to_string());
                                            (inst_pane, inst_name)
                                        };
                                        let pushed = self.core.bubbles_mut().with_bubble_mut(group_id, |group| {
                                            group.indicator_configs.push(
                                                mlc_core::indicator_types::IndicatorGroupConfig {
                                                    id: new_id,
                                                    type_id: item_id.to_string(),
                                                    name: inst_name,
                                                    params: std::collections::HashMap::new(),
                                                    pane: inst_pane,
                                                    visible: true,
                                                    symbol: String::new(),
                                                    exchange: String::new(),
                                                    account_type: String::new(),
                                                },
                                            );
                                        }).is_some();
                                        if pushed {
                                            eprintln!(
                                                "[BubbleStore] Added indicator config id={} '{}' to group {:?}",
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
                                        // sync_indicators is OFF — record in auto_bubble.pre_tag_indicator_ids.
                                        if let Some(cid) = active_container_id {
                                            let auto_bid = self.panel_app.panel_grid.containers()
                                                .get(&cid)
                                                .map(|w| w.auto_tag_id);
                                            if let Some(bid) = auto_bid {
                                                self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                                                    auto_bub.pre_tag_indicator_ids.push(new_id);
                                                });
                                            }
                                            eprintln!(
                                                "[BubbleStore] sync_indicators=off: recorded id={} in auto_bubble.pre_tag_indicator_ids for leaf {:?}",
                                                new_id, active_leaf
                                            );
                                        }
                                    }
                                }
                            }
                            // Record AddIndicator command.
                            self.push_undo_command(mlc_core::Command::AddIndicator {
                                instance_id: new_id,
                                type_id: type_id_str.clone(),
                                params_json: String::new(),
                            });
                            eprintln!("[ChartApp] Recorded AddIndicator {} id={}", type_id_str, new_id);
                            // Recalculate the new indicator with current bars.
                            // Bars live on the chart's DataBubble — the cutover sweep
                            // stubbed this to `Vec::new()` (empty) so the recalc was
                            // a no-op and the new indicator stayed blank until the
                            // next live bar tick. Pull the actual bars out of the
                            // chart's bubble via `with_bubble`.
                            let bars_opt: Option<Vec<mlc_core::Bar>> = bid
                                .and_then(|b| self.with_bubble(b, |bub| bub.bars.clone()));
                            if let (Some(b), Some(bars)) = (bid, bars_opt) {
                                self.core.indicators_mut().recalc_bubble(b, &bars);
                            }
                            // CVD instance just added — ensure the real
                            // aggressor-CVD subscription is live (unless the
                            // window is a footprint chart, which already has
                            // real per-bar buy/sell from `footprint_bars`).
                            if type_id_str.eq_ignore_ascii_case("cvd") {
                                self.subscribe_cvd_line_for_indicator_add(active_container_id);
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
        //
        // Walk per-leaf zones. In single-window mode the map has one entry
        // (the active leaf). In split mode resolve_input already handles
        // ScaleCorner before we reach here, but the map is populated for all
        // leaves as a second-chance safety net.
        let corner_hit = self.scale_corner_zones_by_leaf
            .iter()
            .find_map(|(&lid, zones)| {
                let btn = zones.hit_test(x, y);
                if btn != ScaleCornerButton::None { Some((lid, btn)) } else { None }
            });
        if let Some((hit_leaf_id, button)) = corner_hit {
            self.panel_app.panel_grid.set_active_leaf(hit_leaf_id);
            match button {
                ScaleCornerButton::AutoManual => {
                    let current_mode = self.panel_app.panel_grid
                        .container_for_leaf(hit_leaf_id)
                        .map(|w| w.price_scale().scale_mode)
                        .unwrap_or(ScaleMode::Auto);
                    let next_mode = current_mode.next();
                    let hit_series = self.panel_app.panel_grid
                        .container_id_for_leaf(hit_leaf_id)
                        .map(|cid| self.active_series_for_chart(cid))
                        .unwrap_or_default();
                    if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(hit_leaf_id) {
                        window.cycle_scale_mode(next_mode, hit_series.as_active_series());
                    }
                    // Mutate group.scale_mode when sync_viewport is on so the group
                    // remembers the mode for new joiners.
                    let viewport_sync_on = self.panel_app.panel_grid
                        .container_id_for_leaf(hit_leaf_id)
                        .and_then(|cid| self.bubble_for_chart(cid))
                        .and_then(|gid| {
                            self.core.bubbles_mut().set_scale_mode(gid, next_mode);
                            self.core.bubbles().sync_flags(gid).map(|sf| sf.sync_viewport)
                        })
                        .unwrap_or(false);
                    if viewport_sync_on {
                        let viewport_state = self.panel_app.panel_grid
                            .container_for_leaf(hit_leaf_id)
                            .map(|w| (w.viewport().view_start, w.viewport().bar_spacing));
                        if let Some((view_start, bar_spacing)) = viewport_state {
                            self.propagate_viewport_to_sync_group(hit_leaf_id, view_start, bar_spacing, Some(next_mode));
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
        }

        // 2. Sub-pane overlay button check — clicks on delete/hide/move-up/expand
        //    buttons are handled here before any other canvas logic.
        //
        //    IMPORTANT: `pane_index` from hit_test is the index in overlay_results
        //    (which may be filtered, e.g. only the maximized pane).  We resolve
        //    the actual sub-pane via `instance_id` from the overlay result.
        {
            let overlay_results_btn = self.panel_app.panel_grid.active_container()
                .map(|w| w.sub_pane_overlay_results.clone())
                .unwrap_or_default();
            let _fallback_btn;
            let extended_btn = match self.active_frame_layout.as_ref() {
                Some(e) => e,
                None => { _fallback_btn = self.build_extended_layout(); &_fallback_btn }
            };
            let tester_btn = ExtendedLayoutHitTester::new(extended_btn)
                .with_overlays(&overlay_results_btn);
            use mlc_core::input::ChartHitTester;
            if let mlc_core::engine::input::HitResult::SubPaneOverlayButton { pane_index, button } = tester_btn.hit_test(x, y) {
                // Resolve instance_id from overlay results (correct even when maximized).
                let instance_id = overlay_results_btn.get(pane_index)
                    .map(|o| o.instance_id)
                    .unwrap_or(0);
                if instance_id == 0 { return; }

                // Find the tree-order index for this instance_id.
                let real_index = self.panel_app.panel_grid.active_container()
                    .and_then(|w| {
                        crate::chart_container::collect_inner_instance_ids(&w.inner_docking)
                            .iter()
                            .position(|&iid| iid == instance_id)
                    });

                use mlc_core::state::modal_data::SubPaneButton;
                match button {
                    SubPaneButton::Delete => {
                        self.delete_indicator_instance(instance_id);
                    }
                    SubPaneButton::Hide => {
                        // Single source of truth: instance.visible controls
                        // both overlay and sub-pane indicator visibility.
                        // sub_pane.hidden is not used — render filters by instance.visible.
                        self.core.indicators_mut().toggle_visibility(instance_id);
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                    SubPaneButton::MoveUp => {
                        // Move this sub-pane up by one visual position.
                        //
                        // Three cases:
                        //   A. Pane is BELOW main and is the topmost below-pane
                        //      -> promote to above-main (insert at the BOTTOM of the
                        //         above group, i.e. closest to the main chart).
                        //   B. Pane is already ABOVE main (not the topmost above-pane)
                        //      -> swap within the inner_docking tree so it renders
                        //         higher in the above group.
                        //   C. Pane is the topmost above-pane -> no-op (already at top).
                        //
                        // The inner_docking tree stores ALL panes in below order and
                        // drives proportions/identity. above_main is a presentation
                        // overlay on the geometry side; no tree reorder is needed for
                        // promotion — only for intra-above visual ordering.
                        if let Some(tree_idx) = real_index {
                            let is_above = self.panel_app.panel_grid.active_container()
                                .map(|w| w.sub_pane_above_main.contains(&instance_id))
                                .unwrap_or(false);

                            if is_above {
                                // Case B/C: already above. Swap in tree so the pane
                                // moves to a lower tree_idx (= visually higher since
                                // above panes are rendered from tree order top-to-bottom).
                                let from = tree_idx + 1;
                                if from > 1 {
                                    let to = from - 1;
                                    let all_ids = self.panel_app.panel_grid.active_container()
                                        .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                        .unwrap_or_default();
                                    let neighbor_id = all_ids.get(tree_idx.saturating_sub(1)).copied();
                                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                        let root_id = window.inner_docking.tree().root().id;
                                        window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                                    }
                                    if let (Some(nbr_id), Some(bid)) = (neighbor_id, self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id)) {
                                        self.core.indicators_mut().swap_pane_order(bid, instance_id, nbr_id);
                                    }
                                }
                            } else {
                                // Case A: below main. Check if this is the topmost below pane.
                                // "Topmost below" means no other below pane has a lower tree_idx.
                                let is_topmost_below = self.panel_app.panel_grid.active_container()
                                    .map(|w| {
                                        let all_ids = crate::chart_container::collect_inner_instance_ids(&w.inner_docking);
                                        // tree_idx is the index of this pane in all_ids.
                                        // It is the topmost below pane if all panes with
                                        // lower tree_idx are already above main.
                                        all_ids.iter().take(tree_idx).all(|id| w.sub_pane_above_main.contains(id))
                                    })
                                    .unwrap_or(false);

                                if is_topmost_below {
                                    // Promote: add to above_main set.
                                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                        window.sub_pane_above_main.insert(instance_id);
                                    }
                                    // No tree reorder needed — above-main is presentation only.
                                } else {
                                    // Normal within-below swap.
                                    let from = tree_idx + 1;
                                    if from > 1 {
                                        let to = from - 1;
                                        let all_ids = self.panel_app.panel_grid.active_container()
                                            .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                            .unwrap_or_default();
                                        let neighbor_id = all_ids.get(tree_idx.saturating_sub(1)).copied();
                                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                            let root_id = window.inner_docking.tree().root().id;
                                            window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                                        }
                                        if let (Some(nbr_id), Some(bid)) = (neighbor_id, self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id)) {
                                            self.core.indicators_mut().swap_pane_order(bid, instance_id, nbr_id);
                                        }
                                    }
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::MoveDown => {
                        // Move this sub-pane down by one visual position. Mirrors MoveUp.
                        //
                        // Three cases:
                        //   A. Pane is ABOVE main and is the bottommost above-pane
                        //      -> demote back to below-main (top of the below group).
                        //   B. Pane is ABOVE main and not the bottommost
                        //      -> swap in tree to move it visually lower (higher tree_idx).
                        //   C. Pane is BELOW main -> normal swap within below group.
                        if let Some(tree_idx) = real_index {
                            let is_above = self.panel_app.panel_grid.active_container()
                                .map(|w| w.sub_pane_above_main.contains(&instance_id))
                                .unwrap_or(false);

                            if is_above {
                                // Check if this is the bottommost above-pane (all panes
                                // after tree_idx are below-main or there are none).
                                let is_bottommost_above = self.panel_app.panel_grid.active_container()
                                    .map(|w| {
                                        let all_ids = crate::chart_container::collect_inner_instance_ids(&w.inner_docking);
                                        all_ids.iter().skip(tree_idx + 1).all(|id| !w.sub_pane_above_main.contains(id))
                                    })
                                    .unwrap_or(false);

                                if is_bottommost_above {
                                    // Demote: remove from above_main set.
                                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                        window.sub_pane_above_main.remove(&instance_id);
                                    }
                                } else {
                                    // Swap within above group (move to higher tree_idx = lower on screen).
                                    let from = tree_idx + 1;
                                    let total = self.panel_app.panel_grid.active_container()
                                        .map(|w| w.inner_docking.tree().root().children.len())
                                        .unwrap_or(0);
                                    let to = from + 1;
                                    if to < total {
                                        let all_ids = self.panel_app.panel_grid.active_container()
                                            .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                            .unwrap_or_default();
                                        let neighbor_id = all_ids.get(tree_idx + 1).copied();
                                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                            let root_id = window.inner_docking.tree().root().id;
                                            window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                                        }
                                        if let (Some(nbr_id), Some(bid)) = (neighbor_id, self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id)) {
                                            self.core.indicators_mut().swap_pane_order(bid, instance_id, nbr_id);
                                        }
                                    }
                                }
                            } else {
                                // Normal within-below swap.
                                let from = tree_idx + 1;
                                let total = self.panel_app.panel_grid.active_container()
                                    .map(|w| w.inner_docking.tree().root().children.len())
                                    .unwrap_or(0);
                                let to = from + 1;
                                if to < total {
                                    let all_ids = self.panel_app.panel_grid.active_container()
                                        .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                        .unwrap_or_default();
                                    let neighbor_id = all_ids.get(tree_idx + 1).copied();
                                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                                        let root_id = window.inner_docking.tree().root().id;
                                        window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                                    }
                                    if let (Some(nbr_id), Some(bid)) = (neighbor_id, self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id)) {
                                        self.core.indicators_mut().swap_pane_order(bid, instance_id, nbr_id);
                                    }
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::Expand => {
                        // Maximize this sub-pane so it fills the ENTIRE container area
                        // (main chart and other sub-panes hidden).
                        //
                        // The layout override happens in render_full_chart_panel and
                        // build_extended_layout[_for_leaf] when maximized_sub_panes
                        // contains the entry — no tree proportion manipulation needed.
                        // We save the current tree proportions here so Restore can
                        // put them back exactly.
                        if let Some(cid) = self.panel_app.panel_grid.active_container_id() {
                            let sub_count = self.panel_app.panel_grid.active_container()
                                .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking).len())
                                .unwrap_or(0);
                            if sub_count > 0 {
                                let pre_props = self.panel_app.panel_grid.active_container()
                                    .map(|w| w.inner_docking.tree().root().proportions.clone())
                                    .unwrap_or_default();
                                self.maximized_sub_panes.insert(cid.0, (instance_id, pre_props));
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::Restore => {
                        // Remove the maximized entry — layout reverts to normal tree
                        // proportions on the next frame automatically.
                        if let Some(cid) = self.panel_app.panel_grid.active_container_id() {
                            self.maximized_sub_panes.remove(&cid.0);
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::IndicatorEye => {
                        self.core.indicators_mut().toggle_visibility(instance_id);
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                    SubPaneButton::IndicatorAlert => {
                        // Open the alert creation modal for this indicator instance.
                        // Uses AlertSource::Indicator with output_index=0 (first output line).
                        // The indicator's display name is used as the alert label.
                        let alert_symbol: String = self.panel_app.panel_grid.active_container_id()
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let price_fallback = self.panel_app.panel_grid.active_container()
                            .and_then(|_w| None::<&mlc_core::Bar>)
                            .map(|b| b.close)
                            .unwrap_or(0.0);
                        let label = self.core.indicators()
                            .name_of(instance_id)
                            .unwrap_or_else(|| format!("Indicator #{}", instance_id));
                        let source = mlc_core::alert::AlertSource::Indicator {
                            indicator_id: instance_id,
                            output_index: 0,
                            label,
                        };
                        self.panel_app.alert_settings_state.open_new(source, &alert_symbol, price_fallback);
                        self.panel_app.alert_settings_state.pin_initial_position(
                            self.content_rect.width, self.content_rect.height,
                        );
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
            // Read the frame-cached layout so we get the exact same main_chart rect
            // that render_full_chart_panel uses.  This accounts for sub-panes
            // (RSI, MACD, …) that reduce the main chart height below
            // viewport.chart_height.
            let _fallback_tl;
            let extended = match self.active_frame_layout.as_ref() {
                Some(e) => e,
                None => { _fallback_tl = self.build_extended_layout(); &_fallback_tl }
            };
            let chart_rect = extended.main_chart.chart;

            let active_bid_tool = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            let is_freehand = active_bid_tool
                .map(|bid| self.core.drawings().is_freehand_tool(bid))
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_container()
                    .map(|w| (w.price_scale().price_min, w.price_scale().price_max))
                    .unwrap_or((0.0, 100.0));

                let chart_height = chart_rect.height;
                let bar = self.panel_app.panel_grid.active_container()
                    .map(|w| {
                        // Snap to bar center (matching crosshair coordinate system).
                        if let Some(idx) = w.viewport().x_to_bar(local_x_main) {
                            idx as f64
                        } else {
                            w.viewport().x_to_bar_f64(local_x_main)
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
                let magnet_bars = self.active_chart_bars();
                let price = self.panel_app.panel_grid.active_container()
                    .map(|w| {
                        if w.crosshair.is_magnet() {
                            let bar_idx = w.viewport().x_to_bar(local_x_main);
                            let (snapped_price, _) = w.calculate_magnet_snap(
                                &magnet_bars, bar_idx, raw_price, chart_height, price_min, price_max,
                            );
                            snapped_price
                        } else {
                            raw_price
                        }
                    })
                    .unwrap_or(raw_price);

                // Reset pane context to main chart.
                if let Some(bid) = active_bid_tool {
                    let mut guard = self.core.drawings_mut();
                    guard.set_current_pane(bid, None);
                    if guard.current_tool(bid).map(|t| t != _tool_id).unwrap_or(true) {
                        guard.set_tool(bid, Some(_tool_id));
                    }
                }

                // Dispatch click and record undo if a primitive was just completed.
                eprintln!("[CANVAS_CLICK] screen=({:.0},{:.0}) -> bar={:.2}, price={:.12e}, price_range=[{:.12e}..{:.12e}]", x, y, bar, price, price_min, price_max);
                let click_bars_main: Vec<mlc_core::Bar> = self.active_chart_bars();
                let ts_ms = self.panel_app.panel_grid.active_container()
                    .map(|_w| mlc_core::bar_f64_to_timestamp_ms(&click_bars_main[..], bar))
                    .unwrap_or(0);
                let primitive_created = active_bid_tool
                    .map(|bid| self.core.drawings_mut().on_canvas_click(bid, ts_ms, price))
                    .unwrap_or(false);

                if primitive_created {
                    let prim_count = active_bid_tool
                        .map(|bid| self.core.drawings().count(bid)).unwrap_or(0);
                    eprintln!("[ChartApp] Primitive created (main) at bar={:.2}, price={:.12e}, total_primitives={}", bar, price, prim_count);
                    self.intercept_completed_primitive_to_group();
                    // Record undo and propagate to color-tag peers.
                    let create_snapshot = active_bid_tool
                        .and_then(|bid| {
                            let guard = self.core.drawings();
                            let prim_id = guard.last_index(bid)?;
                            let type_id = guard.get_type_id(bid, prim_id)?;
                            let points = guard.get_points(bid, prim_id)?;
                            let data = guard.get_data(bid, prim_id)?;
                            Some((prim_id, type_id, points, data))
                        });
                    if let Some((prim_id, type_id, points, data)) = create_snapshot {
                        self.push_undo_command(mlc_core::Command::CreatePrimitive {
                            prim_id,
                            type_id,
                            points,
                            data,
                        });
                        eprintln!("[ChartApp] Recorded CreatePrimitive (main) {}", prim_id);
                    }
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_new_primitive_to_sync_group(active_leaf);
                        // Clear the in-progress preview on peers (state is now Idle).
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                    self.sidebar_data_dirty = true;
                    self.autosave_snapshot();
                } else if active_bid_tool
                    .map(|bid| self.core.drawings().is_drawing(bid))
                    .unwrap_or(false)
                {
                    eprintln!("[ChartApp] Drawing point added (main) at bar={:.2}, price={:.2}", bar, price);
                    // Propagate in-progress drawing state so peers show the preview.
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                }

                // Re-check if tool was completed (primed tools deactivate after creation).
                let tool_cleared = active_bid_tool
                    .map(|bid| self.core.drawings().current_tool(bid).is_none())
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_container()
                    .and_then(|w| {
                        w.sub_pane_y_state
                            .get(&instance_id)
                            .map(|s| (s.price_min, s.price_max))
                    })
                    .unwrap_or((0.0, 100.0));

                let bar = self.panel_app.panel_grid.active_container()
                    .map(|w| {
                        // Snap to bar center (matching crosshair coordinate system).
                        if let Some(idx) = w.viewport().x_to_bar(local_x) {
                            idx as f64
                        } else {
                            w.viewport().x_to_bar_f64(local_x)
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
                if let Some(bid) = active_bid_tool {
                    let mut guard = self.core.drawings_mut();
                    guard.set_current_pane(bid, Some(instance_id));
                    if guard.current_tool(bid).map(|t| t != _tool_id).unwrap_or(true) {
                        guard.set_tool(bid, Some(_tool_id));
                    }
                }

                // Dispatch click and record undo if a primitive was just completed.
                let click_bars_sub: Vec<mlc_core::Bar> = self.active_chart_bars();
                let sub_ts_ms = self.panel_app.panel_grid.active_container()
                    .map(|_w| mlc_core::bar_f64_to_timestamp_ms(&click_bars_sub[..], bar))
                    .unwrap_or(0);
                let primitive_created = active_bid_tool
                    .map(|bid| self.core.drawings_mut().on_canvas_click(bid, sub_ts_ms, price))
                    .unwrap_or(false);

                if primitive_created {
                    eprintln!("[ChartApp] Primitive created (sub-pane #{}) at bar={:.2}, price={:.2}", pane_idx, bar, price);
                    self.intercept_completed_primitive_to_group();
                    // Extract snapshot then push undo command.
                    let create_snapshot = active_bid_tool
                        .and_then(|bid| {
                            let guard = self.core.drawings();
                            let prim_id = guard.last_index(bid)?;
                            let type_id = guard.get_type_id(bid, prim_id)?;
                            let points = guard.get_points(bid, prim_id)?;
                            let data = guard.get_data(bid, prim_id)?;
                            Some((prim_id, type_id, points, data))
                        });
                    if let Some((prim_id, type_id, points, data)) = create_snapshot {
                        self.push_undo_command(mlc_core::Command::CreatePrimitive {
                            prim_id,
                            type_id,
                            points,
                            data,
                        });
                        eprintln!("[ChartApp] Recorded CreatePrimitive (sub-pane #{}) {}", pane_idx, prim_id);
                    }
                    // Propagate the new primitive to sync group peers.
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_new_primitive_to_sync_group(active_leaf);
                        // Clear the in-progress preview on peers (state is now Idle).
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                    self.sidebar_data_dirty = true;
                    self.autosave_snapshot();
                } else if active_bid_tool
                    .map(|bid| self.core.drawings().is_drawing(bid))
                    .unwrap_or(false)
                {
                    eprintln!("[ChartApp] Drawing point added (sub-pane #{}) at bar={:.2}, price={:.2}", pane_idx, bar, price);
                    // Propagate in-progress drawing state so peers show the preview.
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                }

                // Sync toolbar if tool completed.
                let tool_cleared = active_bid_tool
                    .map(|bid| self.core.drawings().current_tool(bid).is_none())
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
            let _fallback_cc2;
            let extended = match self.active_frame_layout.as_ref() {
                Some(e) => e,
                None => { _fallback_cc2 = self.build_extended_layout(); &_fallback_cc2 }
            };
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;

            // -- Main chart primitive hit test --
            let active_bid_hit = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            if local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height
            {
                let hit_bars_main: Vec<mlc_core::Bar> = self.active_chart_bars();
                if let Some(window) = self.panel_app.panel_grid.active_container() {
                    // Use corrected viewport dimensions from the layout so that
                    // hit-test coordinate mapping matches the render path exactly.
                    let mut corrected_vp = window.viewport().clone();
                    corrected_vp.chart_width = chart_rect.width;
                    corrected_vp.chart_height = chart_rect.height;
                    let hit_prim_id = active_bid_hit
                        .and_then(|bid| self.core.drawings().hit_test(bid, local_x, local_y, &hit_bars_main[..], &corrected_vp, &window.price_scale()));
                    if let Some(hit_prim_id) = hit_prim_id {
                        // Select the primitive, clear indicator selection, and return.
                        self.selected_indicator_id = None;
                        // Deselect all other bubbles first so control points don't accumulate.
                        let all_bids: Vec<_> = self.panel_app.panel_grid.containers().values()
                            .map(|w| w.current_tag_id)
                            .collect();
                        for bid in all_bids {
                            if Some(bid) != active_bid_hit {
                                self.core.drawings_mut().deselect(bid);
                            }
                        }
                        if let Some(bid) = active_bid_hit {
                            let mut guard = self.core.drawings_mut();
                            guard.set_current_pane(bid, None);
                            guard.select(bid, hit_prim_id);
                            eprintln!("[ChartApp] Primitive selected (main): id={}", hit_prim_id);
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_container()
                    .and_then(|w| {
                        w.sub_pane_y_state
                            .get(&instance_id)
                            .map(|s| (s.price_min, s.price_max))
                    })
                    .unwrap_or((0.0, 100.0));
                let sub_price_scale = mlc_core::PriceScale::new(price_min, price_max);
                // Use a viewport with pane height so hit_test_in_pane coordinate conversion is correct.
                let sub_viewport = self.panel_app.panel_grid.active_container()
                    .map(|w| {
                        let mut vp = w.viewport().clone();
                        vp.chart_height = content.height;
                        vp
                    });
                let hit_bars_sub: Vec<mlc_core::Bar> = self.active_chart_bars();
                if let (Some(sub_viewport), Some(_window)) = (sub_viewport, self.panel_app.panel_grid.active_container()) {
                    let sub_hit_prim_id = active_bid_hit
                        .and_then(|bid| self.core.drawings().hit_test_in_pane(bid, plx, ply, instance_id, &hit_bars_sub[..], &sub_viewport, &sub_price_scale));
                    if let Some(hit_prim_id) = sub_hit_prim_id {
                        // Select the primitive, clear indicator selection, and return.
                        self.selected_indicator_id = None;
                        // Deselect all other bubbles.
                        let all_bids2: Vec<_> = self.panel_app.panel_grid.containers().values()
                            .map(|w| w.current_tag_id)
                            .collect();
                        for bid in all_bids2 {
                            if Some(bid) != active_bid_hit {
                                self.core.drawings_mut().deselect(bid);
                            }
                        }
                        if let Some(bid) = active_bid_hit {
                            let mut guard = self.core.drawings_mut();
                            guard.set_current_pane(bid, Some(instance_id));
                            guard.select(bid, hit_prim_id);
                            eprintln!("[ChartApp] Primitive selected (sub-pane #{}): id={}", pane_idx, hit_prim_id);
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_container()
                    .and_then(|w| {
                        w.sub_pane_y_state
                            .get(&instance_id)
                            .map(|s| (s.price_min, s.price_max))
                    })
                    .unwrap_or((0.0, 100.0));
                let hit = self.panel_app.panel_grid.active_container()
                    .map(|window| {
                        self.core.indicators().hit_test_sub_pane(
                            instance_id,
                            plx, ply,
                            &window.viewport(),
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
                        let all_bids: Vec<_> = self.core.bubbles().bubble_ids().collect();
                        for bid in all_bids {
                            self.core.drawings_mut().deselect(bid);
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
                let ind_hit = self.panel_app.panel_grid.active_container()
                    .and_then(|window| {
                        self.core.indicators().hit_test_overlay(
                            window.current_tag_id,
                            local_x,
                            local_y,
                            &window.viewport(),
                            &window.price_scale(),
                            chart_rect.height,
                            8.0,
                        )
                    });
                if let Some(ind_id) = ind_hit {
                    self.selected_indicator_id = Some(ind_id);
                    // Deselect any drawing primitive across ALL bubbles when an indicator is selected.
                    let all_bids: Vec<_> = self.core.bubbles().bubble_ids().collect();
                    for bid in all_bids {
                        self.core.drawings_mut().deselect(bid);
                    }
                    eprintln!("[ChartApp] Indicator selected: id={}", ind_id);
                    return;
                }
            }
        }

        // No primitive or indicator hit — deselect everything across ALL bubbles.
        self.selected_indicator_id = None;
        let all_bids2: Vec<_> = self.core.bubbles().bubble_ids().collect();
        for bid in all_bids2 {
            self.core.drawings_mut().deselect(bid);
        }

        let _fallback_cc3;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_cc3 = self.build_extended_layout(); &_fallback_cc3 }
        };
        let overlay_results_cc = self.panel_app.panel_grid.active_container()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
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
                    self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
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
                            mlc_app_layout::ui::modal_settings::ProfileManagerPage::ProfileList;
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
                    self.standalone_panels.wl_group_name_input.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: wl_group_name_input closed");
                }
                // Watchlist modal — close when user clicks outside it.
                "watchlist_modal" => {
                    self.standalone_panels.watchlist_modal.close();
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
            self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
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
            ContextMenuTarget::Primitive(prim_id) => {
                let active_bid_ctx = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                match action {
                    "settings" => {
                        self.panel_app.primitive_settings_state.open(prim_id);
                        eprintln!("[ChartApp] Opened primitive settings for #{}", prim_id);
                    }
                    "delete" => {
                        // Capture state BEFORE deletion so undo can recreate.
                        let snapshot = active_bid_ctx
                            .and_then(|bid| {
                                let guard = self.core.drawings();
                                let type_id = guard.get_type_id(bid, prim_id)?;
                                let points = guard.get_points(bid, prim_id)?;
                                let data = guard.get_data(bid, prim_id)?;
                                Some((type_id, points, data))
                            });
                        if let Some((type_id, points, data)) = snapshot {
                            self.push_undo_command(mlc_core::Command::DeletePrimitive {
                                prim_id,
                                type_id,
                                points,
                                data,
                            });
                            eprintln!("[ChartApp] Recorded DeletePrimitive {}", prim_id);
                            if let Some(bid) = active_bid_ctx {
                                self.core.drawings_mut().delete(bid, prim_id);
                                eprintln!("[ChartApp] Deleted primitive #{}", prim_id);
                            }
                            {
                                let removed = self.core.alerts_mut().clear_for_drawing(prim_id);
                                for alert_snapshot in removed {
                                    self.push_undo_command(mlc_core::Command::DeleteAlert { snapshot: alert_snapshot });
                                }
                            }
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                        } else {
                            // No undo snapshot — still remove and clean up alerts.
                            if let Some(bid) = active_bid_ctx {
                                self.core.drawings_mut().delete(bid, prim_id);
                                eprintln!("[ChartApp] Deleted primitive #{} (no undo snapshot)", prim_id);
                            }
                            {
                                let removed = self.core.alerts_mut().clear_for_drawing(prim_id);
                                for alert_snapshot in removed {
                                    self.push_undo_command(mlc_core::Command::DeleteAlert { snapshot: alert_snapshot });
                                }
                            }
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                        }
                    }
                    "clone" => {
                        let cloned_id = active_bid_ctx
                            .and_then(|bid| self.core.drawings_mut().clone_primitive(bid, prim_id));
                        if let Some(new_id) = cloned_id {
                            // Apply +20px right, -20px up offset to the cloned primitive.
                            // Convert screen pixels to bar/price coordinates using the viewport.
                            let clone_bars: Vec<mlc_core::Bar> = self.active_chart_bars();
                            if let Some(w) = self.panel_app.panel_grid.active_container() {
                                let bar_spacing = w.viewport().bar_spacing;
                                let chart_height = w.viewport().chart_height;
                                let price_range = w.price_scale().price_max - w.price_scale().price_min;
                                // 20px right = 20 / bar_spacing bars → convert to ms
                                let bars_delta = 20.0 / bar_spacing.max(0.001);
                                let bar_interval_ms = mlc_core::bar_interval_seconds(&clone_bars[..]) * 1000;
                                let ts_delta_ms = (bars_delta * bar_interval_ms as f64).round() as i64;
                                // 20px up = 20 / chart_height * price_range (up = positive price)
                                let price_delta = if chart_height > 0.0 && price_range > 0.0 {
                                    20.0 / chart_height * price_range
                                } else {
                                    0.0
                                };
                                if let Some(bid) = active_bid_ctx {
                                    let mut guard = self.core.drawings_mut();
                                    guard.translate(bid, new_id, ts_delta_ms, price_delta);
                                    guard.select(bid, new_id);
                                }
                            }
                            // Capture the NEW primitive's data for undo.
                            let snapshot = active_bid_ctx
                                .and_then(|bid| {
                                    let guard = self.core.drawings();
                                    let type_id = guard.get_type_id(bid, new_id)?;
                                    let points = guard.get_points(bid, new_id)?;
                                    let data = guard.get_data(bid, new_id)?;
                                    Some((type_id, points, data))
                                });
                            if let Some((type_id, points, data)) = snapshot {
                                self.push_undo_command(mlc_core::Command::CreatePrimitive {
                                    prim_id: new_id,
                                    type_id,
                                    points,
                                    data,
                                });
                                eprintln!("[ChartApp] Recorded CreatePrimitive (clone) {}", new_id);
                            }
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                            eprintln!("[ChartApp] Cloned primitive #{} -> #{}", prim_id, new_id);
                        } else {
                            eprintln!("[ChartApp] Clone primitive #{} returned None", prim_id);
                        }
                    }
                    "toggle_lock" => {
                        // Read current state BEFORE mutating.
                        let previous = active_bid_ctx
                            .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                            .map(|d| d.locked)
                            .unwrap_or(false);
                        self.push_undo_command(mlc_core::Command::SetPrimitiveLock {
                            prim_id,
                            locked: !previous,
                            previous,
                        });
                        eprintln!("[ChartApp] Recorded SetPrimitiveLock {} locked={}", prim_id, !previous);
                        if let Some(bid) = active_bid_ctx {
                            self.core.drawings_mut().toggle_lock_primitive(bid, prim_id);
                        }
                        self.sidebar_data_dirty = true;
                    }
                    "toggle_visibility" => {
                        // Read current state BEFORE mutating.
                        let previous = active_bid_ctx
                            .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                            .map(|d| d.visible)
                            .unwrap_or(true);
                        self.push_undo_command(mlc_core::Command::SetPrimitiveVisibility {
                            prim_id,
                            visible: !previous,
                            previous,
                        });
                        eprintln!("[ChartApp] Recorded SetPrimitiveVisibility {} visible={}", prim_id, !previous);
                        if let Some(bid) = active_bid_ctx {
                            self.core.drawings_mut().toggle_visibility(bid, prim_id);
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
                            crate::ChartApp::build_demo_symbol_results("", self.core.watchlists().find(self.watchlist_ui.active_list_id), self.core.exchange_symbols().as_map());
                        {
                            let sym_id = uzor::WidgetId::from(crate::text_input::SYMBOL_SEARCH);
                            self.input_coordinator.borrow_mut().text_fields_mut().clear(&sym_id);
                            self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&sym_id);
                            self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::SYMBOL_SEARCH);
                        }
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
