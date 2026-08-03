//! Compare/Indicator overlay click and action handlers.
//! Handles overlay popups, settings clicks, and indicator instance lifecycle.

use crate::ChartApp;
use mlc_app_layout::ui::modal_settings::DualSliderHandle;

impl ChartApp {
    /// After a CVD indicator instance was just added to `bid` (created on
    /// `chart_id`'s window), install the client-side CVD companion fold
    /// (`BackendCore::subscribe_cvd_fold`) for the instrument — unless the
    /// window is a footprint chart, which already carries real per-bar
    /// buy/sell in `bubble.footprint_bars` (Phase 1's delta builder prefers
    /// that source; a second CVD fold would be redundant).
    ///
    /// No-op if `chart_id` is `None`, the chart has no resolvable
    /// instrument, or the connector for its exchange is disabled.
    pub(crate) fn subscribe_cvd_line_for_indicator_add(
        &mut self,
        chart_id: Option<mlc_core::state::ChartId>,
    ) {
        let Some(cid) = chart_id else { return };
        if self.chart_type_for_chart(cid) == Some("footprint") {
            return;
        }
        let Some(symbol) = self.symbol_for_chart(cid) else { return };
        if symbol.is_empty() {
            return;
        }
        let Some(at) = self.account_type_for_chart(cid) else { return };
        let exchange = self.exchange_for_chart(cid).unwrap_or_default();
        let exchange_id = digdigdig3::ExchangeId::from_str(&exchange)
            .unwrap_or(self.active_exchange);
        let eid_str = exchange_id.as_str();
        let connector_enabled = self.sidebar_state.connector_enabled
            .get(eid_str).copied().unwrap_or(true);
        if !connector_enabled {
            return;
        }
        let at_dig3 = mlc_backend_data_bridge::account_type_to_dig3(at);
        // Unified-data-plane Stage Э5: the client-side CVD companion fold is
        // the ONLY CVD data path now (station-derived `request_cvd_line`
        // removed) — `bid` is resolved from `cid`'s current tag (same
        // lookup pattern the `unsubscribe` counterpart below uses via `bid`
        // directly). `subscribe_cvd_fold` internally ensures the trade layer
        // is network-fed (`subscribe_trades` + `ensure_trade_backfill`) and
        // reseeds synchronously from whatever the layer already holds — a
        // fresh bubble with an empty trade layer gets its backfill kicked
        // off right here, and the reseed output (if any) is applied via the
        // returned event.
        if let Some(bid) = self.bubble_for_chart(cid) {
            // TF-scaled trade-history ceiling (owner correction 2026-07-12):
            // the CVD companion warms `~180 × TF` of tape, clamped 1h..24h —
            // scroll demand deepens past it. Replaces the legacy
            // `derived_trade_target` point count.
            let chart_tf_ms = self.timeframe_for_chart(cid)
                .filter(|tf| !tf.is_tick())
                .map(|tf| tf.interval_ms);
            if let Some(ev) = self.core.subscribe_cvd_fold(bid, exchange_id, &symbol, at_dig3, chart_tf_ms) {
                let now_ms = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                self.react_to_bubble_event(&ev, now_ms);
            }
        }
    }

