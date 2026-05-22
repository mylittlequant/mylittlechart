//! Process ChartOutEvent variants — propagates chart-internal events
//! (viewport, timeframe, symbol, indicator, drawing changes) to sync groups,
//! tag manager, autosave, and other downstream consumers.

use crate::ChartApp;
use zengeld_chart::UIStyle;
use zengeld_chart::ui::modal_state::OpenModal;

impl ChartApp {
    /// Process a `ChartOutEvent` — route high-level outcomes back to state.
    pub(crate) fn process_chart_out_event(&mut self, event: zengeld_chart::events::ChartOutEvent) {
        use zengeld_chart::events::ChartOutEvent;
        use zengeld_chart::state::Timeframe;

        // Tracks whether this event mutated chart state (triggers autosave).
        let mut state_mutated = false;

        match event {
            ChartOutEvent::ChangeTimeframe { timeframe_id } => {
                eprintln!("[ChartApp] ChangeTimeframe: {}", timeframe_id);
                let timeframe = match timeframe_id.as_str() {
                    "tf_1m"  => Some(Timeframe::m1()),
                    "tf_3m"  => Some(Timeframe::new("3m", 3)),
                    "tf_5m"  => Some(Timeframe::m5()),
                    "tf_15m" => Some(Timeframe::m15()),
                    "tf_30m" => Some(Timeframe::m30()),
                    "tf_1h"  => Some(Timeframe::h1()),
                    "tf_2h"  => Some(Timeframe::new("2H", 120)),
                    "tf_4h"  => Some(Timeframe::h4()),
                    "tf_6h"  => Some(Timeframe::new("6H", 360)),
                    "tf_12h" => Some(Timeframe::new("12H", 720)),
                    "tf_1d"  => Some(Timeframe::d1()),
                    "tf_1w"  => Some(Timeframe::w1()),
                    "tf_1M"  => Some(Timeframe::mn1()),
                    _ => {
                        eprintln!("[ChartApp] unknown timeframe_id: {}", timeframe_id);
                        None
                    }
                };
                if let Some(tf) = timeframe {
                    // Capture previous timeframe and active leaf BEFORE changing.
                    let previous_timeframe = self.panel_app.panel_grid.active_window()
                        .map(|w| w.timeframe.clone());
                    let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                    // Extract whether timeframe actually changed before mutating.
                    // prev_tf_opt: Some(prev) means there was a window; None means no window.
                    let prev_tf_opt = previous_timeframe;
                    let tf_changed = match prev_tf_opt {
                        Some(ref prev_tf) if *prev_tf != tf => {
                            self.push_undo_command(zengeld_chart::Command::ChangeTimeframe {
                                previous_timeframe: prev_tf.clone(),
                                new_timeframe: tf.clone(),
                            });
                            eprintln!("[ChartApp] Recorded ChangeTimeframe {} -> {}", prev_tf.name, tf.name);
                            true
                        }
                        Some(_) => false,
                        None => true,
                    };
                    // Set timeframe, clear bars, and request new data asynchronously.
                    // (change_timeframe tries a sync cache lookup which fails for live data.)
                    let symbol = self.panel_app.panel_grid.active_window()
                        .map(|w| w.symbol.clone())
                        .unwrap_or_default();
                    let at_label_tf = self.panel_app.panel_grid.active_window()
                        .map(|w| w.account_type.clone())
                        .unwrap_or_else(|| "S".to_string());
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.timeframe = tf.clone();
                        window.update_title();
                        window.bars.clear();
                        window.viewport.bar_count = 0;
                        window.viewport.view_start = 0.0;
                    }
                    // Fetch bars for new timeframe. Trade stream stays alive вЂ” trades
                    // build bars regardless of timeframe and the symbol has not changed.
                    if !symbol.is_empty() {
                        let eid_str = self.active_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping request_bars (timeframe change)", eid_str);
                        } else {
                            let at = crate::account_type_from_label(&at_label_tf);
                            self.bridge.request_bars(self.active_exchange, &symbol, &tf, at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), true);
                        }
                    }
                    // Propagate new timeframe to all leaves in the same sync group.
                    if tf_changed {
                        if let Some(leaf) = active_leaf {
                            self.propagate_timeframe_to_sync_group(leaf, tf);
                        }
                        self.sidebar_data_dirty = true;
                        state_mutated = true;
                    }
                }
            }
            ChartOutEvent::ChangeChartType { chart_type } => {
                eprintln!("[ChartApp] ChangeChartType: {}", chart_type);
                // Map the incoming String to a &'static str used by ChartWindow.
                let static_type: Option<&'static str> = match chart_type.as_str() {
                    "candles"       => Some("candles"),
                    "hollow_candles" => Some("hollow_candles"),
                    "heikin_ashi"   => Some("heikin_ashi"),
                    "bars"          => Some("bars"),
                    "line"          => Some("line"),
                    "step_line"     => Some("step_line"),
                    "line_markers"  => Some("line_markers"),
                    "area"          => Some("area"),
                    "hlc_area"      => Some("hlc_area"),
                    "baseline"      => Some("baseline"),
                    "histogram"     => Some("histogram"),
                    "columns"       => Some("columns"),
                    _ => {
                        eprintln!("[ChartApp] unknown chart_type: {}", chart_type);
                        None
                    }
                };
                if let Some(ct) = static_type {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.set_chart_type_with_undo(ct);
                    }
                    state_mutated = true;
                }
            }
            ChartOutEvent::Consumed => {}
            ChartOutEvent::ToggleIndicators => {
                self.modal_state.toggle(OpenModal::IndicatorSearch);
                if self.modal_state.current == OpenModal::IndicatorSearch {
                    self.modal_state.start_editing(0);
                }
                eprintln!("[ChartApp] indicator search modal toggled: {:?}", self.modal_state.current);
            }
            ChartOutEvent::OpenSymbolSearch => {
                self.modal_state.open(OpenModal::SymbolSearch);
                self.modal_state.symbol_search_results =
                    crate::ChartApp::build_demo_symbol_results("", &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
                self.modal_state.start_editing(0);
                eprintln!("[ChartApp] symbol search modal opened");
            }
            ChartOutEvent::OpenCompareSearch => {
                self.modal_state.open(OpenModal::CompareSearch);
                self.modal_state.symbol_search_results =
                    crate::ChartApp::build_demo_symbol_results("", &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
                self.modal_state.start_editing(0);
                eprintln!("[ChartApp] compare search modal opened");
            }
            // Chart settings modal вЂ” opened by gear dropdown "Chart Settings..." item.
            ChartOutEvent::OpenChartSettings => {
                self.panel_app.chart_settings_state.toggle();
                eprintln!("[ChartApp] chart_settings modal toggled via settings dropdown: {}", self.panel_app.chart_settings_state.is_open);
            }

            // User settings modal вЂ” opened by the chrome gear button.
            ChartOutEvent::OpenUserSettings => {
                self.panel_app.user_settings_state.toggle();
                eprintln!("[ChartApp] user_settings modal toggled: {}", self.panel_app.user_settings_state.is_open);
            }

            // Quick-settings toggles from the settings gear dropdown.
            ChartOutEvent::ToggleGrid => {
                // Read new value from active, then broadcast to all windows
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.toggle_grid();
                    let v = w.grid_options.vert_lines.visible;
                    let h = w.grid_options.horz_lines.visible;
                    Some((v, h))
                } else { None };
                if let Some((v, h)) = new_val {
                    for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                        w.grid_options.vert_lines.visible = v;
                        w.grid_options.horz_lines.visible = h;
                    }
                }
                eprintln!("[ChartApp] grid toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleCrosshair => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.toggle_crosshair();
                    Some(w.crosshair.enabled)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                        w.crosshair.enabled = val;
                    }
                }
                eprintln!("[ChartApp] crosshair toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegend => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_legend();
                    eprintln!("[ChartApp] legend toggled");
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleWatermark => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.toggle_watermark();
                    w.watermark.as_ref().map(|wm| wm.visible)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                        if let Some(ref mut wm) = w.watermark {
                            wm.visible = val;
                        }
                    }
                }
                eprintln!("[ChartApp] watermark toggled (all windows)");
                state_mutated = true;
            }

            // Left panel вЂ” no sidebar in standalone mode.
            ChartOutEvent::ToggleLeftPanel => {
                eprintln!("[ChartApp] Not available in standalone mode: {:?}", event);
            }

            // Right sidebar panels вЂ” handled via SidebarState.
            // toggle_right_panel returns Option<(bool, f64)>:
            //   Some((true,  w)) в†’ sidebar opened  в†’ compensate viewport rightward
            //   Some((false, w)) в†’ sidebar closed  в†’ compensate viewport leftward
            //   None             в†’ panel switched  в†’ no viewport change needed
            ChartOutEvent::ToggleWatchlist => {
                let result = self.sidebar_state.toggle_right_panel(
                    sidebar_content::state::RightSidebarPanel::Watchlist,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Watchlist panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAlerts => {
                let result = self.sidebar_state.toggle_right_panel(
                    sidebar_content::state::RightSidebarPanel::Alerts,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Alerts panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleObjectTree => {
                let result = self.sidebar_state.toggle_right_panel(
                    sidebar_content::state::RightSidebarPanel::ObjectTree,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Object Tree panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleSignals => {
                let result = self.sidebar_state.toggle_right_panel(
                    sidebar_content::state::RightSidebarPanel::Signals,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Signals panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleConnectors => {
                let result = self.sidebar_state.toggle_right_panel(
                    sidebar_content::state::RightSidebarPanel::Connectors,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Connectors panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::TogglePerformance => {
                let result = self.sidebar_state.toggle_right_panel(
                    sidebar_content::state::RightSidebarPanel::Performance,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Performance panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAgents => {
                let result = self.sidebar_state.toggle_right_panel(
                    sidebar_content::state::RightSidebarPanel::Agents,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Agents panel: {}", if opening { "opened" } else { "closed" });
                    // Spawn-on-demand: PTY only starts when user explicitly clicks [Start].
                    let _ = opening;
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleSlot(idx) => {
                let panel = sidebar_content::state::RightSidebarPanel::from_slot_index(idx);
                let _ = self.sidebar_state.toggle_right_panel(panel);
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            // Split layout events вЂ” wire to ChartPanelGrid split system.
            // After each split all new leaves receive a shared sync color tag so
            // that symbol / timeframe changes propagate across the split group.
            ChartOutEvent::InternalSplitHorizontal => { self.do_split(zengeld_chart::SplitKind::SplitRight); state_mutated = true; }
            ChartOutEvent::InternalSplitVertical => { self.do_split(zengeld_chart::SplitKind::SplitBottom); state_mutated = true; }
            ChartOutEvent::InternalSplitGrid2x2 => { self.do_split(zengeld_chart::SplitKind::Grid2x2); state_mutated = true; }
            ChartOutEvent::InternalSplit2Left1Right => { self.do_split(zengeld_chart::SplitKind::TwoLeftOneRight); state_mutated = true; }
            ChartOutEvent::InternalSplit1Left2Right => { self.do_split(zengeld_chart::SplitKind::OneLeftTwoRight); state_mutated = true; }
            ChartOutEvent::InternalSplit2Top1Bottom => { self.do_split(zengeld_chart::SplitKind::TwoTopOneBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit1Top2Bottom => { self.do_split(zengeld_chart::SplitKind::OneTopTwoBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit3Columns => { self.do_split(zengeld_chart::SplitKind::ThreeColumns); state_mutated = true; }
            ChartOutEvent::InternalSplit3Rows => { self.do_split(zengeld_chart::SplitKind::ThreeRows); state_mutated = true; }
            ChartOutEvent::InternalSplit1Big3Small => { self.do_split(zengeld_chart::SplitKind::OneBig3Small); state_mutated = true; }
            ChartOutEvent::InternalSetLayoutSingle => {
                self.panel_app.panel_grid.set_layout_single();
                state_mutated = true;
            }
            ChartOutEvent::InternalToggleExpand => {
                self.panel_app.panel_grid.toggle_expand();
                state_mutated = true;
            }
            ChartOutEvent::InternalClosePanel => {
                if let Some(leaf_id) = self.panel_app.panel_grid.docking().active_leaf() {
                    self.panel_app.panel_grid.close_leaf(leaf_id);
                }
                state_mutated = true;
            }
            ChartOutEvent::InternalResetSizes => {
                self.panel_app.panel_grid.reset_sizes();
                state_mutated = true;
            }
            ChartOutEvent::ToggleGridVertical => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.toggle_grid_vertical();
                    Some(w.grid_options.vert_lines.visible)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                        w.grid_options.vert_lines.visible = val;
                    }
                }
                eprintln!("[ChartApp] grid vertical toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleGridHorizontal => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.toggle_grid_horizontal();
                    Some(w.grid_options.horz_lines.visible)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                        w.grid_options.horz_lines.visible = val;
                    }
                }
                eprintln!("[ChartApp] grid horizontal toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendOHLC => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_legend_ohlc();
                    eprintln!("[ChartApp] legend OHLC toggled -> {}", window.legend.show_ohlc);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendChange => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_legend_change();
                    eprintln!("[ChartApp] legend change toggled -> {}", window.legend.show_change);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendPercent => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_legend_percent();
                    eprintln!("[ChartApp] legend percent toggled -> {}", window.legend.show_percent);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleTooltip => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.toggle_tooltip();
                    Some(w.tooltip.visible)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                        w.tooltip.visible = val;
                    }
                }
                eprintln!("[ChartApp] tooltip toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleTooltipFollow => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.toggle_tooltip_follow();
                    Some(w.tooltip.follow_cursor)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                        w.tooltip.follow_cursor = val;
                    }
                }
                eprintln!("[ChartApp] tooltip follow toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::SetWatermarkText(text) => {
                for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                    w.set_watermark_text(text);
                }
                eprintln!("[ChartApp] watermark text set to {} (all windows)", text);
                state_mutated = true;
            }
            ChartOutEvent::SetWatermarkPosition(pos) => {
                use zengeld_chart::{HorzAlign, VertAlign};
                let (horz, vert) = match pos {
                    "center"       => (HorzAlign::Center, VertAlign::Center),
                    "bottom_left"  => (HorzAlign::Left,   VertAlign::Bottom),
                    "bottom_right" => (HorzAlign::Right,  VertAlign::Bottom),
                    "top_left"     => (HorzAlign::Left,   VertAlign::Top),
                    "top_right"    => (HorzAlign::Right,  VertAlign::Top),
                    _              => (HorzAlign::Left,   VertAlign::Bottom),
                };
                for w in self.panel_app.panel_grid.windows_mut().values_mut() {
                    w.set_watermark_position(horz, vert);
                }
                eprintln!("[ChartApp] watermark position set to {} (all windows)", pos);
                state_mutated = true;
            }
            // === Presets (in-memory HashMap) ===
            ChartOutEvent::SavePreset { name } => {
                // "Save As" вЂ” create a new named preset from current state.
                use zengeld_chart::preset::preset::unix_timestamp_parts;
                let (secs, nanos) = unix_timestamp_parts();
                let id = format!("preset_{}_{}", secs, nanos);
                eprintln!("[ChartApp] save-as preset '{}' в†’ id={}", name, id);
                self.collect_state_into_preset(&id, &name);
                self.panel_app.active_preset_id = id.clone();
                // Add the new preset to open_tabs.
                if !self.panel_app.open_tabs.contains(&id) {
                    self.panel_app.open_tabs.push(id.clone());
                }
                if let Some(preset) = self.panel_app.presets.get(&id).cloned() {
                    self.preset_actions.push(crate::PresetAction::Upsert(preset));
                }
                self.persist_profile();
                if let Some(p) = self.panel_app.presets.get(&id) {
                    eprintln!(
                        "[ChartApp] preset '{}' saved ({} windows, {} groups, {} indicators)",
                        id, p.windows.len(), p.sync_groups.len(), p.indicators.len()
                    );
                }
            }

            ChartOutEvent::LoadPreset { id } => {
                eprintln!("[ChartApp] load preset: {}", id);
                let prev_id = self.panel_app.active_preset_id.clone();

                // Same tab вЂ” no-op.
                if prev_id == id {
                    eprintln!("[ChartApp] LoadPreset: already active, skipping");
                    state_mutated = true;
                } else {

                // Save the outgoing preset (with bars) before switching away.
                if !prev_id.is_empty() && prev_id != "__default__" {
                    self.autosave_snapshot();
                }

                // Park outgoing state into live cache.
                if !prev_id.is_empty() {
                    self.park_active_preset(&prev_id);
                }

                self.panel_app.active_preset_id = id.clone();
                self.persist_profile();

                // Warm path: live cache hit вЂ” zero-flicker switch.
                if self.unpark_preset(&id) {
                    eprintln!("[ChartApp] LoadPreset '{}': live cache hit вЂ” zero-flicker switch", id);

                    // Cure: request 300 fresh bars for every window so gaps are
                    // healed, stale data is refreshed, and bar store is updated.
                    let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
                    let window_bar_data: Vec<(String, String, zengeld_chart::state::Timeframe, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_windows()
                        .map(|(_, w)| (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone()))
                        .collect();
                    let mut bars_requested: usize = 0;
                    for (sym, exch, tf, at_label) in &window_bar_data {
                        let eid = digdigdig3::ExchangeId::from_str(exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                            continue;
                        }
                        let at = crate::account_type_from_label(at_label);
                        self.bridge.ensure_connector(eid);
                        self.bridge.request_bars(eid, sym, tf, at, None, Some(bar_count), true);
                        bars_requested += 1;
                    }
                    eprintln!("[ChartApp] LoadPreset (cache): cure requested for {} windows", bars_requested);

                    self.sidebar_data_dirty = true;
                    state_mutated = true;
                } else if let Some(preset) = self.panel_app.presets.get(&id).cloned() {
                    eprintln!(
                        "[ChartApp] applying preset '{}': {} windows, {} groups, {} indicators",
                        preset.name, preset.windows.len(), preset.sync_groups.len(), preset.indicators.len()
                    );

                    // ----------------------------------------------------------------
                    // Collect current (exchange_id, symbol) pairs before switching.
                    // Used after the preset is loaded to unsubscribe stale trade actors.
                    // ----------------------------------------------------------------
                    let old_subscriptions: std::collections::HashSet<(digdigdig3::ExchangeId, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_windows()
                        .filter_map(|(_, w)| {
                            digdigdig3::ExchangeId::from_str(&w.exchange)
                                .map(|eid| (eid, w.symbol.clone()))
                        })
                        .collect();

                    // ----------------------------------------------------------------
                    // Step 1: Attempt full layout restore (new presets with layout).
                    // Falls back to in-place window patching for old presets without.
                    // ----------------------------------------------------------------

                    let layout_restored = if !preset.layout.is_null() {
                        match serde_json::from_value::<uzor::panels::serialize::LayoutSnapshot>(preset.layout.clone()) {
                            Ok(layout_snap) => {
                                // Step 2: Build new windows + leaf_to_chart from snapshot
                                let mut new_windows = std::collections::HashMap::new();
                                let mut new_leaf_to_chart = std::collections::HashMap::new();

                                for snap in &preset.windows {
                                    let leaf_id = zengeld_chart::LeafId(snap.leaf_id);
                                    let chart_id = zengeld_chart::state::chart_window::ChartId(snap.window_id);

                                    let mut window = zengeld_chart::state::chart_window::ChartWindow::with_id(
                                        chart_id,
                                        &snap.symbol,
                                        snap.timeframe.clone(),
                                    );

                                    // Build a data_provider for this window's own exchange and account_type.
                                    let snap_exchange_id = digdigdig3::ExchangeId::from_str(&snap.exchange)
                                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                                    let snap_at = crate::account_type_from_label(&snap.account_type);
                                    window.data_provider = std::sync::Arc::new(
                                        live_data::LiveDataProvider::new(
                                            snap_exchange_id,
                                            snap_exchange_id.as_str().to_string(),
                                            snap_at,
                                            std::sync::Arc::clone(&self.bridge),
                                        ),
                                    );

                                    // Apply all snapshot fields
                                    window.exchange = snap.exchange.clone();
                                    window.account_type = snap.account_type.clone();
                                    window.viewport = snap.viewport.clone();
                                    window.price_scale = snap.price_scale.clone();
                                    window.grid_options = snap.grid_options.clone();
                                    window.crosshair_options = snap.crosshair_options.clone();
                                    window.legend = snap.legend.clone();
                                    window.watermark = snap.watermark.clone();
                                    window.tooltip = snap.tooltip.clone();
                                    window.show_candles = snap.show_candles;
                                    window.show_bars = snap.show_bars;
                                    window.show_hollow_candles = snap.show_hollow_candles;
                                    window.show_heikin_ashi = snap.show_heikin_ashi;
                                    window.show_line = snap.show_line;
                                    window.show_step_line = snap.show_step_line;
                                    window.show_line_markers = snap.show_line_markers;
                                    window.show_area = snap.show_area;
                                    window.show_hlc_area = snap.show_hlc_area;
                                    window.show_histogram = snap.show_histogram;
                                    window.show_columns = snap.show_columns;
                                    window.show_baseline = snap.show_baseline;
                                    window.scale_settings = snap.scale_settings.clone();

                                    // chart_type: snapshot stores String, window holds &'static str
                                    window.chart_type = match snap.chart_type.as_str() {
                                        "bars"           => "bars",
                                        "hollow_candles" => "hollow_candles",
                                        "heikin_ashi"    => "heikin_ashi",
                                        "line"           => "line",
                                        "step_line"      => "step_line",
                                        "line_markers"   => "line_markers",
                                        "area"           => "area",
                                        "hlc_area"       => "hlc_area",
                                        "baseline"       => "baseline",
                                        "histogram"      => "histogram",
                                        "columns"        => "columns",
                                        _                => "candles",
                                    };

                                    // Restore sync group membership
                                    window.group_id = snap.group_id.map(zengeld_chart::tag_manager::SyncGroupId);

                                    // Restore local drawings
                                    window.drawing_manager.clear_all_primitives();
                                    if let Ok(reg) = zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                        for prim_snap in &snap.drawings.primitives {
                                            if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                window.drawing_manager.add_external_primitive(prim);
                                            }
                                        }
                                    }

                                    // Restore command history
                                    if let Some(history) = &snap.command_history {
                                        window.command_history = history.clone();
                                    }
                                    if let Some(stashed) = &snap.stashed_command_history {
                                        window.stashed_command_history = Some(stashed.clone());
                                    }

                                    // Restore stashed primitives (pre-tag drawings)
                                    window.stashed_primitives.clear();
                                    if !snap.stashed_primitives.is_empty() {
                                        if let Ok(reg) = zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                            for prim_snap in &snap.stashed_primitives {
                                                if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                    window.stashed_primitives.push(prim);
                                                }
                                            }
                                        }
                                    }

                                    // Restore pre-tag indicator IDs
                                    window.pre_tag_indicator_ids = snap.pre_tag_indicator_ids.clone();

                                    // Restore per-symbol drawing cache
                                    window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                    // Restore bar_spacing (user's zoom level). chart_width is NOT restored вЂ”
                                    // it comes from layout on the first frame; view_start is NOT restored вЂ”
                                    // snap-to-end always positions to the latest bar.
                                    if snap.viewport.bar_spacing > 0.0 {
                                        window.viewport.bar_spacing = snap.viewport.bar_spacing;
                                    }
                                    // Do NOT set scale_mode here. Instead, stash it into restore_scale_mode
                                    // so set_bars() can apply it AFTER auto-scale completes. This prevents
                                    // the BarsLoaded handler's `window.price_scale.scale_mode = default_scale_mode`
                                    // from clobbering a Manual scale preference.
                                    window.restore_scale_mode = Some(snap.price_scale.scale_mode);

                                    // Bars arrive asynchronously via BarsLoaded.
                                    window.pending_symbol_load = true;

                                    eprintln!(
                                        "[ChartApp] built window {} в†’ {}/{} ({} drawings)",
                                        snap.window_id, snap.symbol, snap.timeframe.name,
                                        snap.drawings.primitives.len()
                                    );

                                    new_windows.insert(chart_id, window);
                                    new_leaf_to_chart.insert(leaf_id, chart_id);
                                }

                                // Bump global ChartId counter past any restored window IDs.
                                for snap in &preset.windows {
                                    zengeld_chart::bump_chart_id_past(snap.window_id);
                                }

                                // Step 3: Restore the docking tree
                                match layout_snap.restore_tree(|_type_id| {
                                    Some(zengeld_chart::state::sub_panel::ChartSubPanel::new(
                                        zengeld_chart::state::chart_window::ChartId(0),
                                        "",
                                    ))
                                }) {
                                    Ok(restored_tree) => {
                                        // Step 4: Replace panel grid state
                                        self.panel_app.panel_grid.replace_docking(restored_tree);
                                        self.panel_app.panel_grid.replace_windows(new_windows);
                                        self.panel_app.panel_grid.replace_leaf_to_chart(new_leaf_to_chart);
                                        eprintln!("[ChartApp] layout topology restored");
                                        true
                                    }
                                    Err(e) => {
                                        eprintln!("[ChartApp] restore_tree failed: {} вЂ” falling back to patch", e);
                                        false
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[ChartApp] layout deserialize failed: {} вЂ” falling back to patch", e);
                                false
                            }
                        }
                    } else {
                        eprintln!("[ChartApp] preset has no layout snapshot вЂ” falling back to patch");
                        false
                    };

                    // ----------------------------------------------------------------
                    // Fallback: patch existing windows in-place (old presets)
                    // ----------------------------------------------------------------
                    if !layout_restored {
                        let windows = self.panel_app.panel_grid.windows_mut();
                        for snap in &preset.windows {
                            let chart_id = zengeld_chart::state::chart_window::ChartId(snap.window_id);
                            if let Some(window) = windows.get_mut(&chart_id) {
                                window.symbol = snap.symbol.clone();
                                window.exchange = snap.exchange.clone();
                                window.account_type = snap.account_type.clone();
                                window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
                                // Rebuild data_provider for this window's (possibly new) exchange.
                                let patch_exchange_id = digdigdig3::ExchangeId::from_str(&snap.exchange)
                                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                                let patch_at = crate::account_type_from_label(&snap.account_type);
                                window.data_provider = std::sync::Arc::new(
                                    live_data::LiveDataProvider::new(
                                        patch_exchange_id,
                                        patch_exchange_id.as_str().to_string(),
                                        patch_at,
                                        std::sync::Arc::clone(&self.bridge),
                                    ),
                                );
                                window.timeframe = snap.timeframe.clone();
                                window.viewport = snap.viewport.clone();
                                window.price_scale = snap.price_scale.clone();
                                window.grid_options = snap.grid_options.clone();
                                window.crosshair_options = snap.crosshair_options.clone();
                                window.legend = snap.legend.clone();
                                window.watermark = snap.watermark.clone();
                                window.tooltip = snap.tooltip.clone();
                                window.show_candles = snap.show_candles;
                                window.show_bars = snap.show_bars;
                                window.show_hollow_candles = snap.show_hollow_candles;
                                window.show_heikin_ashi = snap.show_heikin_ashi;
                                window.show_line = snap.show_line;
                                window.show_step_line = snap.show_step_line;
                                window.show_line_markers = snap.show_line_markers;
                                window.show_area = snap.show_area;
                                window.show_hlc_area = snap.show_hlc_area;
                                window.show_histogram = snap.show_histogram;
                                window.show_columns = snap.show_columns;
                                window.show_baseline = snap.show_baseline;
                                window.scale_settings = snap.scale_settings.clone();

                                // Restore drawings
                                window.drawing_manager.clear_all_primitives();
                                if let Ok(reg) = zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                    for prim_snap in &snap.drawings.primitives {
                                        if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                            window.drawing_manager.add_external_primitive(prim);
                                        }
                                    }
                                }

                                // Restore command history
                                if let Some(history) = &snap.command_history {
                                    window.command_history = history.clone();
                                }
                                if let Some(stashed) = &snap.stashed_command_history {
                                    window.stashed_command_history = Some(stashed.clone());
                                }

                                // Restore stashed primitives (pre-tag drawings)
                                window.stashed_primitives.clear();
                                if !snap.stashed_primitives.is_empty() {
                                    if let Ok(reg) = zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                        for prim_snap in &snap.stashed_primitives {
                                            if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                window.stashed_primitives.push(prim);
                                            }
                                        }
                                    }
                                }

                                // Restore pre-tag indicator IDs
                                window.pre_tag_indicator_ids = snap.pre_tag_indicator_ids.clone();

                                // Restore per-symbol drawing cache
                                window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                // Restore bar_spacing (user's zoom level).
                                if snap.viewport.bar_spacing > 0.0 {
                                    window.viewport.bar_spacing = snap.viewport.bar_spacing;
                                }
                                window.restore_scale_mode = Some(snap.price_scale.scale_mode);

                                // Bars arrive asynchronously via BarsLoaded.
                                window.pending_symbol_load = true;

                                eprintln!("[ChartApp] patched window {} в†’ {}/{} ({} drawings)",
                                    snap.window_id, snap.symbol, snap.timeframe.name,
                                    snap.drawings.primitives.len());
                            }
                        }
                    }

                    // ----------------------------------------------------------------
                    // Step 5: Restore TagManager (sync groups)
                    // ----------------------------------------------------------------
                    self.panel_app.tag_manager.clear();
                    for sg_snap in &preset.sync_groups {
                        let mut group = zengeld_chart::tag_manager::TagManager::group_from_snapshot(sg_snap);
                        if let Ok(reg) = zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                            for ps in &sg_snap.primitives {
                                if let Some(prim) = reg.from_json(&ps.type_id, &ps.json) {
                                    group.primitives.push(prim);
                                }
                            }
                        }
                        self.panel_app.tag_manager.insert_group_raw(group);
                    }
                    eprintln!("[ChartApp] restored {} sync groups", preset.sync_groups.len());

                    // Bump SyncGroupId counter past any restored group IDs so
                    // future create_group / create_group_auto calls never collide.
                    for sg_snap in &preset.sync_groups {
                        zengeld_chart::tag_manager::SyncGroupId::bump_past(sg_snap.id);
                    }

                    // Seed the global primitive ID counter so future allocations
                    // never collide with any ID loaded from disk.
                    {
                        let max_id = self.panel_app.panel_grid.windows().values()
                            .flat_map(|w| {
                                w.drawing_manager.primitives().iter()
                                    .chain(w.stashed_primitives.iter())
                            })
                            .chain(
                                self.panel_app.tag_manager.groups()
                                    .flat_map(|g| g.primitives.iter())
                            )
                            .map(|p| p.data().id)
                            .max()
                            .unwrap_or(0);
                        zengeld_chart::drawing::seed_primitive_id_counter(max_id);
                    }

                    // Re-ID stashed primitives that collide with group primitives.
                    // Legacy autosave files may have saved both a stash primitive and
                    // its group counterpart with the same numeric ID, causing widget
                    // ID collisions in the Object Tree sidebar.  Assign fresh IDs to
                    // any stash primitive whose ID already exists in group.primitives.
                    {
                        // Phase 1: collect (window_key, group_id, set_of_group_prim_ids)
                        // using immutable borrows only.
                        let collision_info: Vec<(zengeld_chart::ChartId, zengeld_chart::tag_manager::SyncGroupId, std::collections::HashSet<u64>)> =
                            self.panel_app.panel_grid.windows().values()
                                .filter_map(|w| {
                                    let gid = w.group_id?;
                                    let group_ids: std::collections::HashSet<u64> =
                                        self.panel_app.tag_manager.group(gid)
                                            .map(|g| g.primitives.iter().map(|p| p.data().id).collect())
                                            .unwrap_or_default();
                                    Some((w.id, gid, group_ids))
                                })
                                .collect();

                        // Phase 2: mutably iterate windows and re-ID colliding stash entries.
                        for (chart_id, _gid, group_ids) in &collision_info {
                            if let Some(window) = self.panel_app.panel_grid.windows_mut().values_mut()
                                .find(|w| &w.id == chart_id)
                            {
                                let mut reid_count = 0u32;
                                for p in &mut window.stashed_primitives {
                                    if group_ids.contains(&p.data().id) {
                                        p.data_mut().id = zengeld_chart::drawing::alloc_primitive_id();
                                        reid_count += 1;
                                    }
                                }
                                if reid_count > 0 {
                                    eprintln!(
                                        "[ChartApp] Re-IDed {} stash primitive(s) in window {:?} (legacy autosave collision fix)",
                                        reid_count, chart_id
                                    );
                                }
                            }
                        }
                    }

                    // Deduplicate IDs within each group.primitives collection.
                    // Old presets may have group primitives with identical IDs
                    // (from historical clone paths that didn't re-ID).
                    for group in self.panel_app.tag_manager.groups_mut() {
                        let mut seen_ids = std::collections::HashSet::new();
                        let mut reid_count = 0u32;
                        for p in &mut group.primitives {
                            let id = p.data().id;
                            if !seen_ids.insert(id) {
                                p.data_mut().id = zengeld_chart::drawing::alloc_primitive_id();
                                reid_count += 1;
                            }
                        }
                        if reid_count > 0 {
                            eprintln!(
                                "[ChartApp] Re-IDed {} duplicate primitive(s) within group {:?}",
                                reid_count, group.id
                            );
                        }
                    }

                    // Step 5b: If no sync groups exist but windows do, auto-create
                    // a default group so primitives always use the grouped path.
                    if preset.sync_groups.is_empty() && !self.panel_app.panel_grid.windows().is_empty() {
                        // Auto groups get transparent color вЂ” never occupy palette slots
                        let (symbol, timeframe) = self.panel_app.panel_grid.iter_windows().next()
                            .map(|(_, w)| (w.symbol.clone(), w.timeframe.clone()))
                            .unwrap_or_else(|| (
                                "BTCUSDT".to_string(),
                                zengeld_chart::state::Timeframe::h1(),
                            ));
                        let group_id = self.panel_app.tag_manager.create_group_auto([0.0, 0.0, 0.0, 0.0], symbol, timeframe);
                        let leaf_chart_ids: Vec<(zengeld_chart::LeafId, zengeld_chart::ChartId)> =
                            self.panel_app.panel_grid.iter_windows()
                                .map(|(leaf_id, w)| (leaf_id, w.id))
                                .collect();
                        for &(leaf_id, chart_id) in &leaf_chart_ids {
                            let _ = self.panel_app.tag_manager.connect_chart(chart_id, group_id);
                            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                                window.group_id = Some(group_id);
                            }
                        }
                        eprintln!("[ChartApp] Auto-created invisible sync group {:?} for {} orphaned windows", group_id, leaf_chart_ids.len());
                    }

                    // ----------------------------------------------------------------
                    // Step 6: Restore indicators
                    // ----------------------------------------------------------------
                    self.indicator_manager.clear_all();
                    for ind_snap in &preset.indicators {
                        if self.indicator_manager.create_instance_with_id(
                            ind_snap.id,
                            &ind_snap.type_id,
                        ) {
                            // Bind to the persisted window_id via the index — direct
                            // field assignment would leak the instance into the
                            // wrong by_window_id bucket.
                            self.indicator_manager.assign_window(ind_snap.id, ind_snap.window_id);
                            if let Some(inst) = self.indicator_manager.get_instance_mut(ind_snap.id) {
                                inst.name = ind_snap.name.clone();
                                inst.pane = ind_snap.pane;
                                inst.order = ind_snap.order;
                                inst.visible = ind_snap.visible;
                                inst.locked = ind_snap.locked;
                                inst.origin_id = ind_snap.origin_id;
                                inst.signals_enabled = ind_snap.signals_enabled;
                                inst.timeframe_visibility = ind_snap.timeframe_visibility.clone();
                                for (k, v) in &ind_snap.params {
                                    if let Ok(iv) = serde_json::from_value(v.clone()) {
                                        inst.params.insert(k.clone(), iv);
                                    }
                                }
                                // Restore per-output style overrides
                                for (key, out_snap) in &ind_snap.outputs {
                                    let entry = inst.outputs.entry(key.clone())
                                        .or_insert_with(Default::default);
                                    entry.color = out_snap.color.clone();
                                    entry.line_width = out_snap.line_width;
                                    if let Some(vis) = out_snap.visible {
                                        entry.visible = vis;
                                    }
                                }
                                // Restore cached computed values so indicator lines are
                                // visible immediately on tab switch, before Step 6b recalc.
                                if !ind_snap.values.is_empty() {
                                    inst.values = std::sync::Arc::new(ind_snap.values.clone());
                                }
                            }
                        }
                    }
                    eprintln!("[ChartApp] restored {} indicators", preset.indicators.len());

                    // ----------------------------------------------------------------
                    // Step 6b: Immediately recalculate indicators for every window that
                    // already has bars.  Bars were restored in Steps 2-4 so we can feed
                    // them to the indicator engine right now вЂ” no need to wait for the
                    // next BarsLoaded / TradeUpdate round-trip.
                    // Collect (symbol, window_id, bars) first to avoid a split borrow of
                    // `self` (panel_grid vs indicator_manager both live on `self`).
                    // ----------------------------------------------------------------
                    let window_bar_data: Vec<(u64, Vec<zengeld_chart::Bar>)> = self
                        .panel_app
                        .panel_grid
                        .iter_windows()
                        .filter(|(_, w)| !w.bars.is_empty())
                        .map(|(_, w)| (w.id.0, w.bars.clone()))
                        .collect();

                    for (window_id, bars) in window_bar_data {
                        self.indicator_manager.calculate_for_window(window_id, &bars);
                    }
                    eprintln!("[ChartApp] recalculated indicators for all windows with bars");

                    // ----------------------------------------------------------------
                    // Step 6c: Stage sub-pane height ratios, above_main flags, and
                    // ordering for application on the next
                    // sync_sub_panes_from_manager() call.
                    // ----------------------------------------------------------------
                    self.pending_sub_pane_ratios.clear();
                    self.pending_sub_pane_above_main.clear();
                    self.pending_sub_pane_order.clear();
                    for snap in &preset.windows {
                        if !snap.sub_pane_height_ratios.is_empty() {
                            self.pending_sub_pane_ratios
                                .insert(snap.window_id, snap.sub_pane_height_ratios.clone());
                        }
                        if !snap.sub_pane_above_main.is_empty() {
                            self.pending_sub_pane_above_main
                                .insert(snap.window_id, snap.sub_pane_above_main.clone());
                        }
                        if !snap.sub_pane_order.is_empty() {
                            self.pending_sub_pane_order
                                .insert(snap.window_id, snap.sub_pane_order.clone());
                        }
                    }
                    if !self.pending_sub_pane_ratios.is_empty() {
                        eprintln!(
                            "[ChartApp] staged sub-pane height ratios for {} windows",
                            self.pending_sub_pane_ratios.len()
                        );
                    }

                    // ----------------------------------------------------------------
                    // Step 7: Clear stale per-leaf UI state
                    // ----------------------------------------------------------------
                    self.panel_app.indicator_overlay_states.clear();
                    self.panel_app.leaf_color_tags.clear();

                    // Restore leaf_color_tags directly from the preset snapshot.
                    for (&leaf_raw, &color) in &preset.leaf_color_tags {
                        let leaf_id = zengeld_chart::LeafId(leaf_raw);
                        self.panel_app.leaf_color_tags.insert(leaf_id, color);
                    }
                    eprintln!(
                        "[ChartApp] Step7: restored {} leaf_color_tags from snapshot",
                        self.panel_app.leaf_color_tags.len()
                    );

                    // ----------------------------------------------------------------
                    // Step 8: Restore alerts
                    // ----------------------------------------------------------------
                    self.alert_manager.restore(preset.alerts.clone());
                    eprintln!("[ChartApp] restored {} alerts", self.alert_manager.len());

                    // Step 8b: Restore per-slot FreeItem docking layouts.
                    // Pre-populate the store with panel state so the closure
                    // can construct the matching `FreeItem` variants.
                    {
                        // Determine max panel_id across ALL slots to update next_id.
                        let max_panel_id: u64 = preset.slot_leaves.iter()
                            .flat_map(|v| v.iter().map(|l| l.panel_id))
                            .max()
                            .unwrap_or(0);
                        self.panels_store.set_min_next_id(max_panel_id);
                    }

                    // Helper: normalize legacy account_type strings (e.g. "spot") to short labels ("S").
                    let normalize_account_type = |at: &str| -> String {
                        match at.to_lowercase().as_str() {
                            "spot" => "S".to_string(),
                            "margin" => "M".to_string(),
                            "futures" | "futurescross" => "F".to_string(),
                            "futuresiso" | "futuresisolated" => "FI".to_string(),
                            _ => at.to_string(),
                        }
                    };

                    // Helper: convert PersistedSymbolSource в†’ SymbolSource.
                    let convert_source = |src: &zengeld_chart::preset::preset::PersistedSymbolSource| -> zengeld_panels::trading::SymbolSource {
                        match src {
                            zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => zengeld_panels::trading::SymbolSource::HyperFocus,
                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, exchange, account_type } => zengeld_panels::trading::SymbolSource::Fixed {
                                symbol: symbol.clone(),
                                exchange: exchange.clone(),
                                account_type: normalize_account_type(account_type),
                            },
                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { leaf_id } => zengeld_panels::trading::SymbolSource::BoundToChart {
                                leaf_id: *leaf_id,
                            },
                        }
                    };

                    for i in 0..4 {
                        // Pre-insert state into the store keyed by the saved panel_id.
                        for pl in &preset.slot_leaves[i] {
                            let pid = sidebar_content::free_slot::PanelId(pl.panel_id);
                            use zengeld_chart::preset::preset::PersistedFreeItemKind;
                            match &pl.kind {
                                PersistedFreeItemKind::Dom { source, tick_size, levels_displayed, center_price } => {
                                    if !self.panels_store.dom.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::order_flow::dom::DomState::new(symbol, *tick_size);
                                        s.levels_displayed = *levels_displayed;
                                        s.center_price = *center_price;
                                        // Market-data panels no longer carry SymbolSource; TagManager tracks their group.
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared orderbook + trade series if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let ob_handle = self.bridge.subscribe_orderbook(eid, &s.symbol, at);
                                                s.shared_orderbook = Some(ob_handle);
                                                let trade_handle = self.bridge.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = Some(trade_handle);
                                            }
                                        }
                                        self.panels_store.dom.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::Footprint { source, tick_size } => {
                                    if !self.panels_store.footprint.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::order_flow::footprint::FootprintState::new(symbol, *tick_size);
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.bridge.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = Some(handle);
                                            }
                                        }
                                        self.panels_store.footprint.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::VolumeProfile { source, tick_size } => {
                                    if !self.panels_store.volume_profile.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::order_flow::volume_profile::VolumeProfileState::new(symbol, *tick_size);
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.bridge.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = Some(handle);
                                            }
                                        }
                                        self.panels_store.volume_profile.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::LiquidityHeatmap { source, tick_size, snapshot_interval_ms } => {
                                    if !self.panels_store.liquidity_heatmap.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::order_flow::liquidity_heatmap::LiquidityHeatmapState::new(symbol, *tick_size, *snapshot_interval_ms);
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared orderbook series if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.bridge.subscribe_orderbook(eid, &s.symbol, at);
                                                s.shared_orderbook = Some(handle);
                                            }
                                        }
                                        self.panels_store.liquidity_heatmap.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::BigTrades { source } => {
                                    if !self.panels_store.big_trades.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::order_flow::big_trades::BigTradesState::new();
                                        s.symbol = symbol;
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.bridge.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = Some(handle);
                                            }
                                        }
                                        self.panels_store.big_trades.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::L2Tape { source } => {
                                    if !self.panels_store.l2_tape.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::order_flow::l2_tape::L2TapeState::new();
                                        s.symbol = symbol;
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared orderbook series if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.bridge.subscribe_orderbook(eid, &s.symbol, at);
                                                s.shared_orderbook = Some(handle);
                                            }
                                        }
                                        self.panels_store.l2_tape.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::TradeTape { source } => {
                                    if !self.panels_store.trade_tape.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::order_flow::trade_tape::TradeTapeState::new();
                                        s.symbol = symbol;
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        // Subscribe to the shared trade ring if we have a concrete symbol.
                                        if !s.symbol.is_empty() {
                                            if let Some(eid) = digdigdig3::ExchangeId::from_str(&s.exchange) {
                                                let at = crate::account_type_from_label(&s.account_type);
                                                let handle = self.bridge.subscribe_trades(eid, &s.symbol, at);
                                                s.shared_trades = Some(handle);
                                            }
                                        }
                                        self.panels_store.trade_tape.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::OrderEntry { source } => {
                                    if !self.panels_store.order_entry.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::trading::order_entry::OrderEntryState::new(symbol);
                                        s.source = convert_source(source);
                                        if let zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        self.panels_store.order_entry.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::PositionManager => {
                                    if !self.panels_store.position_manager.contains_key(&pid) {
                                        self.panels_store.position_manager.insert(pid, zengeld_panels::trading::trading::position_manager::PositionManagerState::new());
                                    }
                                }
                                PersistedFreeItemKind::TradeLog => {
                                    if !self.panels_store.trade_log.contains_key(&pid) {
                                        self.panels_store.trade_log.insert(pid, zengeld_panels::trading::trading::trade_log::TradeLogState::new());
                                    }
                                }
                                PersistedFreeItemKind::RiskCalculator => {
                                    if !self.panels_store.risk_calculator.contains_key(&pid) {
                                        self.panels_store.risk_calculator.insert(pid, zengeld_panels::trading::trading::risk_calculator::RiskCalculatorState::new());
                                    }
                                }
                                PersistedFreeItemKind::TradingContainer { source, tick_size, market_price } => {
                                    if !self.panels_store.trading_container.contains_key(&pid) {
                                        let symbol = match source {
                                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart { .. } | zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = zengeld_panels::trading::trading::trading_container::TradingContainerState::new(symbol, *tick_size, *market_price);
                                        s.source = convert_source(source);
                                        self.panels_store.trading_container.insert(pid, s);
                                    }
                                }
                            }
                        }

                        // Build a leaf_id в†’ PersistedFreeLeaf map for O(1) lookup
                        // inside the restore closure.  type_id() now returns only the
                        // variant kind (e.g. "free_dom") вЂ” panel identity comes from
                        // the leaf_id passed by restore_tree_with_id.
                        let leaves_by_id: std::collections::HashMap<u64, &zengeld_chart::preset::preset::PersistedFreeLeaf> =
                            preset.slot_leaves[i].iter().map(|l| (l.leaf_id, l)).collect();

                        let mgr = match preset.slot_layouts[i].as_deref() {
                            Some(json) => {
                                match uzor::panels::serialize::LayoutSnapshot::from_json(json) {
                                    Ok(snap) => {
                                        match snap.restore_tree_with_id::<sidebar_content::FreeItem, _>(|leaf_id, _type_id| {
                                            // Look up the persisted leaf descriptor by leaf_id.
                                            // The panel_id and kind are stored there; type_id is
                                            // not used (it only carries the variant kind, no id).
                                            let pl = leaves_by_id.get(&leaf_id)?;
                                            let pid = sidebar_content::free_slot::PanelId(pl.panel_id);
                                            use sidebar_content::free_slot::FreeItem;
                                            let item = match &pl.kind {
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::Dom { .. }               => FreeItem::Dom(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::Footprint { .. }         => FreeItem::Footprint(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::VolumeProfile { .. }     => FreeItem::VolumeProfile(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::LiquidityHeatmap { .. }  => FreeItem::LiquidityHeatmap(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::BigTrades { .. }         => FreeItem::BigTrades(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::L2Tape { .. }            => FreeItem::L2Tape(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::TradeTape { .. }         => FreeItem::TradeTape(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::OrderEntry { .. }        => FreeItem::OrderEntry(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::PositionManager          => FreeItem::PositionManager(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::TradeLog                 => FreeItem::TradeLog(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::RiskCalculator           => FreeItem::RiskCalculator(pid),
                                                zengeld_chart::preset::preset::PersistedFreeItemKind::TradingContainer { .. }  => FreeItem::TradingContainer(pid),
                                            };
                                            Some(item)
                                        }) {
                                            Ok(tree) => {
                                                sidebar_content::SlotDockingManager(
                                                    uzor::layout::DockState::from_tree(tree),
                                                )
                                            }
                                            Err(e) => {
                                                eprintln!("[ChartApp] slot{} restore_tree failed: {} вЂ” empty fallback", i + 1, e);
                                                sidebar_content::SlotDockingManager::new()
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[ChartApp] slot{} from_json failed: {} вЂ” empty fallback", i + 1, e);
                                        sidebar_content::SlotDockingManager::new()
                                    }
                                }
                            }
                            None => sidebar_content::SlotDockingManager::new(),
                        };
                        self.sidebar_state.slot_dockings[i] = mgr;
                    }

                    // Subscribe depth for restored panels that require L2 data
                    // (DOM, L2Tape, LiquidityHeatmap).  For Fixed sources we use the
                    // stored exchange string; for HyperFocus/BoundToChart we fall back
                    // to the active exchange and Spot account type.
                    {
                        // Build a deduplicated list of (exchange_id, symbol, account_type)
                        // tuples that need depth subscriptions.
                        let mut depth_subs: Vec<(digdigdig3::ExchangeId, String, digdigdig3::AccountType)> = Vec::new();

                        let resolve_eid = |exchange_str: &str| -> digdigdig3::ExchangeId {
                            self.exchange_symbols
                                .keys()
                                .find(|e| e.as_str() == exchange_str)
                                .copied()
                                .unwrap_or(self.active_exchange)
                        };

                        // DOM panels subscribe via subscribe_orderbook() in the
                        // PersistedFreeItemKind::Dom block above.  No additional
                        // subscribe_depth() call is needed here for DOM.

                        for state in self.panels_store.l2_tape.values() {
                            if state.symbol.is_empty() { continue; }
                            let eid = if state.exchange.is_empty() {
                                self.active_exchange
                            } else {
                                resolve_eid(&state.exchange)
                            };
                            let at = if state.account_type.is_empty() {
                                digdigdig3::AccountType::Spot
                            } else {
                                crate::account_type_from_label(&state.account_type)
                            };
                            depth_subs.push((eid, state.symbol.clone(), at));
                        }

                        for state in self.panels_store.liquidity_heatmap.values() {
                            if state.symbol.is_empty() { continue; }
                            let eid = if state.exchange.is_empty() {
                                self.active_exchange
                            } else {
                                resolve_eid(&state.exchange)
                            };
                            let at = if state.account_type.is_empty() {
                                digdigdig3::AccountType::Spot
                            } else {
                                crate::account_type_from_label(&state.account_type)
                            };
                            depth_subs.push((eid, state.symbol.clone(), at));
                        }

                        // Deduplicate before subscribing.
                        let mut seen: std::collections::HashSet<(digdigdig3::ExchangeId, String, digdigdig3::AccountType)> = std::collections::HashSet::new();
                        for (eid, sym, at) in depth_subs {
                            if seen.insert((eid, sym.clone(), at)) {
                                self.bridge.subscribe_depth(eid, &sym, at);
                            }
                        }
                    }

                    self.sidebar_data_dirty = true;

                    // ----------------------------------------------------------------
                    // Step 9: Unsubscribe trade actors for symbols that were in the
                    // old preset but are absent from the new one.  Uses targeted
                    // unsubscribe_trades() with a 30-second grace period so symbols
                    // that appear in both old and new presets are not disrupted.
                    // ----------------------------------------------------------------
                    let new_subscriptions: std::collections::HashSet<(digdigdig3::ExchangeId, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_windows()
                        .filter_map(|(_, w)| {
                            digdigdig3::ExchangeId::from_str(&w.exchange)
                                .map(|eid| (eid, w.symbol.clone()))
                        })
                        .collect();

                    for (eid, symbol) in &old_subscriptions {
                        if !new_subscriptions.contains(&(*eid, symbol.clone())) {
                            self.bridge.unsubscribe_trades(*eid, symbol, digdigdig3::AccountType::default());
                            eprintln!("[ChartApp] unsubscribed trades: {}/{}", eid.as_str(), symbol);
                        }
                    }

                    // ----------------------------------------------------------------
                    // Step 10: Request bars for every window in the newly loaded preset.
                    // At startup this is handled by a dedicated post-load loop in lib.rs,
                    // but at runtime (tab-bar switch) only this handler runs, so we must
                    // trigger the fetches here.
                    // ----------------------------------------------------------------
                    let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
                    let mut bars_requested: usize = 0;
                    let window_bar_data: Vec<(String, String, zengeld_chart::state::Timeframe, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_windows()
                        .map(|(_, w)| (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone()))
                        .collect();
                    for (sym, exch, tf, at_label) in &window_bar_data {
                        let eid = digdigdig3::ExchangeId::from_str(exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        if !self.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                            continue;
                        }
                        let at = crate::account_type_from_label(at_label);
                        self.bridge.ensure_connector(eid);
                        self.bridge.request_bars(eid, sym, tf, at, None, Some(bar_count), true);
                        bars_requested += 1;
                    }
                    eprintln!("[ChartApp] LoadPreset: requesting bars for {} windows", bars_requested);

                    eprintln!("[ChartApp] preset '{}' fully restored", preset.name);
                    state_mutated = true;
                } else {
                    eprintln!("[ChartApp] preset '{}' not found in memory", id);
                }

                } // end else (prev_id != id)
            }

            ChartOutEvent::DeletePreset { id } => {
                eprintln!("[ChartApp] delete preset: {}", id);
                // Also remove from open_tabs when deleting.
                self.panel_app.open_tabs.retain(|t| t != &id);
                if self.panel_app.presets.remove(&id).is_some() {
                    self.preset_actions.push(crate::PresetAction::Delete { id: id.clone() });
                    self.persist_profile();
                    eprintln!("[ChartApp] preset '{}' removed from memory", id);
                } else {
                    eprintln!("[ChartApp] preset '{}' not found", id);
                }
            }

            ChartOutEvent::CloseTab { id } => {
                eprintln!("[ChartApp] close tab: {}", id);
                // Autosave the closing tab's state before removing.
                if id == self.panel_app.active_preset_id {
                    self.autosave_snapshot();
                }
                self.panel_app.open_tabs.retain(|t| t != &id);
                // Drop live cache entry for the closed tab.
                self.live_preset_cache.remove(&id);
                // If the closed tab was active, switch to another.
                if self.panel_app.active_preset_id == id {
                    if let Some(next) = self.panel_app.open_tabs.last().cloned() {
                        self.process_chart_out_event(ChartOutEvent::LoadPreset { id: next });
                    } else {
                        // No tabs left вЂ” fall back to __default__.
                        self.panel_app.active_preset_id = "__default__".to_string();
                    }
                }
                self.persist_profile();
            }

            ChartOutEvent::OpenTab { id } => {
                eprintln!("[ChartApp] open tab: {}", id);
                if self.panel_app.open_tabs.contains(&id) {
                    // Preset already open вЂ” just switch to it.
                    self.panel_app.active_preset_id = id.clone();
                    self.process_chart_out_event(ChartOutEvent::LoadPreset { id });
                } else {
                    self.panel_app.open_tabs.push(id.clone());
                    self.process_chart_out_event(ChartOutEvent::LoadPreset { id });
                }
                self.persist_profile();
            }

            ChartOutEvent::RenamePreset { id, new_name } => {
                // Resolve the target preset: empty id means "active preset".
                let target_id = if id.is_empty() {
                    self.panel_app.active_preset_id.clone()
                } else {
                    id
                };

                if target_id == "__default__" || target_id.is_empty() {
                    eprintln!("[ChartApp] no active named preset to rename");
                } else {
                    // Generate a name if none was supplied by the caller.
                    let final_name = if new_name.is_empty() {
                        if let Some(preset) = self.panel_app.presets.get(&target_id) {
                            format!("{} (renamed)", preset.name)
                        } else {
                            eprintln!("[ChartApp] preset '{}' not found for rename", target_id);
                            return;
                        }
                    } else {
                        new_name
                    };

                    if let Some(preset) = self.panel_app.presets.get_mut(&target_id) {
                        eprintln!(
                            "[ChartApp] renamed preset '{}' -> '{}'",
                            preset.name, final_name
                        );
                        preset.name = final_name.clone();
                        self.preset_actions.push(crate::PresetAction::Rename {
                            id: target_id.clone(),
                            new_name: final_name,
                        });
                    } else {
                        eprintln!("[ChartApp] preset '{}' not found for rename", target_id);
                    }
                }
            }

            ChartOutEvent::OpenPresetSaveAs => {
                // Open the preset name input modal in "Save As" mode.
                // Find the next free "Untitled N" number.
                let max_n = self.panel_app.presets.values()
                    .filter_map(|p| p.name.strip_prefix("Untitled "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Untitled {}", max_n + 1);
                self.panel_app.preset_name_input.open_save_as(&default_name, 0);
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::PresetNameInput;
                eprintln!("[ChartApp] opened preset name input (save-as), default='{}'", default_name);
            }

            ChartOutEvent::OpenPresetRename => {
                // Open the preset name input modal in "Rename" mode for the active preset.
                let target_id = self.panel_app.active_preset_id.clone();
                if target_id == "__default__" || target_id.is_empty() {
                    eprintln!("[ChartApp] no active named preset to rename");
                } else if let Some(preset) = self.panel_app.presets.get(&target_id) {
                    let current_name = preset.name.clone();
                    self.panel_app.preset_name_input.open_rename(&target_id, &current_name, 0);
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::PresetNameInput;
                    eprintln!("[ChartApp] opened preset name input (rename), preset='{}'", target_id);
                } else {
                    eprintln!("[ChartApp] preset '{}' not found for rename modal", target_id);
                }
            }

            ChartOutEvent::SaveCurrentPreset => {
                // Re-save snapshot into the currently active preset
                let id = self.panel_app.active_preset_id.clone();
                if id != "__default__" && !id.is_empty() {
                    if let Some(preset) = self.panel_app.presets.get(&id) {
                        let name = preset.name.clone();
                        self.collect_state_into_preset(&id, &name);
                        if let Some(preset) = self.panel_app.presets.get(&id).cloned() {
                            self.preset_actions.push(crate::PresetAction::Upsert(preset));
                        }
                        eprintln!("[ChartApp] saved snapshot into preset '{}' (id={})", name, id);
                    }
                } else {
                    // No active named preset вЂ” treat as Save As
                    self.process_chart_out_event(ChartOutEvent::OpenPresetSaveAs);
                }
            }

            ChartOutEvent::ToggleAutosave => {
                self.panel_app.autosave_enabled = !self.panel_app.autosave_enabled;
                eprintln!("[ChartApp] autosave = {}", self.panel_app.autosave_enabled);
            }

            ChartOutEvent::OpenChartBrowser => {
                self.panel_app.chart_browser.open(0);
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::ChartBrowser;
                eprintln!("[ChartApp] chart browser opened");
            }

            ChartOutEvent::OpenChartBrowserInNewTab => {
                self.panel_app.chart_browser.open(0);
                self.panel_app.chart_browser.open_in_new_tab = true;
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::ChartBrowser;
                eprintln!("[ChartApp] chart browser opened (new-tab mode)");
            }

            ChartOutEvent::NewChart => {
                // Delegate to OpenPresetNewChart so the user is asked for a name first.
                self.process_chart_out_event(ChartOutEvent::OpenPresetNewChart);
            }

            ChartOutEvent::OpenPresetNewChart => {
                // If current chart is __default__, auto-save it first so the name
                // counter sees it and avoids collisions.
                if self.panel_app.active_preset_id == "__default__" {
                    use zengeld_chart::preset::preset::unix_timestamp_parts;
                    let max_n = self.panel_app.presets.values()
                        .filter_map(|p| p.name.strip_prefix("Untitled "))
                        .filter_map(|s| s.parse::<u32>().ok())
                        .max()
                        .unwrap_or(0);
                    let autosave_name = format!("Untitled {}", max_n + 1);
                    let (secs, nanos) = unix_timestamp_parts();
                    let autosave_id = format!("preset_{}_{}", secs, nanos);
                    self.collect_state_into_preset(&autosave_id, &autosave_name);
                    self.panel_app.active_preset_id = autosave_id.clone();
                    eprintln!("[ChartApp] auto-saved __default__ as '{}' (id={})", autosave_name, autosave_id);
                }

                // Now compute the next "Untitled N" вЂ” the auto-saved one is already counted.
                let max_n = self.panel_app.presets.values()
                    .filter_map(|p| p.name.strip_prefix("Untitled "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Untitled {}", max_n + 1);
                self.panel_app.preset_name_input.open_new_chart(&default_name, 0);
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::PresetNameInput;
                eprintln!("[ChartApp] opened preset name input (new-chart), default='{}'", default_name);
            }

            ChartOutEvent::SetTheme(name) => {
                self.panel_app.theme_manager.set_preset(name);
                // Signal the App-level coordinator to propagate this change to all windows.
                self.theme_changed = Some(name.to_string());
                eprintln!("[ChartApp] SetTheme: preset={}", name);
            }

            ChartOutEvent::SetStyle(name) => {
                if let Some(style) = UIStyle::from_name(name) {
                    self.panel_app.theme_manager.current_mut().set_style(style);
                    eprintln!("[ChartApp] SetStyle: style={:?} (from name={})", style, name);
                } else {
                    eprintln!("[ChartApp] SetStyle: unknown style name '{}'", name);
                }
            }

            ChartOutEvent::InternalToggleSyncSymbol => {
                let gid = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.sync_flags.sync_symbol = !group.sync_flags.sync_symbol;
                        newly_enabled = group.sync_flags.sync_symbol;
                        eprintln!("[ChartApp] InternalToggleSyncSymbol: group={:?} sync_symbol={}", gid, group.sync_flags.sync_symbol);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncSymbol: no active group");
                }
                // When sync is turned ON, immediately propagate active leaf's instrument key to peers
                if newly_enabled {
                    if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let (symbol, exchange, account_type) = self.panel_app.panel_grid.active_window()
                            .map(|w| (w.symbol.clone(), w.exchange.clone(), w.account_type.clone()))
                            .unwrap_or_default();
                        if !symbol.is_empty() {
                            self.propagate_symbol_to_sync_group(leaf, &symbol, &exchange, &account_type);
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncTimeframe => {
                let gid = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.sync_flags.sync_timeframe = !group.sync_flags.sync_timeframe;
                        newly_enabled = group.sync_flags.sync_timeframe;
                        eprintln!("[ChartApp] InternalToggleSyncTimeframe: group={:?} sync_timeframe={}", gid, group.sync_flags.sync_timeframe);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncTimeframe: no active group");
                }
                // When sync is turned ON, immediately propagate active leaf's TF to peers
                if newly_enabled {
                    if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let tf = self.panel_app.panel_grid.active_window()
                            .map(|w| w.timeframe.clone());
                        if let Some(tf) = tf {
                            self.propagate_timeframe_to_sync_group(leaf, tf);
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncCrosshair => {
                let gid = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.sync_flags.sync_crosshair = !group.sync_flags.sync_crosshair;
                        newly_enabled = group.sync_flags.sync_crosshair;
                        eprintln!("[ChartApp] InternalToggleSyncCrosshair: group={:?} sync_crosshair={}", gid, group.sync_flags.sync_crosshair);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncCrosshair: no active group");
                }
                // When sync is turned ON, immediately propagate active window's crosshair to peers
                // so that peer price ranges are re-synced and the horizontal line becomes visible.
                if newly_enabled {
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let (timestamp, price, crosshair_visible, pane_index) = self.panel_app
                            .panel_grid
                            .active_window()
                            .map(|w| {
                                let bar_f64 = w.crosshair.bar_f64;
                                let bar_idx = bar_f64 as usize;
                                let timestamp = if bar_idx < w.bars.len() {
                                    w.bars[bar_idx].timestamp
                                } else if w.bars.len() >= 2 {
                                    let last = w.bars[w.bars.len() - 1].timestamp;
                                    let prev = w.bars[w.bars.len() - 2].timestamp;
                                    let interval = last - prev;
                                    let bars_past = bar_f64 - (w.bars.len() - 1) as f64;
                                    last + (bars_past * interval as f64).round() as i64
                                } else if !w.bars.is_empty() {
                                    w.bars[0].timestamp
                                } else {
                                    0
                                };
                                (timestamp, w.crosshair.price, w.crosshair.visible, w.crosshair.pane_index)
                            })
                            .unwrap_or((0, 0.0, false, None));
                        self.propagate_crosshair_to_sync_group(active_leaf, timestamp, price, crosshair_visible, pane_index);
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncViewport => {
                let gid = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.sync_flags.sync_viewport = !group.sync_flags.sync_viewport;
                        newly_enabled = group.sync_flags.sync_viewport;
                        eprintln!("[ChartApp] InternalToggleSyncViewport: group={:?} sync_viewport={}", gid, group.sync_flags.sync_viewport);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncViewport: no active group");
                }
                // When sync is turned ON, immediately propagate active window's viewport to peers
                // so that peer price ranges are re-aligned before crosshair sync resumes.
                if newly_enabled {
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let viewport_state = self.panel_app
                            .panel_grid
                            .active_window()
                            .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                        if let Some((view_start, bar_spacing)) = viewport_state {
                            self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, None);
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncDrawings => {
                let gid = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);
                let chart_id = self.panel_app.panel_grid.active_window().map(|w| w.id);
                if let Some(gid) = gid {
                    // Toggle the flag and capture the new value.
                    let new_sync = if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.sync_flags.sync_drawings = !group.sync_flags.sync_drawings;
                        eprintln!("[TagManager] sync_drawings toggled to {}", group.sync_flags.sync_drawings);
                        group.sync_flags.sync_drawings
                    } else {
                        return;
                    };

                    if new_sync {
                        // OFF в†’ ON: stash window-local primitives (those not in group.primitives).
                        // Collect the set of IDs that live in the group.
                        let group_prim_ids: std::collections::HashSet<u64> = self
                            .panel_app
                            .tag_manager
                            .group(gid)
                            .map(|g| g.primitives.iter().map(|p| p.data().id).collect())
                            .unwrap_or_default();

                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            // Drain all primitives from the manager, partition into local vs synced.
                            let all: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                                std::mem::take(window.drawing_manager.primitives_mut());
                            let mut local: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> = Vec::new();
                            let mut synced: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> = Vec::new();
                            for prim in all {
                                if group_prim_ids.contains(&prim.data().id) {
                                    synced.push(prim);
                                } else {
                                    local.push(prim);
                                }
                            }
                            let stash_count = local.len();
                            window.stashed_primitives.extend(local);
                            // Restore the synced ones back into the manager.
                            window.drawing_manager.add_synced_primitives(synced);
                            eprintln!(
                                "[TagManager] sync_drawings ON: stashed {} window-local primitives",
                                stash_count
                            );
                        }
                    } else {
                        // ON в†’ OFF: restore stashed window-local primitives.
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let restored: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                                std::mem::take(&mut window.stashed_primitives);
                            let count = restored.len();
                            window.drawing_manager.add_synced_primitives(restored);
                            eprintln!(
                                "[TagManager] sync_drawings OFF: restored {} stashed primitives",
                                count
                            );
                        }
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncDrawings: no active group");
                }
                let _ = chart_id; // chart_id reserved for future per-chart filtering
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncIndicators => {
                let gid = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);
                let chart_id = self.panel_app.panel_grid.active_window().map(|w| w.id);
                if let (Some(gid), Some(chart_id)) = (gid, chart_id) {
                    // Toggle the flag and capture the new value.
                    let new_sync = if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.sync_flags.sync_indicators = !group.sync_flags.sync_indicators;
                        eprintln!("[TagManager] sync_indicators toggled to {}", group.sync_flags.sync_indicators);
                        group.sync_flags.sync_indicators
                    } else {
                        return;
                    };

                    let pre_tag_ids: Vec<u64> = self
                        .panel_app
                        .panel_grid
                        .active_window()
                        .map(|w| w.pre_tag_indicator_ids.clone())
                        .unwrap_or_default();

                    if new_sync {
                        // OFF в†’ ON: hide window-local indicators (in pre_tag_indicator_ids
                        // but not yet in the group's indicator_configs).
                        let group_indicator_ids: std::collections::HashSet<u64> = self
                            .panel_app
                            .tag_manager
                            .group(gid)
                            .map(|g| g.indicator_configs.iter().map(|c| c.id).collect())
                            .unwrap_or_default();

                        let to_hide: Vec<u64> = self
                            .indicator_manager
                            .instances_iter()
                            .filter(|i| {
                                i.window_id == Some(chart_id.0)
                                    && pre_tag_ids.contains(&i.id)
                                    && !group_indicator_ids.contains(&i.id)
                            })
                            .map(|i| i.id)
                            .collect();

                        for id in &to_hide {
                            if let Some(inst) = self.indicator_manager.get_instance_mut(*id) {
                                inst.visible = false;
                            }
                        }
                        eprintln!(
                            "[TagManager] sync_indicators ON: hid {} window-local indicators",
                            to_hide.len()
                        );
                    } else {
                        // ON в†’ OFF: unhide window-local (pre_tag) indicators.
                        let to_show: Vec<u64> = self
                            .indicator_manager
                            .instances_iter()
                            .filter(|i| {
                                i.window_id == Some(chart_id.0)
                                    && pre_tag_ids.contains(&i.id)
                                    && !i.visible
                            })
                            .map(|i| i.id)
                            .collect();

                        for id in &to_show {
                            if let Some(inst) = self.indicator_manager.get_instance_mut(*id) {
                                inst.visible = true;
                            }
                        }
                        eprintln!(
                            "[TagManager] sync_indicators OFF: unhid {} window-local indicators",
                            to_show.len()
                        );
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncIndicators: no active group");
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncScaleMode => {
                let gid = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.sync_flags.sync_scale_mode = !group.sync_flags.sync_scale_mode;
                        newly_enabled = group.sync_flags.sync_scale_mode;
                        eprintln!("[ChartApp] InternalToggleSyncScaleMode: group={:?} sync_scale_mode={}", gid, group.sync_flags.sync_scale_mode);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncScaleMode: no active group");
                }
                // When sync is turned ON, immediately push the active window's
                // scale mode to all peers so they land in a consistent state.
                if newly_enabled {
                    if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let mode = self.panel_app.panel_grid.active_window()
                            .map(|w| w.price_scale.scale_mode);
                        if let Some(mode) = mode {
                            self.propagate_scale_mode_to_sync_group(leaf, mode);
                            // Also attempt via viewport path in case sync_viewport is on.
                            let vp = self.panel_app.panel_grid.active_window()
                                .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                            if let Some((vs, bs)) = vp {
                                self.propagate_viewport_to_sync_group(leaf, vs, bs, Some(mode));
                            }
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSplitUntagged => {
                self.split_without_group = !self.split_without_group;
                eprintln!("[ChartApp] split_without_group toggled to {}", self.split_without_group);
            }

            ref other => {
                eprintln!("[ChartApp] unhandled event: {:?}", other);
            }
        }

        if state_mutated {
            self.autosave_snapshot();
        }
    }

}
