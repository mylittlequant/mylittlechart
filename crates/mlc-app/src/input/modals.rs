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
use uzor::WidgetId;

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
                // Alerts attached to the deleted drawing go with it, and come
                // back with it: each is recorded so one Ctrl+Z restores the
                // drawing AND its alerts.
                let removed_alerts = self.core.alerts_mut().clear_for_drawing(prim_id);
                for snapshot in removed_alerts {
                    self.push_undo_command(mlc_core::Command::DeleteAlert { snapshot });
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
                    // Bind the read FIRST: `self.core.drawings()` owns a
                    // RwLockReadGuard, and an `if let` keeps its scrutinee's
                    // temporaries alive for the whole body — so calling
                    // `drawings_mut()` inside would re-acquire the same lock
                    // while still read-locked. On wasm that is `rtabort!`, a
                    // hard trap that leaves the lock permanently corrupted and
                    // kills every later frame. Same trap as the five sites
                    // fixed in `settings.rs` (8ac92db1).
                    let existing = self.core.drawings().get_data(bid, prim_id);
                    if let Some(mut data) = existing {
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

            // ── Wave button — toggle wave-slot picker dropdown (E2) ──────────
            "inline:wave" => {
                let was_open = self.panel_app.toolbar_state.open_inline_wave_dropdown;
                // Close all four dropdowns first, then toggle — same shape
                // as the style/width chevrons above. Unlike those, this
                // button is not a split button (there is no "cycle to the
                // next slot" main-area action), so the whole button IS the
                // toggle trigger.
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.open_inline_wave_dropdown = false;
                self.panel_app.toolbar_state.open_inline_wave_next_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
                if !was_open {
                    self.panel_app.toolbar_state.open_inline_wave_dropdown = true;
                }
                eprintln!("[ChartApp] inline: wave dropdown toggled to {}", self.panel_app.toolbar_state.open_inline_wave_dropdown);
            }

            // ── Wave-succession button — toggle wave-succession picker
            // dropdown (E3, "what comes after this" — the second, separate
            // wave control). Same shape as "inline:wave" above, its own
            // sibling dropdown flag.
            "inline:wave_next" => {
                let was_open = self.panel_app.toolbar_state.open_inline_wave_next_dropdown;
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.open_inline_wave_dropdown = false;
                self.panel_app.toolbar_state.open_inline_wave_next_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
                if !was_open {
                    self.panel_app.toolbar_state.open_inline_wave_next_dropdown = true;
                }
                eprintln!("[ChartApp] inline: wave-succession dropdown toggled to {}", self.panel_app.toolbar_state.open_inline_wave_next_dropdown);
            }

            // ── Background absorber — close inline dropdown ──────────────────
            "inline_dropdown:__bg__" => {
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.open_inline_wave_dropdown = false;
                self.panel_app.toolbar_state.open_inline_wave_next_dropdown = false;
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

            _ if action.starts_with("inline:wave_option:") => {
                // ── Wave slot selected — create the nested Elliott child
                // (E2). Mirrors the context menu's "clone" handler
                // (`on_context_menu_action`): create through the facade,
                // record `CreatePrimitive` for undo, sync back to the sync
                // group, autosave — the same path every other
                // programmatically-created primitive takes.
                use mlc_core::drawing::primitives_v2::elliott::WaveSlot;
                let slot_id = action.strip_prefix("inline:wave_option:").unwrap_or_default();
                if let Some(slot) = WaveSlot::from_id(slot_id) {
                    let new_id = active_bid_inline
                        .and_then(|bid| self.core.drawings_mut().create_wave_child(bid, prim_id, slot));
                    match new_id {
                        Some(new_id) => {
                            let cmd = active_bid_inline.and_then(|bid| self.build_create_primitive_command(bid, new_id));
                            if let Some(cmd) = cmd {
                                self.push_undo_command(cmd);
                                eprintln!("[ChartApp] Recorded CreatePrimitive (wave child) {}", new_id);
                            }
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                            eprintln!("[ChartApp] inline: created wave child #{} in slot {} of #{}", new_id, slot_id, prim_id);
                        }
                        None => {
                            eprintln!("[ChartApp] inline: create_wave_child returned None for slot {} of #{}", slot_id, prim_id);
                        }
                    }
                } else {
                    eprintln!("[ChartApp] inline: unknown wave slot id '{}'", slot_id);
                }
                self.panel_app.toolbar_state.open_inline_wave_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ if action.starts_with("inline:wave_next_option:") => {
                // ── Wave-succession type selected — create the Elliott
                // successor (E3). Mirrors the "inline:wave_option:" handler
                // above bit-for-bit, EXCEPT the primitive it creates
                // continues `prim_id` (`follows`) instead of nesting inside
                // it (`link`) — see `create_wave_successor`'s own doc for
                // why that is a different call, not the same one with a
                // different argument.
                let type_id = action.strip_prefix("inline:wave_next_option:").unwrap_or_default();
                let new_id = active_bid_inline
                    .and_then(|bid| self.core.drawings_mut().create_wave_successor(bid, prim_id, type_id));
                match new_id {
                    Some(new_id) => {
                        let cmd = active_bid_inline.and_then(|bid| self.build_create_primitive_command(bid, new_id));
                        if let Some(cmd) = cmd {
                            self.push_undo_command(cmd);
                            eprintln!("[ChartApp] Recorded CreatePrimitive (wave successor) {}", new_id);
                        }
                        self.sync_drawing_back_to_group();
                        self.autosave_snapshot();
                        eprintln!("[ChartApp] inline: created wave successor #{} ({}) after #{}", new_id, type_id, prim_id);
                    }
                    None => {
                        eprintln!("[ChartApp] inline: create_wave_successor returned None for type '{}' after #{}", type_id, prim_id);
                    }
                }
                self.panel_app.toolbar_state.open_inline_wave_next_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            // ── Promote-chain button (E4) — collapse the selected
            // primitive's own `follows`-linked same-degree chain into one
            // coarser structure. Single action, no dropdown (unlike
            // E2/E3): `promote_wave_chain` resolves the chain's own head
            // from `prim_id` internally (`selected_config`'s own
            // `can_promote` already ran the identical check to decide
            // whether this button even renders).
            "inline:wave_up" => {
                if let Some(bid) = active_bid_inline {
                    let result = self.core.drawings_mut().promote_wave_chain(bid, prim_id);
                    match result {
                        Some((parent_id, members)) => {
                            let cmd = self.core.drawings().get_snapshot(bid, parent_id).map(|parent_snapshot| {
                                mlc_core::Command::PromoteWaveChain { parent_id, parent_snapshot, members }
                            });
                            if let Some(cmd) = cmd {
                                self.push_undo_command(cmd);
                                eprintln!("[ChartApp] Recorded PromoteWaveChain parent=#{}", parent_id);
                            }
                            self.core.drawings_mut().select(bid, parent_id);
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                            eprintln!("[ChartApp] inline: promoted chain into #{}", parent_id);
                        }
                        None => {
                            eprintln!("[ChartApp] inline: promote_wave_chain returned None for #{}", prim_id);
                        }
                    }
                }
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
                // Hard cutover (2026-07-13 symbol-switch fix): this arm was a
                // second hand-rolled copy of the `ApplySymbol` handler — same
                // disease as the SymbolSearch arm (see its comment), same
                // dead-footprint symptom. One dispatch, one code path.
                if let Some(chart_id) = self.panel_app.panel_grid.active_container_id() {
                    self.apply_symbol(chart_id, sym_part, resolved_exchange.as_str(), &wl_at_label);
                }
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

    /// Handle clicks on widgets registered with the "modal_cmp:" prefix —
    /// the merged-mode compare-plus icon on a SymbolSearch modal row
    /// (symbol-search-compare-inner arc, Wave 2, кусок 5 — owner spec
    /// 2026-07-16). `rest` is the composite `"SYMBOL:exchange_id:
    /// account_type"` key, same shape `"star:"`/`"item:"` parse (rfind
    /// twice so symbols containing ':' are handled correctly).
    pub(crate) fn handle_search_modal_cmp_click(&mut self, rest: &str) {
        let (sym_part, exchange_part, at_label) =
            if let Some(at_pos) = rest.rfind(':') {
                let at_part = &rest[at_pos + 1..];
                let remainder = &rest[..at_pos];
                if let Some(ex_pos) = remainder.rfind(':') {
                    (&remainder[..ex_pos], &remainder[ex_pos + 1..], at_part.to_string())
                } else {
                    (remainder, self.active_exchange.as_str(), at_part.to_string())
                }
            } else {
                (rest, self.active_exchange.as_str(), "S".to_string())
            };
        self.add_compare_symbol(sym_part, exchange_part, &at_label);
        self.modal_state.close();
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
            "toggle_quick" => {
                // Header toggle (symbol-search-compare-inner arc, Wave 3 —
                // owner spec 2026-07-16): the ONLY UI path back to the
                // inline fast-search dropdown once the modal is open with
                // `symbol_quick_search == false`. Does NOT auto-close the
                // modal — same helper the toolbar dropdown's own toggle row
                // uses (`panel_click.rs::handle_symbol_quick_click`).
                self.toggle_symbol_quick_search();
            }
            "toggle_split" => {
                self.toggle_split_symbol_compare();
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
                        // Workspace-panel arc (Stage 3): when the Trading
                        // tab's container is the current workspace focus,
                        // the search commit targets IT instead of the
                        // active chart — see `commit_symbol_search_to_
                        // trading_panel`'s doc comment for the color-sync
                        // group mirror.
                        if let Some(pid) = self.workspace_focus().trading_panel() {
                            self.commit_symbol_search_to_trading_panel(
                                pid, symbol_part, resolved_exchange.as_str(), &item_at_label,
                            );
                        } else if let Some(chart_id) = self.panel_app.panel_grid.active_container_id() {
                            // Hard cutover (2026-07-13 symbol-switch fix): this
                            // arm used to hand-roll a byte-for-byte copy of the
                            // `ApplySymbol` handler (drawings swap, instrument
                            // rebind, unsubscribe, kline request, undo, sync
                            // group, recalc, autosave) — which silently missed
                            // every later addition to the canonical path, most
                            // recently the trade-plane re-ignite (footprint/CVD
                            // stayed dead after a modal symbol pick until a full
                            // reload). One dispatch, one code path.
                            self.apply_symbol(chart_id, symbol_part, resolved_exchange.as_str(), &item_at_label);
                        }
                        self.modal_state.close();
                    }
                    OpenModal::CompareSearch => {
                        // Hard cutover (2026-07-16, mirrors the ApplySymbol
                        // cutover at :742-750 above): compare-add used to be
                        // hand-rolled inline here, with no `ChartOutEvent`
                        // variant and no agent-reachable path. Now routed
                        // through the canonical `AddCompareSymbol` event —
                        // one dispatch, one code path.
                        self.add_compare_symbol(symbol_part, resolved_exchange.as_str(), &item_at_label);
                        self.modal_state.close();
                    }
                    OpenModal::IndicatorSearch => {
                        // Opaque host-driven overlays (Volume Profile / TPO Profile /
                        // OI Delta / Funding Rate / DOM Heatmap) are catalog entries
                        // for discoverability only — they carry no `machine_id` in
                        // mli and are not computed through `IndicatorManager`.
                        // Intercept here and dispatch the existing Toggle event
                        // instead of falling through to the regular add() path
                        // below.
                        //
                        // Nothing is intercepted here anymore (2026-08-03).
                        // Every host-rendered catalog entry falls through to
                        // the regular add() path and becomes an ordinary
                        // `IndicatorInstance`: legend row, object-tree node,
                        // gear, and — for the two sub-panes — a pane of its
                        // own. The intercept added in `adc5506d` dispatched a
                        // Toggle event "in place of the regular
                        // `IndicatorManager::add` path", which is exactly why
                        // picking Volume Profile / TPO Profile / OI Delta /
                        // Funding Rate out of the indicator list never
                        // produced an indicator. Liquidation Heatmap was
                        // freed first (wave 1, 2026-08-01), DOM Heatmap
                        // second; these four close the set.
                        // Trading-panel focus (chart-in-container arc,
                        // 2026-07-26): the picked indicator lands on the
                        // embedded center chart's bubble, not the active
                        // grid chart — same focus-routing contract the
                        // symbol search commit uses. The aux chart lives on
                        // its own auto-bubble (never group-synced), so this
                        // is the sync-off branch of the flow below, reduced.
                        if let crate::focus::WorkspaceFocus::TradingPanel(tpid) = self.workspace_focus {
                            if let Some(&aux_cid) = self.trading_center_charts.get(&tpid) {
                                let aux_bid = self.panel_app.panel_grid.containers()
                                    .get(&aux_cid)
                                    .map(|w| w.current_tag_id);
                                if let Some(bid) = aux_bid {
                                    let auto_color: Option<String> =
                                        self.core.indicators_mut().pick_overlay_color(item_id, bid);
                                    let new_id_opt = self.core.indicators_mut().add(Some(bid), item_id);
                                    if let Some(new_id) = new_id_opt {
                                        if let Some(ref color) = auto_color {
                                            self.core.indicators_mut().set_all_output_colors(new_id, color.clone());
                                        }
                                        // Record on the aux auto-bubble so desync
                                        // logic never treats it as group-owned.
                                        self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                                            auto_bub.pre_tag_indicator_ids.push(new_id);
                                        });
                                        let bars_opt: Option<Vec<mlc_core::Bar>> =
                                            self.with_bubble(bid, |bub| bub.bars.clone());
                                        if let Some(bars) = bars_opt {
                                            self.core.indicators_mut().recalc_bubble(bid, &bars);
                                        }
                                        if item_id.eq_ignore_ascii_case("cvd") {
                                            self.subscribe_cvd_line_for_indicator_add(Some(aux_cid));
                                        }
                                        self.sync_sub_panes_from_manager();
                                        self.sidebar_data_dirty = true;
                                        eprintln!(
                                            "[TradingCenter] created indicator '{}' (id={}) on aux chart {:?} bubble {:?}",
                                            item_id, new_id, aux_cid, bid
                                        );
                                    }
                                    self.modal_state.close();
                                    return;
                                }
                            }
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
                            // Nothing overlay-specific here: the instance IS
                            // the overlay's state, and the bubble raises the
                            // feeds its indicators declared on the next tick
                            // (`ensure_bubble_indicator_feeds`).
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

    /// Sub-pane overlay button action, parameterized by TARGET chart —
    /// extracted verbatim from `handle_canvas_click`'s overlay-button
    /// branch (chart-in-container arc, 2026-07-26) so the trading
    /// container's embedded center chart shares the SAME hide/delete/
    /// move/maximize/left-button machinery. `target_cid =
    /// active_container_id()` reproduces the old behavior bit-for-bit.
    pub(crate) fn apply_sub_pane_overlay_button(
        &mut self,
        target_cid: Option<mlc_core::state::ChartId>,
        instance_id: u64,
        real_index: Option<usize>,
        button: mlc_core::state::modal_data::SubPaneButton,
    ) {
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
                    let is_above = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                        .map(|w| w.sub_pane_above_main.contains(&instance_id))
                        .unwrap_or(false);

                    if is_above {
                        // Case B/C: already above. Swap in tree so the pane
                        // moves to a lower tree_idx (= visually higher since
                        // above panes are rendered from tree order top-to-bottom).
                        let from = tree_idx + 1;
                        if from > 1 {
                            let to = from - 1;
                            let all_ids = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                                .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                .unwrap_or_default();
                            let neighbor_id = all_ids.get(tree_idx.saturating_sub(1)).copied();
                            if let Some(window) = target_cid.and_then(|c| self.panel_app.panel_grid.containers_mut().get_mut(&c)) {
                                let root_id = window.inner_docking.tree().root().id;
                                window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                            }
                            if let (Some(nbr_id), Some(bid)) = (neighbor_id, target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c)).map(|w| w.current_tag_id)) {
                                self.core.indicators_mut().swap_pane_order(bid, instance_id, nbr_id);
                            }
                        }
                    } else {
                        // Case A: below main. Check if this is the topmost below pane.
                        // "Topmost below" means no other below pane has a lower tree_idx.
                        let is_topmost_below = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
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
                            if let Some(window) = target_cid.and_then(|c| self.panel_app.panel_grid.containers_mut().get_mut(&c)) {
                                window.sub_pane_above_main.insert(instance_id);
                            }
                            // No tree reorder needed — above-main is presentation only.
                        } else {
                            // Normal within-below swap.
                            let from = tree_idx + 1;
                            if from > 1 {
                                let to = from - 1;
                                let all_ids = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                                    .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                    .unwrap_or_default();
                                let neighbor_id = all_ids.get(tree_idx.saturating_sub(1)).copied();
                                if let Some(window) = target_cid.and_then(|c| self.panel_app.panel_grid.containers_mut().get_mut(&c)) {
                                    let root_id = window.inner_docking.tree().root().id;
                                    window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                                }
                                if let (Some(nbr_id), Some(bid)) = (neighbor_id, target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c)).map(|w| w.current_tag_id)) {
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
                    let is_above = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                        .map(|w| w.sub_pane_above_main.contains(&instance_id))
                        .unwrap_or(false);

                    if is_above {
                        // Check if this is the bottommost above-pane (all panes
                        // after tree_idx are below-main or there are none).
                        let is_bottommost_above = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                            .map(|w| {
                                let all_ids = crate::chart_container::collect_inner_instance_ids(&w.inner_docking);
                                all_ids.iter().skip(tree_idx + 1).all(|id| !w.sub_pane_above_main.contains(id))
                            })
                            .unwrap_or(false);

                        if is_bottommost_above {
                            // Demote: remove from above_main set.
                            if let Some(window) = target_cid.and_then(|c| self.panel_app.panel_grid.containers_mut().get_mut(&c)) {
                                window.sub_pane_above_main.remove(&instance_id);
                            }
                        } else {
                            // Swap within above group (move to higher tree_idx = lower on screen).
                            let from = tree_idx + 1;
                            let total = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                                .map(|w| w.inner_docking.tree().root().children.len())
                                .unwrap_or(0);
                            let to = from + 1;
                            if to < total {
                                let all_ids = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                                    .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                    .unwrap_or_default();
                                let neighbor_id = all_ids.get(tree_idx + 1).copied();
                                if let Some(window) = target_cid.and_then(|c| self.panel_app.panel_grid.containers_mut().get_mut(&c)) {
                                    let root_id = window.inner_docking.tree().root().id;
                                    window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                                }
                                if let (Some(nbr_id), Some(bid)) = (neighbor_id, target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c)).map(|w| w.current_tag_id)) {
                                    self.core.indicators_mut().swap_pane_order(bid, instance_id, nbr_id);
                                }
                            }
                        }
                    } else {
                        // Normal within-below swap.
                        let from = tree_idx + 1;
                        let total = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                            .map(|w| w.inner_docking.tree().root().children.len())
                            .unwrap_or(0);
                        let to = from + 1;
                        if to < total {
                            let all_ids = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                                .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                                .unwrap_or_default();
                            let neighbor_id = all_ids.get(tree_idx + 1).copied();
                            if let Some(window) = target_cid.and_then(|c| self.panel_app.panel_grid.containers_mut().get_mut(&c)) {
                                let root_id = window.inner_docking.tree().root().id;
                                window.inner_docking.tree_mut().reorder_children(root_id, from, to);
                            }
                            if let (Some(nbr_id), Some(bid)) = (neighbor_id, target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c)).map(|w| w.current_tag_id)) {
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
                if let Some(cid) = target_cid {
                    let sub_count = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
                        .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking).len())
                        .unwrap_or(0);
                    if sub_count > 0 {
                        let pre_props = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
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
                if let Some(cid) = target_cid {
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
                let alert_symbol: String = target_cid
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let price_fallback = target_cid.and_then(|c| self.panel_app.panel_grid.containers().get(&c))
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
    }

    /// Handle a click that landed on the chart canvas (not a widget).
    pub(super) fn handle_canvas_click(&mut self, x: f64, y: f64) {
        // Any click that reaches the canvas returns workspace focus to the
        // chart — even while the Trading tab is still open in the sidebar
        // (owner directive, 2026-07 workspace-panel arc: the two are equal
        // workspace panels, so focus follows whichever the user clicked,
        // not whichever the sidebar happens to display). Single-window mode
        // never calls `set_active_leaf` before reaching here, so this is
        // the one hook that covers it; split mode's `set_active_leaf`
        // callers (`on_click`'s `ScaleCorner` branch, `on_right_click`) set
        // it themselves since they return before reaching this function.
        if let Some(cid) = self.panel_app.panel_grid.active_container_id() {
            self.workspace_focus = crate::focus::WorkspaceFocus::Chart(cid);
        }

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

                let target = self.panel_app.panel_grid.active_container_id();
                self.apply_sub_pane_overlay_button(target, instance_id, real_index, button);
                return;
            }
        }

        // 3. Drawing tool check — if a tool is active, convert the click to
        //    chart-local data coordinates and forward to DrawingManager.on_click().
        //
        //    Handles both main chart and sub-pane areas.  Sub-panes share the
        //    X-axis (bar index) with the main chart but have their own Y-axis
        //    (price range = indicator values, e.g. RSI 0-100).
        //    Cursor-section tools (crosshair, hand) are navigation modes, not
        //    primitive creators — they must fall through to selection below.
        let tool_id_opt = self
            .panel_app
            .toolbar_state
            .active_tool_id
            .clone()
            .filter(|id| !mlc_core::toolbar::is_cursor_tool(id));
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
                // The window magnet can land on a neighbouring bar — `bar` travels
                // with it so the click's timestamp matches the bar the price was
                // snapped to (see `calculate_magnet_snap`'s own doc).
                let magnet_bars = self.active_chart_bars();
                let (bar, price) = self.panel_app.panel_grid.active_container()
                    .map(|w| {
                        if w.crosshair.is_magnet() {
                            let bar_idx = w.viewport().x_to_bar(local_x_main);
                            let (snapped_price, _, snapped_bar_idx) = w.calculate_magnet_snap(
                                &magnet_bars, bar_idx, raw_price, chart_height, price_min, price_max,
                            );
                            let snapped_bar = snapped_bar_idx.map(|idx| idx as f64).unwrap_or(bar);
                            (snapped_bar, snapped_price)
                        } else {
                            (bar, raw_price)
                        }
                    })
                    .unwrap_or((bar, raw_price));

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
                    let cmd = active_bid_tool.and_then(|bid| {
                        let prim_id = self.core.drawings().last_index(bid)?;
                        self.build_create_primitive_command(bid, prim_id)
                    });
                    if let Some(cmd) = cmd {
                        eprintln!("[ChartApp] Recorded CreatePrimitive (main) {}", cmd.prim_id().unwrap_or_default());
                        self.push_undo_command(cmd);
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
                    let cmd = active_bid_tool.and_then(|bid| {
                        let prim_id = self.core.drawings().last_index(bid)?;
                        self.build_create_primitive_command(bid, prim_id)
                    });
                    if let Some(cmd) = cmd {
                        eprintln!("[ChartApp] Recorded CreatePrimitive (sub-pane #{}) {}", pane_idx, cmd.prim_id().unwrap_or_default());
                        self.push_undo_command(cmd);
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
                // Bars from the bubble this click already resolved — NOT via
                // `active_chart_bars()`, whose tag-registry lookup lags and
                // hands back an empty series for the first seconds of a
                // session, making every primitive on screen unclickable.
                let hit_bars_main: Vec<mlc_core::Bar> = active_bid_hit
                    .map(|bid| self.bars_for_bubble(bid))
                    .unwrap_or_default();
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
                            self.blur_agent_focus_for_primitive_select();
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
                let hit_bars_sub: Vec<mlc_core::Bar> = active_bid_hit
                    .map(|bid| self.bars_for_bubble(bid))
                    .unwrap_or_default();
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
                            self.blur_agent_focus_for_primitive_select();
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
        if !self.close_topmost_known_modal_layer() {
            // Unknown layer — fall back to closing everything so the UI never
            // gets stuck in an unrecoverable state.
            self.close_open_modals();
        }
    }

    /// Close the topmost modal layer IF we know what it is; report whether we
    /// did. Escape uses this and the backdrop click uses the wrapper above:
    /// "get me out of here" may close everything on an unknown layer, but a
    /// keystroke that missed a match must fall through to the chart, not wipe
    /// the modal stack.
    pub(super) fn close_topmost_known_modal_layer(&mut self) -> bool {
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
                "color_picker_compare" => {
                    self.panel_app.compare_settings_state.close_color_picker();
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
                "compare_settings" => {
                    self.panel_app.compare_settings_state.close();
                }
                "modal_search" => {
                    self.modal_state.close();
                }
                "rc_modal" => {
                    self.rc_modal.close();
                }
                "dom_modal" => {
                    self.dom_modal.close();
                }
                // Top-level modals — close when user clicks outside them.
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
                // Trading Container modal — close when user clicks outside it.
                "tc_modal" => {
                    self.tc_modal.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: tc_modal closed");
                }
                // Standalone panel-settings surface (the gear popover).
                "pset" => {
                    self.panel_settings_open = None;
                }
                _ => {
                    eprintln!("[ChartApp] close_topmost_modal_layer: unknown layer '{}'", name);
                    return false;
                }
            }
            return true;
        }
        false
    }

    /// Close all open modals (state lives inside panel_app at checkpoint).
    pub(super) fn close_open_modals(&mut self) {
        // Close search/compare/indicator-search overlays (SymbolSearch, CompareSearch, IndicatorSearch).
        self.modal_state.close();
        self.panel_app.primitive_settings_state.close();
        self.panel_app.chart_settings_state.close();
        self.panel_app.indicator_settings_state.close();
        // Close top-level modals that are managed outside of modal_state.
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
        self.trading_header_tb.open_dropdown_id = None;
        self.trading_header_tb.hovered_dropdown_item = None;
        self.trading_header_tb.hovered_top_toolbar_id = None;
        // Dropdown closing abandons any in-progress inline granularity edit
        // (user-granularity-templates-inner arc, кусок 2) — see the
        // `dropdown:__bg__` handler in `panel_click.rs` for the same rule on
        // the explicit background-click path.
        self.granularity_edit = None;
        // Dropdown closing also cancels any pending/active row drag-reorder
        // (owner spec 2026-07-16 items 2+3) — no reorder applies when the
        // dropdown vanishes mid-drag.
        self.granularity_drag_pending = None;
        self.granularity_drag = None;
        self.panel_app.toolbar_state.open_inline_style_dropdown = false;
        self.panel_app.toolbar_state.open_inline_width_dropdown = false;
        self.panel_app.toolbar_state.open_inline_wave_dropdown = false;
        self.panel_app.toolbar_state.open_inline_wave_next_dropdown = false;
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
                        if let Some(bid) = active_bid_ctx {
                            self.delete_primitive_with_undo(bid, prim_id);
                        }
                    }
                    "clone" => {
                        let cloned_id = active_bid_ctx
                            .and_then(|bid| self.core.drawings_mut().clone_primitive(bid, prim_id));
                        if let Some(new_id) = cloned_id {
                            // Apply the shared +20px right, -20px up offset to the
                            // cloned primitive (same delta the paste hotkey uses).
                            if let Some((ts_delta_ms, price_delta)) = self.primitive_offset_delta() {
                                if let Some(bid) = active_bid_ctx {
                                    let mut guard = self.core.drawings_mut();
                                    guard.translate(bid, new_id, ts_delta_ms, price_delta);
                                    guard.select(bid, new_id);
                                }
                            }
                            // Capture the NEW primitive's data for undo.
                            let cmd = active_bid_ctx.and_then(|bid| self.build_create_primitive_command(bid, new_id));
                            if let Some(cmd) = cmd {
                                self.push_undo_command(cmd);
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
                    "bring_to_front" => {
                        // Layer order, like pan/zoom, is not tracked on the
                        // undo stack (no `Command::SetPrimitiveZOrder`
                        // variant exists) — same non-undoable class as the
                        // viewport navigation actions.
                        if let Some(bid) = active_bid_ctx {
                            self.core.drawings_mut().bring_to_front(bid, prim_id);
                        }
                        eprintln!("[ChartApp] Brought primitive #{} to front", prim_id);
                    }
                    "send_to_back" => {
                        if let Some(bid) = active_bid_ctx {
                            self.core.drawings_mut().send_to_back(bid, prim_id);
                        }
                        eprintln!("[ChartApp] Sent primitive #{} to back", prim_id);
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

    /// Blur AGENT_PTY / AGENT_CHAT text focus when a chart primitive is
    /// about to be selected — text-field focus and primitive selection are
    /// mutually exclusive.
    pub(super) fn blur_agent_focus_for_primitive_select(&mut self) {
        let pty_wid = WidgetId::from(crate::text_input::AGENT_PTY);
        let chat_wid = WidgetId::from(crate::text_input::AGENT_CHAT);
        let should_blur = {
            let coord = self.input_coordinator.borrow();
            let tf = coord.text_fields();
            tf.is_focused(&pty_wid) || tf.is_focused(&chat_wid)
        };
        if should_blur {
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
        }
    }

    /// True while a text field is focused or a blocking modal is open —
    /// guards the Ctrl+C/Ctrl+V/Ctrl+X primitive clipboard hotkeys and the
    /// Backspace-deletes-primitive hotkey so they never fire while the user
    /// is typing or a settings modal is in front. `pub(super)` so
    /// `input::mod::on_key_press` can gate the Backspace arm the same way.
    ///
    /// OrderEntry/RiskCalculator numeric fields are registered
    /// `TextFieldStore` fields like everything else (TextFieldStore
    /// migration, 2026-07-25), so `text_fields().focused()` alone already
    /// covers them — no separate per-panel editing check needed.
    pub(super) fn primitive_hotkeys_blocked(&self) -> bool {
        self.input_coordinator.borrow().text_fields().focused().is_some()
            || self.any_blocking_modal_open()
    }

    /// Compute the shared +20px right / -20px up screen-space offset (in
    /// bar-timestamp-ms / price data-space units) used by both the
    /// right-click "clone" action and the Ctrl+V paste hotkey. `None` when
    /// there is no active container to size the offset against.
    fn primitive_offset_delta(&self) -> Option<(i64, f64)> {
        let bars: Vec<mlc_core::Bar> = self.active_chart_bars();
        let w = self.panel_app.panel_grid.active_container()?;
        let bar_spacing = w.viewport().bar_spacing;
        let chart_height = w.viewport().chart_height;
        let price_range = w.price_scale().price_max - w.price_scale().price_min;
        // 20px right = 20 / bar_spacing bars → convert to ms
        let bars_delta = 20.0 / bar_spacing.max(0.001);
        let bar_interval_ms = mlc_core::bar_interval_seconds(&bars[..]) * 1000;
        let ts_delta_ms = (bars_delta * bar_interval_ms as f64).round() as i64;
        // 20px up = 20 / chart_height * price_range (up = positive price)
        let price_delta = if chart_height > 0.0 && price_range > 0.0 {
            20.0 / chart_height * price_range
        } else {
            0.0
        };
        Some((ts_delta_ms, price_delta))
    }

    /// Build a `Command::CreatePrimitive` for `prim_id` in `bid` — the
    /// shared shape every primitive-creation call site needs: `points`
    /// (kept separately so `Command::primitive_identity`'s cold-load
    /// matching never needs to parse JSON) plus a full serialized
    /// `PrimitiveSnapshot` (`type_id` + the primitive's own `to_json()`) so
    /// undo/redo restores EVERY field the concrete type carries, not just
    /// the ones on `PrimitiveData` — see `elliott::count`'s own module doc
    /// for why that distinction matters for an Elliott wave's own
    /// degree/link/follows. `cascaded` is always empty here — only a
    /// cascading delete's own inverse ever populates it (see
    /// `delete_primitive_with_undo`).
    pub(crate) fn build_create_primitive_command(&self, bid: mlc_core::BubbleId, prim_id: u64) -> Option<mlc_core::Command> {
        let guard = self.core.drawings();
        let points = guard.get_points(bid, prim_id)?;
        let snapshot = guard.get_snapshot(bid, prim_id)?;
        Some(mlc_core::Command::CreatePrimitive { prim_id, points, snapshot, cascaded: Vec::new() })
    }

    /// Delete a primitive by stable id, recording undo state first — shared
    /// by the right-click "delete" action, the Delete hotkey, and the
    /// toolbar "Delete selected" dropdown item.
    ///
    /// `DrawingManager::delete`'s own cascade (Elliott subdivision
    /// children nested under `prim_id`) is captured too
    /// (`delete_with_snapshot`), so undo restores the whole subtree in one
    /// step instead of silently losing every child a single-primitive
    /// snapshot could never have named.
    pub(super) fn delete_primitive_with_undo(&mut self, bid: mlc_core::BubbleId, prim_id: u64) {
        let deleted = self.core.drawings_mut().delete_with_snapshot(bid, prim_id);
        match deleted {
            Some(d) => {
                let cascaded_count = d.cascaded.len();
                self.push_undo_command(mlc_core::Command::DeletePrimitive {
                    prim_id,
                    points: d.points,
                    snapshot: d.snapshot,
                    cascaded: d.cascaded,
                });
                eprintln!("[ChartApp] Recorded DeletePrimitive {} (+{} cascaded)", prim_id, cascaded_count);
                eprintln!("[ChartApp] Deleted primitive #{}", prim_id);
            }
            None => {
                eprintln!("[ChartApp] Deleted primitive #{} (no undo snapshot)", prim_id);
            }
        }
        let removed_alerts = self.core.alerts_mut().clear_for_drawing(prim_id);
        for snapshot in removed_alerts {
            self.push_undo_command(mlc_core::Command::DeleteAlert { snapshot });
        }
        self.sync_drawing_back_to_group();
        self.autosave_snapshot();
    }

    /// Delete every primitive in `bid` as one undoable group
    /// (`Command::DeleteAllPrimitives` / its inverse `RestoreAllPrimitives`)
    /// so a single Ctrl+Z restores the whole set — used by the toolbar
    /// "Delete all" dropdown item.
    pub(super) fn delete_all_primitives_with_undo(&mut self, bid: mlc_core::BubbleId) {
        let (ids, primitives): (Vec<u64>, Vec<mlc_core::drawing::PrimitiveSnapshot>) = {
            let guard = self.core.drawings();
            guard.primitive_list(bid).into_iter()
                .filter_map(|item| guard.get_snapshot(bid, item.id).map(|snap| (item.id, snap)))
                .unzip()
        };
        if primitives.is_empty() {
            return;
        }
        self.push_undo_command(mlc_core::Command::DeleteAllPrimitives { primitives: primitives.clone() });
        eprintln!("[ChartApp] Recorded DeleteAllPrimitives ({} primitives)", primitives.len());
        self.core.drawings_mut().delete_all(bid);
        for id in &ids {
            let removed_alerts = self.core.alerts_mut().clear_for_drawing(*id);
            for snapshot in removed_alerts {
                self.push_undo_command(mlc_core::Command::DeleteAlert { snapshot });
            }
        }
        eprintln!("[ChartApp] Deleted all primitives (bid={:?})", bid);
        self.sync_drawing_back_to_group();
        self.autosave_snapshot();
    }

    /// Ctrl+C fallback: copy the selected primitive in the active bubble to
    /// the in-RAM clipboard. Silent no-op when nothing is selected, a text
    /// field is focused, or a blocking modal is open.
    pub(super) fn copy_selected_primitive_to_clipboard(&mut self) {
        if self.primitive_hotkeys_blocked() {
            return;
        }
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else { return };
        let Some(prim_id) = self.core.drawings().selected(bid) else { return };
        let captured = {
            let guard = self.core.drawings();
            let points = guard.get_points(bid, prim_id);
            let data = guard.get_data(bid, prim_id);
            let snapshot = guard.get_snapshot(bid, prim_id);
            match (points, data, snapshot) {
                (Some(points), Some(data), Some(snapshot)) => Some((points, data, snapshot)),
                _ => None,
            }
        };
        if let Some((points, data, snapshot)) = captured {
            self.primitive_clipboard = Some(crate::PrimitiveClipboard { snapshot, points, data, paste_count: 0 });
            eprintln!("[ChartApp] Copied primitive #{} to clipboard", prim_id);
        }
    }

    /// Ctrl+V fallback: paste the in-RAM primitive clipboard into the
    /// active bubble, offset by the shared +20px/-20px delta (stacked per
    /// repeated paste via `paste_count`). Silent no-op when the clipboard
    /// is empty, a text field is focused, or a blocking modal is open.
    pub(super) fn paste_primitive_from_clipboard(&mut self) {
        if self.primitive_hotkeys_blocked() {
            return;
        }
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else { return };
        let Some((ts_delta_ms, price_delta)) = self.primitive_offset_delta() else { return };

        let Some((snapshot, base_points, mut data, multiplier)) = self.primitive_clipboard.as_mut().map(|c| {
            c.paste_count += 1;
            (c.snapshot.clone(), c.points.clone(), c.data.clone(), c.paste_count as f64)
        }) else { return };

        let shifted_points: Vec<(i64, f64)> = base_points.iter()
            .map(|&(ts, price)| (
                ts + (ts_delta_ms as f64 * multiplier).round() as i64,
                price + price_delta * multiplier,
            ))
            .collect();

        let new_id = mlc_core::drawing::alloc_primitive_id();
        data.id = new_id;
        data.origin_id = None;
        data.pane_id = self.core.drawings().current_pane(bid);
        data.window_id = None;
        data.symbol = self.core.bubbles().symbol(bid).map(String::from).unwrap_or_default();
        data.exchange = self.core.bubbles().exchange(bid).map(String::from).unwrap_or_default();
        data.account_type = self.core.bubbles().account_type(bid).map(String::from).unwrap_or_default();

        let insert_pos = self.core.drawings().count(bid);
        if self.core.drawings_mut().insert_pasted_at(bid, insert_pos, &snapshot.type_id, &snapshot.json, &shifted_points, &data).is_none() {
            return;
        }
        self.core.drawings_mut().select(bid, new_id);
        if let Some(cmd) = self.build_create_primitive_command(bid, new_id) {
            self.push_undo_command(cmd);
        }
        eprintln!("[ChartApp] Pasted primitive -> #{}", new_id);
        self.sync_drawing_back_to_group();
        self.autosave_snapshot();
    }

    /// Resolve the pane currently under the mouse cursor (`self.last_mouse_pos`)
    /// to its owning bubble plus the cursor's `(ts_ms, price)` in THAT pane's
    /// own coordinate space — used by the Ctrl+V cursor-paste path.
    ///
    /// Returns `None` when the cursor is not over a chart canvas (over a
    /// scale, separator, toolbar, or off-window), or the hovered leaf has no
    /// window/layout yet. Reuses the exact screen→data conversion the
    /// drawing tools use to place points
    /// (`ChartApp::screen_to_data_coords_for_window`), resolved against the
    /// HOVERED leaf's window rather than always the active container — the
    /// pane under the cursor is not necessarily the active leaf in split mode.
    fn cursor_pane_position(&self) -> Option<(mlc_core::BubbleId, i64, f64)> {
        let (x, y) = self.last_mouse_pos;
        let leaf_id = match self.panel_app.panel_grid.resolve_input(
            x, y, self.content_rect.x, self.content_rect.y,
        ) {
            crate::panel_grid::ChartInputTarget::Chart { leaf_id } => leaf_id,
            _ => return None,
        };
        let window = self.panel_app.panel_grid.container_for_leaf(leaf_id)?;
        let bid = window.current_tag_id;

        let extended = self.frame_layouts.get(&leaf_id)
            .cloned()
            .or_else(|| self.get_leaf_absolute_rect(leaf_id)
                .and_then(|r| self.build_extended_layout_for_leaf(leaf_id, &r)))?;

        let chart_x = extended.main_chart.chart.x;
        let chart_y = extended.main_chart.chart.y;
        let chart_h = extended.main_chart.chart.height;
        let pane_id = self.core.drawings().current_pane(bid);

        let (bar, price) = self.screen_to_data_coords_for_window(
            x, y, pane_id, &extended, chart_x, chart_y, chart_h, window,
        );

        let bars = self.bars_for_leaf(leaf_id);
        let ts_ms = mlc_core::bar_f64_to_timestamp_ms(&bars[..], bar);
        Some((bid, ts_ms, price))
    }

    /// Ctrl+V primary path (owner spec 2026-07-25): paste the in-RAM
    /// primitive clipboard positioned at the mouse cursor — translates the
    /// whole copied shape rigidly so its bounding-box center lands at the
    /// cursor's `(ts, price)` in the pane under the cursor (target bubble =
    /// that pane's [`mlc_core::BubbleId`], restamped like the offset path).
    /// No `paste_count` stacking — this places exactly at the cursor every
    /// time.
    ///
    /// Falls back to [`Self::paste_primitive_from_clipboard`] (the
    /// +20px/-20px offset-stacking behavior) when the cursor is not over a
    /// chart pane or its position cannot be resolved. Silent no-op when the
    /// clipboard is empty, a text field is focused, or a blocking modal is
    /// open (same guard as the offset path).
    pub(super) fn paste_primitive_at_cursor(&mut self) {
        if self.primitive_hotkeys_blocked() {
            return;
        }
        let Some((target_bid, cursor_ts, cursor_price)) = self.cursor_pane_position() else {
            self.paste_primitive_from_clipboard();
            return;
        };

        let Some((snapshot, base_points, mut data)) = self.primitive_clipboard.as_ref().map(|c| {
            (c.snapshot.clone(), c.points.clone(), c.data.clone())
        }) else { return };

        if base_points.is_empty() {
            return;
        }

        // Bounding-box center of the copied shape — the rigid-translation anchor.
        let (min_ts, max_ts) = base_points.iter().fold(
            (i64::MAX, i64::MIN),
            |(lo, hi), &(ts, _)| (lo.min(ts), hi.max(ts)),
        );
        let (min_price, max_price) = base_points.iter().fold(
            (f64::INFINITY, f64::NEG_INFINITY),
            |(lo, hi), &(_, p)| (lo.min(p), hi.max(p)),
        );
        let center_ts = (min_ts + max_ts) / 2;
        let center_price = (min_price + max_price) / 2.0;
        let ts_delta = cursor_ts - center_ts;
        let price_delta = cursor_price - center_price;

        let shifted_points: Vec<(i64, f64)> = base_points.iter()
            .map(|&(ts, price)| (ts + ts_delta, price + price_delta))
            .collect();

        let new_id = mlc_core::drawing::alloc_primitive_id();
        data.id = new_id;
        data.origin_id = None;
        data.pane_id = self.core.drawings().current_pane(target_bid);
        data.window_id = None;
        data.symbol = self.core.bubbles().symbol(target_bid).map(String::from).unwrap_or_default();
        data.exchange = self.core.bubbles().exchange(target_bid).map(String::from).unwrap_or_default();
        data.account_type = self.core.bubbles().account_type(target_bid).map(String::from).unwrap_or_default();

        let insert_pos = self.core.drawings().count(target_bid);
        if self.core.drawings_mut().insert_pasted_at(target_bid, insert_pos, &snapshot.type_id, &snapshot.json, &shifted_points, &data).is_none() {
            return;
        }
        self.core.drawings_mut().select(target_bid, new_id);
        if let Some(cmd) = self.build_create_primitive_command(target_bid, new_id) {
            self.push_undo_command(cmd);
        }
        eprintln!("[ChartApp] Pasted primitive at cursor -> #{}", new_id);
        self.sync_drawing_back_to_group();
        self.autosave_snapshot();
    }

    /// Ctrl+X fallback (owner spec 2026-07-25): cut the selected primitive —
    /// copy it to the in-RAM clipboard (same guards as
    /// [`Self::copy_selected_primitive_to_clipboard`]: silent no-op when
    /// nothing is selected, a text field is focused, or a blocking modal is
    /// open) then delete it through the same undo-correct path as the
    /// Delete hotkey. A single undo entry (`DeletePrimitive`, pushed by
    /// `delete_primitive_with_undo`) restores it.
    pub(super) fn cut_selected_primitive(&mut self) {
        if self.primitive_hotkeys_blocked() {
            return;
        }
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else { return };
        let Some(prim_id) = self.core.drawings().selected(bid) else { return };
        self.copy_selected_primitive_to_clipboard();
        self.delete_primitive_with_undo(bid, prim_id);
    }
}