    /// After the last CVD indicator instance for an instrument was removed
    /// (`bid` no longer has any `type_id == "cvd"` instance), drop the CVD
    /// companion fold runtime and clear the cached samples.
    ///
    /// Reads `bid`'s own resolvable instrument directly (does not depend on
    /// which window triggered the delete) so peer-instance cleanup — see
    /// `delete_indicator_instance`'s `peer_instances` sweep — always lands
    /// on the correct bubble's data plane.
    pub(super) fn unsubscribe_cvd_line_if_last(&mut self, bid: mlc_core::ids::BubbleId) {
        let still_has_cvd = self.core.indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .any(|inst| inst.type_id == "cvd");
        if still_has_cvd {
            return;
        }
        // Unified-data-plane Stage Э5: no station-derived unsubscribe left —
        // `active_cvd_fold` is the only CVD runtime, torn down alongside the
        // cached samples in one bubble mutation.
        self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
            bubble.clear_cvd();
            bubble.active_cvd_fold = None;
        });
    }

    /// Handle clicks on widgets registered with the "ind_overlay:" prefix.
    pub(super) fn handle_ind_overlay_click(&mut self, rest: &str, _x: f64, _y: f64) {
        self.handle_indicator_overlay_click(rest);
    }

    /// Handle clicks on widgets registered with the "cmp_overlay:" prefix.
    ///
    /// Widget IDs (single-window mode):
    /// - `cmp_overlay:vis:{idx}` — toggle compare series visibility at index idx
    /// - `cmp_overlay:delete:{idx}` — remove compare series at index idx
    /// - `cmp_overlay:settings:{idx}` — settings for compare series (not yet implemented)
    /// - `cmp_overlay:alert:{idx}` — alert for compare series (not applicable, no-op)
    ///
    /// Widget IDs (split mode):
    /// - `cmp_overlay:leaf{leaf_id}:{action}` — same actions routed to the active leaf window
    pub(super) fn handle_cmp_overlay_click(&mut self, rest: &str) {
        // In split mode the widget ID has the form `leaf{n}:{action}`.
        if rest.starts_with("leaf") {
            if let Some(colon_pos) = rest.find(':') {
                let leaf_id_str = &rest["leaf".len()..colon_pos];
                let action = &rest[colon_pos + 1..];
                if let Ok(leaf_raw) = leaf_id_str.parse::<u64>() {
                    let leaf_id = mlc_core::LeafId(leaf_raw);
                    // In split mode, the compare overlay is per-window.
                    // Find the window for this leaf and apply the action.
                    self.handle_leaf_cmp_overlay_action(leaf_id, action);
                    return;
                }
            }
            eprintln!("[ChartApp] cmp_overlay leaf prefix malformed: {}", rest);
            return;
        }

        // Single-window mode: act on the active window's compare overlay.
        self.handle_cmp_overlay_action(rest);
    }

    /// Apply a compare overlay action in single-window (non-split) mode.
    pub(super) fn handle_cmp_overlay_action(&mut self, action: &str) {
        match action {
            _ if action.starts_with("vis:") => {
                if let Ok(idx) = action["vis:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        let new_vis = window.compare_overlay.toggle_visibility(idx);
                        eprintln!("[ChartApp] cmp_overlay vis toggled: idx={} -> visible={}", idx, new_vis);
                    }
                }
            }
            _ if action.starts_with("delete:") => {
                if let Ok(idx) = action["delete:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        if let Some(symbol) = window.compare_overlay.remove_series(idx) {
                            eprintln!("[ChartApp] cmp_overlay deleted: idx={} symbol={}", idx, symbol);
                        }
                    }
                }
            }
            _ if action.starts_with("settings:") => {
                if let Ok(idx) = action["settings:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_container() {
                        if let Some(series) = window.compare_overlay.series.get(idx) {
                            self.panel_app.compare_settings_state.open(
                                idx,
                                &series.symbol.clone(),
                                &series.name.clone(),
                                &series.color.clone(),
                                series.line_width,
                                &series.line_style.clone(),
                                &series.chart_type.clone(),
                                series.visible,
                                series.bars.len(),
                                series.base_price,
                                series.timeframe_visibility.clone(),
                            );
                            self.panel_app.compare_settings_state.pin_initial_position(
                                self.content_rect.width,
                                self.content_rect.height,
                            );
                            eprintln!("[ChartApp] cmp_overlay settings opened: idx={}", idx);
                        }
                    }
                }
            }
            _ if action.starts_with("alert:") => {
                if let Ok(idx) = action["alert:".len()..].parse::<usize>() {
                    let price = self.panel_app.panel_grid.active_container()
                        .and_then(|_w| None::<&mlc_core::Bar>)
                        .map(|b| b.close)
                        .unwrap_or(0.0);
                    let symbol = self.panel_app.panel_grid.active_container_id()
                        .and_then(|cid| self.symbol_for_chart(cid))
                        .map(String::from)
                        .unwrap_or_else(|| "BTCUSD".to_string());
                    let source_name = self.panel_app.panel_grid.active_container()
                        .and_then(|w| w.compare_overlay.series.get(idx))
                        .map(|s| format!("Compare: {}", s.symbol))
                        .unwrap_or_else(|| format!("Compare series {}", idx));
                    let source = mlc_core::alert::AlertSource::Price { symbol: symbol.clone() };
                    self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                    self.panel_app.alert_settings_state.source_name = source_name;
                    self.panel_app.alert_settings_state.pin_initial_position(
                        self.content_rect.width, self.content_rect.height,
                    );
                    eprintln!("[ChartApp] cmp_overlay alert opened: idx={}", idx);
                }
            }
            _ if action.starts_with("row:") => {
                // Row hit zone — no action needed.
            }
            _ => {
                eprintln!("[ChartApp] cmp_overlay unhandled action: {}", action);
            }
        }
    }

    /// Apply a compare overlay action for a specific leaf (split mode).
    pub(super) fn handle_leaf_cmp_overlay_action(&mut self, leaf_id: mlc_core::LeafId, action: &str) {
        // Route to the same per-window logic. In split mode compare overlays are
        // per-window; we currently act on the active window's compare overlay.
        match action {
            _ if action.starts_with("vis:") => {
                if let Ok(idx) = action["vis:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        let new_vis = window.compare_overlay.toggle_visibility(idx);
                        eprintln!("[ChartApp] cmp_overlay leaf {} vis toggled: idx={} -> visible={}", leaf_id.0, idx, new_vis);
                    }
                }
            }
            _ if action.starts_with("delete:") => {
                if let Ok(idx) = action["delete:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        if let Some(symbol) = window.compare_overlay.remove_series(idx) {
                            eprintln!("[ChartApp] cmp_overlay leaf {} deleted: idx={} symbol={}", leaf_id.0, idx, symbol);
                        }
                    }
                }
            }
            _ if action.starts_with("settings:") => {
                if let Ok(idx) = action["settings:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_container() {
                        if let Some(series) = window.compare_overlay.series.get(idx) {
                            self.panel_app.compare_settings_state.open(
                                idx,
                                &series.symbol.clone(),
                                &series.name.clone(),
                                &series.color.clone(),
                                series.line_width,
                                &series.line_style.clone(),
                                &series.chart_type.clone(),
                                series.visible,
                                series.bars.len(),
                                series.base_price,
                                series.timeframe_visibility.clone(),
                            );
                            self.panel_app.compare_settings_state.pin_initial_position(
                                self.content_rect.width,
                                self.content_rect.height,
                            );
                            eprintln!("[ChartApp] cmp_overlay leaf {} settings opened: idx={}", leaf_id.0, idx);
                        }
                    }
                }
            }
            _ if action.starts_with("alert:") => {
                if let Ok(idx) = action["alert:".len()..].parse::<usize>() {
                    let price = self.panel_app.panel_grid.active_container()
                        .and_then(|_w| None::<&mlc_core::Bar>)
                        .map(|b| b.close)
                        .unwrap_or(0.0);
                    let symbol = self.panel_app.panel_grid.active_container_id()
                        .and_then(|cid| self.symbol_for_chart(cid))
                        .map(String::from)
                        .unwrap_or_else(|| "BTCUSD".to_string());
                    let source_name = self.panel_app.panel_grid.active_container()
                        .and_then(|w| w.compare_overlay.series.get(idx))
                        .map(|s| format!("Compare: {}", s.symbol))
                        .unwrap_or_else(|| format!("Compare series {}", idx));
                    let source = mlc_core::alert::AlertSource::Price { symbol: symbol.clone() };
                    self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                    self.panel_app.alert_settings_state.source_name = source_name;
                    self.panel_app.alert_settings_state.pin_initial_position(
                        self.content_rect.width, self.content_rect.height,
                    );
                    eprintln!("[ChartApp] cmp_overlay leaf {} alert opened: idx={}", leaf_id.0, idx);
                }
            }
            _ if action.starts_with("row:") => {
                // Row hit zone — no action needed.
            }
            _ => {
                eprintln!("[ChartApp] cmp_overlay leaf {} unhandled action: {}", leaf_id.0, action);
            }
        }
    }

    /// Handle clicks on widgets registered with the `"cmp_settings:"` prefix.
    ///
    /// Widget IDs:
    /// - `cmp_settings:close` — close the modal
    /// - `cmp_settings:cancel` — close the modal (no changes applied)
    /// - `cmp_settings:ok` — close the modal (changes already live via immediate updates)
    /// - `cmp_settings:tab:{id}` — switch active tab
    /// - `cmp_settings:modal_bg` — absorb background click (no-op)
    /// - `cmp_settings:color_swatch` — open color picker for line color
    /// - `cmp_settings:line_width_slider` — start line-width slider drag
    /// - `cmp_settings:toggle_visible` — toggle series visibility
    /// - `cmp_settings:line_style_dd` — line style dropdown (no-op placeholder)
    /// - `cmp_settings:chart_type_dd` — chart type dropdown (toggle open)
    /// - `cmp_settings:chart_type_option:{id}` — select chart type
    pub(super) fn handle_cmp_settings_click(&mut self, rest: &str, x: f64, y: f64) {
        // Close template dropdown on any non-template click
        if !rest.starts_with("template_") {
            self.panel_app.compare_settings_state.template_dropdown_open = false;
        }
        match rest {
            "close" | "ok" => {
                self.panel_app.compare_settings_state.close();
                self.autosave_snapshot();
                eprintln!("[ChartApp] cmp_settings closed ({})", rest);
            }
            "cancel" => {
                // Revert all changes to original values before closing.
                let idx = self.panel_app.compare_settings_state.series_index;
                let orig_color = self.panel_app.compare_settings_state.original_color.clone();
                let orig_width = self.panel_app.compare_settings_state.original_line_width;
                let orig_style = self.panel_app.compare_settings_state.original_line_style.clone();
                let orig_chart_type = self.panel_app.compare_settings_state.original_chart_type.clone();
                let orig_tf = self.panel_app.compare_settings_state.original_timeframe_visibility.clone();
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.compare_overlay.set_series_color_by_index(idx, &orig_color);
                    window.compare_overlay.set_series_line_width_by_index(idx, orig_width);
                    window.compare_overlay.set_series_line_style_by_index(idx, &orig_style);
                    window.compare_overlay.set_series_chart_type_by_index(idx, &orig_chart_type);
                    if let Some(tf) = orig_tf.clone() {
                        window.compare_overlay.set_series_timeframe_visibility(idx, tf);
                    }
                }
                self.panel_app.compare_settings_state.cached_color = orig_color;
                self.panel_app.compare_settings_state.cached_line_width = orig_width;
                self.panel_app.compare_settings_state.cached_line_style = orig_style;
                self.panel_app.compare_settings_state.cached_chart_type = orig_chart_type;
                self.panel_app.compare_settings_state.cached_timeframe_visibility = orig_tf;
                self.panel_app.compare_settings_state.close();
                self.autosave_snapshot();
                eprintln!("[ChartApp] cmp_settings cancelled and reverted");
            }
            "modal_bg" => {
                // Click inside modal body — close template dropdown if open
                self.panel_app.compare_settings_state.template_dropdown_open = false;
            }
            id if id.starts_with("tab:") => {
                self.panel_app.compare_settings_state.template_dropdown_open = false;
                use mlc_app_layout::ui::modal_settings::CompareSettingsTab;
                if let Some(tab) = CompareSettingsTab::from_id(&id["tab:".len()..]) {
                    self.panel_app.compare_settings_state.set_tab(tab);
                    eprintln!("[ChartApp] cmp_settings tab: {:?}", tab);
                }
            }
            "color_swatch" => {
                // Open the color picker anchored to the swatch position.
                // The color picker state lives inside compare_settings_state.color_picker
                // and is rendered as part of the compare settings modal.
                let swatch_opt = self.frame_result
                    .as_ref()
                    .and_then(|r| r.compare_settings.as_ref())
                    .and_then(|cs| cs.color_swatch_rect);
                if let Some(swatch_rect) = swatch_opt {
                    let current_color = self.panel_app.compare_settings_state.cached_color.clone();
                    self.panel_app.compare_settings_state.open_color_picker(
                        "line_color",
                        swatch_rect.x, swatch_rect.y,
                        swatch_rect.width, swatch_rect.height,
                        self.content_rect.width,
                        self.content_rect.height,
                        Some(&current_color),
                    );
                    eprintln!("[ChartApp] cmp_settings color picker opened");
                }
            }
            "line_width_slider" => {
                // Start slider drag from click position.
                let track_opt = self.frame_result
                    .as_ref()
                    .and_then(|r| r.compare_settings.as_ref())
                    .and_then(|cs| cs.line_width_slider.as_ref())
                    .cloned();
                if let Some(track) = track_opt {
                    self.panel_app.compare_settings_state.start_slider_drag(
                        &track.field_id,
                        track.track_x,
                        track.track_width,
                        track.min_val,
                        track.max_val,
                    );
                    eprintln!("[ChartApp] cmp_settings line_width slider drag started");
                }
            }
            "toggle_visible" => {
                let idx = self.panel_app.compare_settings_state.series_index;
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    let new_vis = window.compare_overlay.toggle_visibility(idx);
                    self.panel_app.compare_settings_state.cached_visible = new_vis;
                    eprintln!("[ChartApp] cmp_settings toggle_visible: idx={} -> visible={}", idx, new_vis);
                    self.autosave_snapshot();
                }
            }
            "line_style_dd" => {
                // Toggle the line style dropdown open/closed.
                self.panel_app.compare_settings_state.line_style_dropdown_open =
                    !self.panel_app.compare_settings_state.line_style_dropdown_open;
                eprintln!("[ChartApp] cmp_settings line_style_dd toggled: {}", self.panel_app.compare_settings_state.line_style_dropdown_open);
            }
            id if id.starts_with("line_style_option:") => {
                let style = &id["line_style_option:".len()..];
                let idx = self.panel_app.compare_settings_state.series_index;
                self.panel_app.compare_settings_state.cached_line_style = style.to_string();
                self.panel_app.compare_settings_state.line_style_dropdown_open = false;
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.compare_overlay.set_series_line_style_by_index(idx, style);
                }
                self.autosave_snapshot();
                eprintln!("[ChartApp] cmp_settings line_style set to: {}", style);
            }
            "chart_type_dd" => {
                // Toggle the chart-type dropdown open/closed.
                self.panel_app.compare_settings_state.chart_type_dropdown_open =
                    !self.panel_app.compare_settings_state.chart_type_dropdown_open;
                eprintln!("[ChartApp] cmp_settings chart_type_dd toggled: {}", self.panel_app.compare_settings_state.chart_type_dropdown_open);
            }
            id if id.starts_with("chart_type_option:") => {
                let chart_type = &id["chart_type_option:".len()..];
                let idx = self.panel_app.compare_settings_state.series_index;
                self.panel_app.compare_settings_state.cached_chart_type = chart_type.to_string();
                self.panel_app.compare_settings_state.chart_type_dropdown_open = false;
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.compare_overlay.set_series_chart_type_by_index(idx, chart_type);
                }
                self.autosave_snapshot();
                eprintln!("[ChartApp] cmp_settings chart_type set to: {}", chart_type);
            }
            // ---- Visibility tab: tf_*_toggle ----
            id if id.starts_with("item:tf_") && id.ends_with("_toggle") => {
                let inner = &id["item:tf_".len()..id.len() - "_toggle".len()];
                if let Ok(tf_idx) = inner.parse::<usize>() {
                    self.apply_cmp_tf_toggle(tf_idx);
                }
            }
            // ---- Visibility tab: tf_*_slider (click → start drag) ----
            id if id.starts_with("item:tf_") && id.ends_with("_slider") => {
                let inner = &id["item:tf_".len()..id.len() - "_slider".len()];
                if let Ok(tf_idx) = inner.parse::<usize>() {
                    let field_id = format!("tf_{}_slider", tf_idx);
                    if let Some(track) = self.frame_result
                        .as_ref()
                        .and_then(|r| r.compare_settings.as_ref())
                        .and_then(|cs| cs.tf_slider_tracks.iter().find(|t| t.field_id == field_id))
                        .cloned()
                    {
                        let tf_config = self.panel_app.compare_settings_state
                            .cached_timeframe_visibility.clone()
                            .unwrap_or_else(mlc_core::drawing::TimeframeVisibilityConfig::all);
                        let (cur_min, cur_max): (u32, u32) = match tf_idx {
                            1 => tf_config.seconds.unwrap_or((1, 59)),
                            2 => tf_config.minutes.unwrap_or((1, 59)),
                            3 => tf_config.hours.unwrap_or((1, 24)),
                            4 => tf_config.days.unwrap_or((1, 366)),
                            5 => tf_config.weeks.unwrap_or((1, 52)),
                            6 => tf_config.months.unwrap_or((1, 12)),
                            _ => return,
                        };
                        let t = ((x - track.track_x) / track.track_width).clamp(0.0, 1.0);
                        let min_pos = (cur_min as f64 - track.min_val) / (track.max_val - track.min_val);
                        let max_pos = (cur_max as f64 - track.min_val) / (track.max_val - track.min_val);
                        let handle = if (t - min_pos).abs() <= (t - max_pos).abs() {
                            DualSliderHandle::Min
                        } else {
                            DualSliderHandle::Max
                        };
                        self.panel_app.compare_settings_state.start_dual_slider_drag(
                            &field_id,
                            track.track_x,
                            track.track_width,
                            track.min_val,
                            track.max_val,
                            handle,
                            x,
                        );
                        eprintln!("[ChartApp] cmp_settings tf_{} slider drag started {:?}", tf_idx, handle);
                    }
                }
            }
            // ---- Visibility tab: tf_*_min / tf_*_max text input (click → start editing) ----
            id if id.starts_with("item:tf_") && (id.ends_with("_min") || id.ends_with("_max")) => {
                let field_id = id["item:".len()..].to_string();
                let tf_idx_result: Option<(usize, bool)> = if let Some(inner) = field_id.strip_prefix("tf_").and_then(|s| s.strip_suffix("_min")) {
                    inner.parse::<usize>().ok().map(|i| (i, true))
                } else if let Some(inner) = field_id.strip_prefix("tf_").and_then(|s| s.strip_suffix("_max")) {
                    inner.parse::<usize>().ok().map(|i| (i, false))
                } else {
                    None
                };
                if let Some((tf_idx, is_min)) = tf_idx_result {
                    let tf_config = self.panel_app.compare_settings_state
                        .cached_timeframe_visibility.clone()
                        .unwrap_or_else(mlc_core::drawing::TimeframeVisibilityConfig::all);
                    let (cur_min, cur_max): (u32, u32) = match tf_idx {
                        1 => tf_config.seconds.unwrap_or((1, 59)),
                        2 => tf_config.minutes.unwrap_or((1, 59)),
                        3 => tf_config.hours.unwrap_or((1, 24)),
                        4 => tf_config.days.unwrap_or((1, 366)),
                        5 => tf_config.weeks.unwrap_or((1, 52)),
                        6 => tf_config.months.unwrap_or((1, 12)),
                        _ => return,
                    };
                    let current_val = if is_min { cur_min } else { cur_max };
                    let text = current_val.to_string();
                    // Wave 2b: start editing via TextFieldStore.
                    let cmp_wid = uzor::WidgetId::from(crate::text_input::COMPARE_SETTINGS_FIELD);
                    self.panel_app.compare_settings_state.active_field_id = Some(field_id);
                    {
                        let mut coord = self.input_coordinator.borrow_mut();
                        coord.text_fields_mut().set_text(&cmp_wid, &text);
                        coord.text_fields_mut().focus(cmp_wid.clone());
                        if let Some(fs) = coord.text_fields_mut().field_state_mut(&cmp_wid) {
                            fs.cursor = text.len();
                            fs.selection_start = Some(0);
                        }
                    }
                    eprintln!("[ChartApp] cmp_settings editing tf_{} {}", tf_idx, if is_min { "min" } else { "max" });
                }
            }
            "template_dropdown" => {
                self.panel_app.compare_settings_state.template_dropdown_open =
                    !self.panel_app.compare_settings_state.template_dropdown_open;
                eprintln!("[ChartApp] cmp_settings template_dropdown toggled");
            }
            "template_save_as" => {
                self.panel_app.compare_settings_state.template_dropdown_open = false;
                self.panel_app.compare_settings_state.save_template_mode = true;
                let prefix = "Мой шаблон ";
                let max_n = self.core.templates().compare_templates()
                    .iter()
                    .filter_map(|t| t.name.strip_prefix(prefix))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("{}{}", prefix, max_n + 1);
                let field_id = uzor::WidgetId::from(crate::text_input::COMPARE_SETTINGS_TEMPLATE_NAME);
                self.input_coordinator.borrow_mut().text_fields_mut().focus(field_id.clone());
                self.input_coordinator.borrow_mut().text_fields_mut().set_text(&field_id, &default_name);
                eprintln!("[ChartApp] cmp_settings template save-as opened");
            }
            "template_default" => {
                self.panel_app.compare_settings_state.template_dropdown_open = false;
                self.panel_app.compare_settings_state.applied_template_id = None;
                let defaults = mlc_core::templates::CompareTemplate::new_with_defaults("__default__");
                self.panel_app.compare_settings_state.cached_color = defaults.color.clone();
                self.panel_app.compare_settings_state.cached_line_width = defaults.line_width;
                self.panel_app.compare_settings_state.cached_line_style = defaults.line_style.clone();
                let idx = self.panel_app.compare_settings_state.series_index;
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.compare_overlay.set_series_color_by_index(idx, &defaults.color);
                    window.compare_overlay.set_series_line_width_by_index(idx, defaults.line_width);
                    window.compare_overlay.set_series_line_style_by_index(idx, &defaults.line_style);
                }
                self.autosave_snapshot();
                self.snapshot_compare_settings_to_user_manager();
                eprintln!("[ChartApp] cmp_settings reset to developer defaults");
            }
            "template_dropdown_menu" => {
                // Absorb click inside dropdown, keep it open
            }
            id if id.starts_with("template_delete:") => {
                let tmpl_id = &id["template_delete:".len()..];
                eprintln!("[ChartApp] cmp_settings deleted compare template: {}", tmpl_id);
                if self.panel_app.compare_settings_state.applied_template_id.as_deref() == Some(tmpl_id) {
                    self.panel_app.compare_settings_state.applied_template_id = None;
                }
                self.template_actions.push(crate::TemplateAction::RemoveCompare { id: tmpl_id.to_string() });
            }
            id if id.starts_with("template_option:") => {
                let tmpl_id = &id["template_option:".len()..];
                let tmpl_opt = self.core.templates().get_compare_template(tmpl_id).cloned();
                if let Some(tmpl) = tmpl_opt {
                    let idx = self.panel_app.compare_settings_state.series_index;
                    self.panel_app.compare_settings_state.applied_template_id = Some(tmpl.id.clone());
                    self.panel_app.compare_settings_state.template_dropdown_open = false;
                    // Apply template styles immediately
                    self.panel_app.compare_settings_state.cached_color = tmpl.color.clone();
                    self.panel_app.compare_settings_state.cached_line_width = tmpl.line_width;
                    self.panel_app.compare_settings_state.cached_line_style = tmpl.line_style.clone();
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.compare_overlay.set_series_color_by_index(idx, &tmpl.color);
                        window.compare_overlay.set_series_line_width_by_index(idx, tmpl.line_width);
                        window.compare_overlay.set_series_line_style_by_index(idx, &tmpl.line_style);
                    }
                    self.autosave_snapshot();
                    self.snapshot_compare_settings_to_user_manager();
                    eprintln!("[ChartApp] cmp_settings template applied: {}", tmpl.name);
                }
            }
            _ => {
                eprintln!("[ChartApp] cmp_settings unhandled: {}", rest);
            }
        }
        let _ = (x, y); // suppress unused warning
    }

    /// Toggle timeframe visibility category for the compare series currently being edited.
    pub(super) fn apply_cmp_tf_toggle(&mut self, tf_idx: usize) {
        use mlc_core::drawing::TimeframeVisibilityConfig;
        let series_idx = self.panel_app.compare_settings_state.series_index;

        let mut tf_config = self.panel_app.compare_settings_state
            .cached_timeframe_visibility.clone()
            .unwrap_or_else(TimeframeVisibilityConfig::all);

        match tf_idx {
            0 => tf_config.ticks = !tf_config.ticks,
            1 => tf_config.seconds = if tf_config.seconds.is_some() { None } else { Some((1, 59)) },
            2 => tf_config.minutes = if tf_config.minutes.is_some() { None } else { Some((1, 59)) },
            3 => tf_config.hours   = if tf_config.hours.is_some()   { None } else { Some((1, 24)) },
            4 => tf_config.days    = if tf_config.days.is_some()    { None } else { Some((1, 366)) },
            5 => tf_config.weeks   = if tf_config.weeks.is_some()   { None } else { Some((1, 52)) },
            6 => tf_config.months  = if tf_config.months.is_some()  { None } else { Some((1, 12)) },
            _ => return,
        }

        self.panel_app.compare_settings_state.cached_timeframe_visibility = Some(tf_config.clone());
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.compare_overlay.set_series_timeframe_visibility(series_idx, tf_config);
        }
        self.autosave_snapshot();
        eprintln!("[ChartApp] cmp_settings tf_{} toggled", tf_idx);
    }

    /// Apply a committed min/max value from the compare settings text input.
    pub(super) fn apply_cmp_tf_text_commit(&mut self, field_id: &str, text: &str) {
        use mlc_core::drawing::TimeframeVisibilityConfig;
        use mlc_app_layout::ui::modal_settings::DualSliderHandle;

        let tf_idx_result: Option<(usize, DualSliderHandle)> = if let Some(inner) = field_id.strip_prefix("tf_").and_then(|s| s.strip_suffix("_min")) {
            inner.parse::<usize>().ok().map(|i| (i, DualSliderHandle::Min))
        } else if let Some(inner) = field_id.strip_prefix("tf_").and_then(|s| s.strip_suffix("_max")) {
            inner.parse::<usize>().ok().map(|i| (i, DualSliderHandle::Max))
        } else {
            None
        };

        let Some((tf_idx, handle)) = tf_idx_result else { return; };
        let Ok(new_val) = text.trim().parse::<u32>() else { return; };

        let series_idx = self.panel_app.compare_settings_state.series_index;
        let mut tf_config = self.panel_app.compare_settings_state
            .cached_timeframe_visibility.clone()
            .unwrap_or_else(TimeframeVisibilityConfig::all);

        match tf_idx {
            1 => {
                let (cmin, cmax) = tf_config.seconds.unwrap_or((1, 59));
                tf_config.seconds = Some(match handle {
                    DualSliderHandle::Min => (new_val.clamp(1, cmax), cmax),
                    DualSliderHandle::Max => (cmin, new_val.clamp(cmin, 59)),
                });
            }
            2 => {
                let (cmin, cmax) = tf_config.minutes.unwrap_or((1, 59));
                tf_config.minutes = Some(match handle {
                    DualSliderHandle::Min => (new_val.clamp(1, cmax), cmax),
                    DualSliderHandle::Max => (cmin, new_val.clamp(cmin, 59)),
                });
            }
            3 => {
                let (cmin, cmax) = tf_config.hours.unwrap_or((1, 24));
                tf_config.hours = Some(match handle {
                    DualSliderHandle::Min => (new_val.clamp(1, cmax), cmax),
                    DualSliderHandle::Max => (cmin, new_val.clamp(cmin, 24)),
                });
            }
            4 => {
                let (cmin, cmax) = tf_config.days.unwrap_or((1, 366));
                tf_config.days = Some(match handle {
                    DualSliderHandle::Min => (new_val.clamp(1, cmax), cmax),
                    DualSliderHandle::Max => (cmin, new_val.clamp(cmin, 366)),
                });
            }
            5 => {
                let (cmin, cmax) = tf_config.weeks.unwrap_or((1, 52));
                tf_config.weeks = Some(match handle {
                    DualSliderHandle::Min => (new_val.clamp(1, cmax), cmax),
                    DualSliderHandle::Max => (cmin, new_val.clamp(cmin, 52)),
                });
            }
            6 => {
                let (cmin, cmax) = tf_config.months.unwrap_or((1, 12));
                tf_config.months = Some(match handle {
                    DualSliderHandle::Min => (new_val.clamp(1, cmax), cmax),
                    DualSliderHandle::Max => (cmin, new_val.clamp(cmin, 12)),
                });
            }
            _ => return,
        }

        self.panel_app.compare_settings_state.cached_timeframe_visibility = Some(tf_config.clone());
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.compare_overlay.set_series_timeframe_visibility(series_idx, tf_config);
        }
        self.autosave_snapshot();
        eprintln!("[ChartApp] cmp_settings tf_{} {:?} committed: {}", tf_idx, handle, new_val);
    }

    /// Commit the active compare-settings sub-field from TextFieldStore (Wave 2b).
    ///
    /// Reads the current text from the store, takes `active_field_id`, blurs the
    /// field, then delegates to `apply_cmp_tf_text_commit`.
    pub(crate) fn commit_cmp_editing_text(&mut self) {
        let cmp_wid = uzor::WidgetId::from(crate::text_input::COMPARE_SETTINGS_FIELD);
        let field_id = match self.panel_app.compare_settings_state.active_field_id.take() {
            Some(f) => f,
            None => return,
        };
        let text = self.input_coordinator.borrow().text_fields().text(&cmp_wid).to_string();
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        self.apply_cmp_tf_text_commit(&field_id, &text);
    }

    /// Handle clicks on the indicator overlay widget (top-left of chart).
    ///
    /// Widget IDs registered in render() (single mode):
    /// - `ind_overlay:toggle` — toggle dropdown open/close
    /// - `ind_overlay:close` — close the dropdown
    /// - `ind_overlay:vis:{id}` — toggle indicator visibility
    /// - `ind_overlay:settings:{id}` — open indicator settings modal
    /// - `ind_overlay:delete:{id}` — remove indicator instance
    ///
    /// Widget IDs registered in split mode:
    /// - `ind_overlay:leaf{leaf_id}:toggle` — toggle per-leaf dropdown
    /// - `ind_overlay:leaf{leaf_id}:close` — close per-leaf dropdown
    /// - `ind_overlay:leaf{leaf_id}:vis:{id}` — toggle indicator visibility
    /// - `ind_overlay:leaf{leaf_id}:settings:{id}` — open indicator settings
    /// - `ind_overlay:leaf{leaf_id}:delete:{id}` — remove indicator instance
    pub(super) fn handle_indicator_overlay_click(&mut self, rest: &str) {
        // Trading container's embedded center chart (chart-in-container
        // arc, 2026-07-26): `tcc:{action}` — its overlay state lives on
        // `trading_center_ind_overlay`, not the leaf-keyed map.
        if let Some(action) = rest.strip_prefix("tcc:") {
            self.handle_trading_center_ind_overlay_action(action);
            return;
        }

        // In split mode the widget ID has the form `leaf{leaf_id}:{action}`.
        // Detect that prefix and route to per-leaf state.
        if rest.starts_with("leaf") {
            // Extract leaf_id: rest = "leaf{n}:{action}"
            if let Some(colon_pos) = rest.find(':') {
                let leaf_id_str = &rest["leaf".len()..colon_pos];
                let action = &rest[colon_pos + 1..];
                if let Ok(leaf_raw) = leaf_id_str.parse::<u64>() {
                    let leaf_id = mlc_core::LeafId(leaf_raw);
                    self.handle_leaf_indicator_overlay_action(leaf_id, action);
                    return;
                }
            }
            eprintln!("[ChartApp] ind_overlay leaf prefix malformed: {}", rest);
            return;
        }

        // Single-window mode: operate on the shared indicator_overlay_state.
        self.handle_indicator_overlay_action(rest);
    }

    /// Apply an indicator overlay action using the single-window (non-split) state.
    pub(super) fn handle_indicator_overlay_action(&mut self, action: &str) {
        match action {
            "toggle" => {
                self.panel_app.indicator_overlay_state.toggle();
                eprintln!("[ChartApp] indicator overlay toggled: {}", self.panel_app.indicator_overlay_state.is_open);
            }
            "close" => {
                self.panel_app.indicator_overlay_state.close();
                eprintln!("[ChartApp] indicator overlay closed");
            }
            _ if action.starts_with("vis:") => {
                if let Ok(id) = action["vis:".len()..].parse::<u64>() {
                    self.core.indicators_mut().toggle_visibility(id);
                    // Visibility is PERSISTED state (`IndicatorSnapshot.visible`),
                    // so the toggle has to reach disk like every other edit in
                    // this handler — without this the eye icon reset on reload
                    // (owner report 2026-08-02). The save and restore paths were
                    // both already correct; nothing was writing the snapshot.
                    self.autosave_snapshot();
                    eprintln!("[ChartApp] indicator {} visibility toggled", id);
                }
            }
            _ if action.starts_with("settings:") => {
                if let Ok(id) = action["settings:".len()..].parse::<u64>() {
                    self.panel_app.indicator_settings_state.open(id);
                    eprintln!("[ChartApp] indicator {} settings opened", id);
                }
            }
            _ if action.starts_with("delete:") => {
                if let Ok(id) = action["delete:".len()..].parse::<u64>() {
                    self.delete_indicator_instance(id);
                }
            }
            _ if action.starts_with("alert:") => {
                if let Some(id_str) = action.strip_prefix("alert:") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        let price = self.panel_app.panel_grid.active_container()
                            .and_then(|_w| None::<&mlc_core::Bar>)
                            .map(|b| b.close)
                            .unwrap_or(0.0);
                        let label = self.core.indicators().name_of(id)
                            .unwrap_or_else(|| format!("Indicator {}", id));
                        let symbol = self.panel_app.panel_grid.active_container_id()
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_else(|| "BTCUSD".to_string());
                        let source = mlc_core::alert::AlertSource::Signal {
                            indicator_id: id,
                            label: label.clone(),
                            direction_filter: mlc_core::alert::SignalDirection::Any,
                            bar_state: mlc_core::alert::SignalBarState::Forming,
                            kind_filter: None,
                        };
                        self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                        self.panel_app.alert_settings_state.pin_initial_position(
                            self.content_rect.width, self.content_rect.height,
                        );
                        eprintln!("[ChartApp] ind_overlay alert: opened modal for indicator {} ({})", id, label);
                    }
                }
            }
            _ => {
                eprintln!("[ChartApp] ind_overlay unhandled: {}", action);
            }
        }
    }

    /// Apply an indicator overlay action for a specific leaf (split mode).
    pub(super) fn handle_leaf_indicator_overlay_action(&mut self, leaf_id: mlc_core::LeafId, action: &str) {
        match action {
            "toggle" => {
                let state = self.panel_app.indicator_overlay_state_for_leaf_mut(leaf_id);
                state.toggle();
                eprintln!("[ChartApp] leaf {} indicator overlay toggled: {}", leaf_id.0, state.is_open);
            }
            "close" => {
                let state = self.panel_app.indicator_overlay_state_for_leaf_mut(leaf_id);
                state.close();
                eprintln!("[ChartApp] leaf {} indicator overlay closed", leaf_id.0);
            }
            _ if action.starts_with("vis:") => {
                if let Ok(id) = action["vis:".len()..].parse::<u64>() {
                    self.core.indicators_mut().toggle_visibility(id);
                    self.autosave_snapshot();
                    eprintln!("[ChartApp] indicator {} visibility toggled (leaf {})", id, leaf_id.0);
                }
            }
            _ if action.starts_with("settings:") => {
                if let Ok(id) = action["settings:".len()..].parse::<u64>() {
                    self.panel_app.indicator_settings_state.open(id);
                    eprintln!("[ChartApp] indicator {} settings opened (leaf {})", id, leaf_id.0);
                }
            }
            _ if action.starts_with("delete:") => {
                if let Ok(id) = action["delete:".len()..].parse::<u64>() {
                    self.delete_indicator_instance(id);
                }
            }
            _ if action.starts_with("alert:") => {
                if let Some(id_str) = action.strip_prefix("alert:") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        let price = self.panel_app.panel_grid.active_container()
                            .and_then(|_w| None::<&mlc_core::Bar>)
                            .map(|b| b.close)
                            .unwrap_or(0.0);
                        let label = self.core.indicators().name_of(id)
                            .unwrap_or_else(|| format!("Indicator {}", id));
                        let symbol = self.panel_app.panel_grid.active_container_id()
                            .and_then(|cid| self.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_else(|| "BTCUSD".to_string());
                        let source = mlc_core::alert::AlertSource::Signal {
                            indicator_id: id,
                            label: label.clone(),
                            direction_filter: mlc_core::alert::SignalDirection::Any,
                            bar_state: mlc_core::alert::SignalBarState::Forming,
                            kind_filter: None,
                        };
                        self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                        self.panel_app.alert_settings_state.pin_initial_position(
                            self.content_rect.width, self.content_rect.height,
                        );
                        eprintln!("[ChartApp] ind_overlay alert (leaf {}): opened modal for indicator {} ({})", leaf_id.0, id, label);
                    }
                }
            }
            _ if action.starts_with("row:") => {
                // row is registered for hit-zone coverage but has no action.
            }
            _ => {
                eprintln!("[ChartApp] ind_overlay leaf {} unhandled action: {}", leaf_id.0, action);
            }
        }
    }

    /// Remove an indicator instance and record the command in the active window history.
    ///
    /// When the deleted indicator belongs to a tagged sync group, ALL instances
    /// of the same `type_id` across ALL group member windows are removed so that
    /// peer windows do not keep stale renders alive.
    pub(crate) fn delete_indicator_instance(&mut self, id: u64) {
        let type_id = match self.core.indicators().type_id_of(id) {
            Some(type_id) => type_id,
            None => return,
        };

        // No special case remains: EVERY host-rendered overlay is a real
        // `IndicatorInstance` now, so removing the legend row removes the
        // instance, the render gate (derived from a visible instance) goes
        // false by itself, and the bubble's per-tick feed reconcile
        // (`ensure_bubble_indicator_feeds`) drops the feed nobody declares
        // anymore. The symmetric intercept that used to live here turned a
        // delete into a Toggle event, which is why these ids could never be
        // deleted like an ordinary indicator either.

        self.push_undo_command(mlc_core::Command::RemoveIndicator {
            instance_id: id,
            type_id: type_id.clone(),
            params_json: String::new(),
        });
        eprintln!("[ChartApp] Recorded RemoveIndicator {} id={}", type_id, id);

        // Resolve the bubble the active chart points at — indicator ownership
        // is by bubble, so peer instances live with the same `bubble_id`,
        // regardless of which chart originally created them.
        let active_bubble_id: Option<u64> = self.panel_app.panel_grid.docking()
            .active_leaf()
            .and_then(|leaf| self.panel_app.panel_grid.container_id_for_leaf(leaf))
            .and_then(|cid| self.bubble_for_chart(cid))
            .map(|b| b.0);

        // Remove at most ONE indicator_config with this type_id from the group.
        // Using retain-all would incorrectly delete configs for duplicate indicators
        // of the same type (e.g. two "ac" indicators on the same window).
        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
            if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(active_leaf) {
                if let Some(group_id) = self.bubble_for_chart(chart_id) {
                    let removed = self.core.bubbles_mut().with_bubble_mut(group_id, |group| {
                        let pos = group.indicator_configs.iter().position(|c| c.type_id == type_id)?;
                        group.indicator_configs.remove(pos);
                        Some(())
                    }).flatten();
                    if removed.is_some() {
                        eprintln!(
                            "[BubbleStore] Removed 1 indicator config with type_id={} from group {:?}",
                            type_id,
                            group_id,
                        );
                    }
                }
            }
        }

        // Indicator clones live in the same bubble as their origin under the
        // bubble-ownership model, so peer instances are simply other entries
        // of the same `type_id` sharing the active chart's bubble.  The
        // `id != id` guard keeps the primary instance from being listed
        // twice in `all_to_remove`.
        let peer_instances: Vec<u64> = active_bubble_id
            .map(|bid| {
                self.core.indicators().instances_for_bubble(mlc_core::ids::BubbleId(bid))
                    .into_iter()
                    .filter(|inst| inst.type_id == type_id && inst.id != id)
                    .map(|inst| inst.id)
                    .collect()
            })
            .unwrap_or_default();

        // Build the full removal list: the specific instance + its peers on other windows.
        let mut all_to_remove = vec![id];
        all_to_remove.extend(peer_instances);

        for inst_id in &all_to_remove {
            self.core.indicators_mut().remove(*inst_id);
            self.core.alerts_mut().clear_for_indicator(*inst_id);
        }

        // Last CVD instance for this bubble just went away — cancel the
        // real aggressor-CVD subscription so it stops writing into a bubble
        // nothing renders from anymore.
        if type_id == "cvd" {
            if let Some(bid) = active_bubble_id {
                self.unsubscribe_cvd_line_if_last(mlc_core::ids::BubbleId(bid));
            }
        }

        // Remove stale entries from auto_bubble.pre_tag_indicator_ids for every window
        // so the Object Tree does not show phantom items for deleted indicators.
        {
            let auto_bids: Vec<mlc_core::ids::BubbleId> = self.panel_app.panel_grid
                .containers()
                .values()
                .map(|w| w.auto_tag_id)
                .filter(|&bid| bid != mlc_core::ids::BubbleId(0))
                .collect();
            for bid in auto_bids {
                self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                    auto_bub.pre_tag_indicator_ids.retain(|iid| !all_to_remove.contains(iid));
                });
            }
        }

        self.sync_sub_panes_from_manager();
        self.autosave_snapshot();
        self.sidebar_data_dirty = true;
        eprintln!(
            "[ChartApp] indicator {} deleted (type={}, removed {} instance(s))",
            id,
            type_id,
            all_to_remove.len(),
        );
    }

    /// Keep every bubble subscribed to the feeds ITS indicators declared.
    ///
    /// Data is pulled by the bubble, never by the UI: an indicator states
    /// what it consumes in its mli signature (`input_stream` /
    /// `aux_streams`), and the bubble that hosts it keeps that feed alive —
    /// exactly how it already keeps klines and trades alive for the chart
    /// drawn on it. The indicator then reads the layer off the bubble.
    ///
    /// Runs once per tick and is idempotent, so adding an indicator,
    /// deleting it, switching the instrument or restoring a preset all
    /// converge here instead of each click site arming feeds by hand.
    /// `Bar`/`Tick` need no action — those layers are always on for a
    /// bubble with a chart.
    pub(crate) fn ensure_bubble_indicator_feeds(&mut self) {
        use mlc_backend_indicator_engine::StreamKind;

        // What each bubble's indicators declare, and where that bubble is
        // rendered (its instrument comes from the window bound to it).
        let mut wanted: std::collections::HashMap<mlc_core::ids::BubbleId, std::collections::HashSet<StreamKind>> =
            std::collections::HashMap::new();
        for inst in self.core.indicators().all_instances() {
            let Some(bid) = inst.bubble_id else { continue };
            let streams = self.core.indicators().definition_streams(&inst.type_id);
            if streams.is_empty() {
                continue;
            }
            let entry = wanted.entry(mlc_core::ids::BubbleId(bid)).or_default();
            for s in streams {
                if !matches!(s, StreamKind::Bar | StreamKind::Tick) {
                    entry.insert(s);
                }
            }
        }

        let targets: Vec<(mlc_core::state::ChartId, mlc_core::ids::BubbleId)> = self
            .panel_app.panel_grid.containers().keys()
            .filter_map(|cid| self.bubble_for_chart(*cid).map(|bid| (*cid, bid)))
            .collect();

        for (cid, bid) in targets {
            let want = wanted.get(&bid).cloned().unwrap_or_default();
            let held = self.bubble_feeds.get(&bid).cloned().unwrap_or_default();
            if want == held {
                continue;
            }
            let (Some(symbol), Some(at)) = (self.symbol_for_chart(cid), self.account_type_for_chart(cid))
            else { continue };
            if symbol.is_empty() {
                continue;
            }
            let eid = self.exchange_id_for_chart(cid);
            if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                continue;
            }
            let at_dig3 = mlc_backend_data_bridge::account_type_to_dig3(at);

            for stream in want.difference(&held) {
                match stream {
                    StreamKind::OrderBook => {
                        self.core.bubbles_mut().with_bubble_mut(bid, |b| {
                            mlc_backend::chart_type::clear_slot_dom_heatmap(b);
                        });
                        // The subscriptions themselves are armed by
                        // `ensure_dom_book_rosters` (frame_lifecycle/
                        // dom_books.rs), which knows the overlay's venue set.
                        // Arming the chart's own book here as well would
                        // leave it up when the roster later drops it — this
                        // reconcile only fires on a change of the declared
                        // STREAM set, and a venue edit is not one.
                        crate::sglog!("[bubble {:?}] book slot cleared ({} {:?})", bid, symbol, eid);
                    }
                    StreamKind::OpenInterest => {
                        // The projected-liquidation overlay models FROM open
                        // interest, so it raises the same feed the OI Delta
                        // pane uses — one subscription, and a chart showing
                        // both cannot disagree with itself about what OI did.
                        self.core.ensure_connector(eid);
                        self.core.bridge_handle().request_open_interest(eid, &symbol, at_dig3);
                        crate::sglog!("[bubble {:?}] OI feed up ({} {:?})", bid, symbol, eid);
                    }
                    StreamKind::Funding => {
                        // The Funding Rate sub-pane declares this stream, and
                        // without an arm here it fell into the catch-all
                        // below: the pane appeared, correctly, and stayed
                        // empty forever because nothing ever subscribed.
                        self.core.ensure_connector(eid);
                        self.core.bridge_handle().request_funding_rate(eid, &symbol, at_dig3);
                        crate::sglog!("[bubble {:?}] funding feed up ({} {:?})", bid, symbol, eid);
                    }
                    StreamKind::Liquidation => {
                        self.core.bubbles_mut().with_bubble_mut(bid, |b| {
                            mlc_backend::chart_type::clear_slot_liq_heatmap(b);
                        });
                        // The subscription itself is armed by
                        // `ensure_liquidation_roster` (frame_lifecycle/
                        // liq_pump.rs), which knows the overlay's aggregate
                        // flag and coerces to Futures. Arming here too would
                        // open the chart's OWN market — Spot on a spot chart,
                        // where no venue has a liquidation channel at all.
                        crate::sglog!("[bubble {:?}] liquidation slot cleared ({} {:?})", bid, symbol, eid);
                    }
                    _ => {}
                }
            }
            for stream in held.difference(&want) {
                match stream {
                    StreamKind::OrderBook => {
                        // Released by `ensure_dom_book_rosters` on the next
                        // frame: with the overlay gone its wanted set is
                        // empty, so every book it held comes down there.
                        // Tearing one down here as well would race the
                        // roster's own bookkeeping.
                        crate::sglog!("[bubble {:?}] book feed released", bid);
                    }
                    StreamKind::Liquidation => {
                        // Nothing to tear down: the ring is shared station
                        // state with its own grace window, and the tape panel
                        // may still be reading it.
                        crate::sglog!("[bubble {:?}] liquidation feed released", bid);
                    }
                    _ => {}
                }
            }
            self.bubble_feeds.insert(bid, want);
        }
    }
}
