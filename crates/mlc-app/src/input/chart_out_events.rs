//! Process ChartOutEvent variants  -  propagates chart-internal events
//! (viewport, timeframe, symbol, indicator, drawing changes) to sync groups,
//! tag manager, autosave, and other downstream consumers.

use crate::ChartApp;
use mlc_core::UIStyle;
use mlc_core::state::modal_state::OpenModal;

impl ChartApp {
    /// Process a `ChartOutEvent`  -  route high-level outcomes back to state.
    pub(crate) fn process_chart_out_event(&mut self, event: mlc_core::events::ChartOutEvent) {
        use mlc_core::events::ChartOutEvent;
        use mlc_core::state::Timeframe;

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
                    // Phase 4 S2.2: capture chart_id BEFORE mutating block.
                    // Phase 4 S2.7.1c: reads via branch_meta (B1 flip).
                    let active_chart_id = self.panel_app.panel_grid.active_container_id();
                    // Capture previous timeframe and active leaf BEFORE changing.
                    let previous_timeframe = active_chart_id
                        .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
                        .cloned();
                    let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                    // Extract whether timeframe actually changed before mutating.
                    // prev_tf_opt: Some(prev) means there was a window; None means no window.
                    let prev_tf_opt = previous_timeframe;
                    let tf_changed = match prev_tf_opt {
                        Some(ref prev_tf) if *prev_tf != tf => {
                            self.push_undo_command(mlc_core::Command::ChangeTimeframe {
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
                    let symbol = active_chart_id
                        .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
                        .map(String::from)
                        .unwrap_or_default();
                    let at_label_tf = active_chart_id
                        .and_then(|cid| self.managers.bubble.account_type_for_chart(cid))
                        .map(String::from)
                        .unwrap_or_else(|| "S".to_string());
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.bars.clear();
                        window.viewport.bar_count = 0;
                        window.viewport.view_start = 0.0;
                    }
                    if let Some(chart_id) = active_chart_id {
                        if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                            if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                                bubble.timeframe = tf.clone();
                            }
                        }
                    }
                    // Fetch bars for new timeframe. Trade stream stays alive  -  trades
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
            ChartOutEvent::ChangeBarMode { mode_id } => {
                eprintln!("[ChartApp] ChangeBarMode: {}", mode_id);
                let new_mode = mlc_core::BarMode::with_defaults(&mode_id);
                let active_chart_id = self.panel_app.panel_grid.active_container_id();
                let active_leaf = self.panel_app.panel_grid.docking().active_leaf();

                // Pre-fill the text field with the mode's default/current param.
                if let Some(param) = new_mode.param_f64() {
                    let param_str = format!("{}", param);
                    let field_id = uzor::WidgetId::from(crate::text_input::BAR_MODE_PARAM);
                    self.input_coordinator.borrow_mut()
                        .text_fields_mut()
                        .set_text(&field_id, &param_str);
                }

                // Clear bars on the active window. Derived bars (Range/Tick/Volume/
                // Footprint) render index-packed like candles, so leave the time
                // axis OFF — if we're switching here from a tick chart, disable it.
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.bars.clear();
                    window.viewport.bar_count = 0;
                    window.viewport.view_start = 0.0;
                    window.viewport.disable_time_axis();
                }

                // Write the new bar_mode into the active chart's bubble.
                if let Some(chart_id) = active_chart_id {
                    if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                        if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                            bubble.bar_mode = new_mode.clone();
                        }
                    }
                }

                // Request new bars via the mode-aware bridge method.
                if let Some(chart_id) = active_chart_id {
                    let symbol = self.managers.bubble.symbol_for_chart(chart_id)
                        .map(String::from).unwrap_or_default();
                    let at_label = self.managers.bubble.account_type_for_chart(chart_id)
                        .map(String::from).unwrap_or_else(|| "S".to_string());
                    let tf = self.managers.bubble.timeframe_for_chart(chart_id)
                        .cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                    if !symbol.is_empty() {
                        let eid_str = self.active_exchange.as_str();
                        if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            let at = crate::account_type_from_label(&at_label);
                            self.bridge.request_bars_for_mode(self.active_exchange, &symbol, &tf, at, &new_mode);
                        }
                    }
                }

                // Propagate to sync group peers.
                if let Some(leaf) = active_leaf {
                    self.propagate_bar_mode_to_sync_group(leaf, new_mode);
                }
                self.sidebar_data_dirty = true;
                state_mutated = true;
            }

            ChartOutEvent::UpdateBarModeParam { param } => {
                eprintln!("[ChartApp] UpdateBarModeParam: {}", param);
                let active_chart_id = self.panel_app.panel_grid.active_container_id();
                let active_leaf = self.panel_app.panel_grid.docking().active_leaf();

                // Read current mode, update the param, write it back.
                let updated_mode = active_chart_id
                    .and_then(|cid| self.managers.bubble.bar_mode_for_chart(cid))
                    .cloned()
                    .unwrap_or_default()
                    .with_param(param);

                if let Some(chart_id) = active_chart_id {
                    if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                        if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                            bubble.bar_mode = updated_mode.clone();
                        }
                    }
                }

                // Clear bars and re-subscribe with the new param.
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.bars.clear();
                    window.viewport.bar_count = 0;
                    window.viewport.view_start = 0.0;
                }
                if let Some(chart_id) = active_chart_id {
                    let symbol = self.managers.bubble.symbol_for_chart(chart_id)
                        .map(String::from).unwrap_or_default();
                    let at_label = self.managers.bubble.account_type_for_chart(chart_id)
                        .map(String::from).unwrap_or_else(|| "S".to_string());
                    let tf = self.managers.bubble.timeframe_for_chart(chart_id)
                        .cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                    if !symbol.is_empty() {
                        let eid_str = self.active_exchange.as_str();
                        if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            let at = crate::account_type_from_label(&at_label);
                            self.bridge.request_bars_for_mode(self.active_exchange, &symbol, &tf, at, &updated_mode);
                        }
                    }
                }

                // Propagate to sync group.
                if let Some(leaf) = active_leaf {
                    self.propagate_bar_mode_to_sync_group(leaf, updated_mode);
                }
                self.sidebar_data_dirty = true;
                state_mutated = true;
            }

            ChartOutEvent::ChangeChartType { chart_type } => {
                eprintln!("[ChartApp] ChangeChartType: {}", chart_type);
                // Map the incoming String to a &'static str used by ChartContainer.
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
                    // Capture chart_panel_id before borrow.
                    let pid = self.panel_app.panel_grid
                        .active_container()
                        .map(|w| w.chart_panel_id);
                    let active_chart_id = self.panel_app.panel_grid.active_container_id();

                    // Render-style and bar-mode share one menu but are mutually
                    // exclusive: picking ANY render style (candles/line/tick/…)
                    // returns the window to TimeBased data. If the window was in a
                    // derived bar_mode (Range/Tick/Volume/Footprint), reset it,
                    // clear the derived bars, and re-request normal klines so the
                    // chart recovers instead of staying stuck on stale derived data.
                    let was_derived = active_chart_id
                        .and_then(|c| self.managers.bubble.bar_mode_for_chart(c))
                        .map(|m| !matches!(m, mlc_core::BarMode::TimeBased))
                        .unwrap_or(false);
                    if was_derived {
                        if let Some(chart_id) = active_chart_id {
                            if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                                if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                                    bubble.bar_mode = mlc_core::BarMode::TimeBased;
                                }
                            }
                        }
                    }

                    // Instrument for any re-subscription below (needs &self.bridge,
                    // so capture before the mutable window borrow).
                    let instrument: Option<(String, mlc_core::state::Timeframe, digdigdig3::AccountType)> =
                        active_chart_id.and_then(|c| {
                            let sym = self.managers.bubble.symbol_for_chart(c)?.to_string();
                            if sym.is_empty() { return None; }
                            let tf = self.managers.bubble.timeframe_for_chart(c)
                                .cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let at_label = self.managers.bubble.account_type_for_chart(c).unwrap_or("S");
                            Some((sym, tf, crate::account_type_from_label(at_label)))
                        });

                    if let Some(chart_panel_id) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                            &mut self.panels_store, chart_panel_id,
                        );
                        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                            window.set_chart_type_with_undo(panel, ct);
                            // Leaving a derived mode: drop the stale derived bars so
                            // the re-requested klines populate a clean series.
                            if was_derived {
                                window.bars.clear();
                                window.viewport.bar_count = 0;
                                window.viewport.view_start = 0.0;
                            }
                        }
                    }

                    // After dropping a derived subscription, re-request normal
                    // TimeBased klines so the chart refills.
                    if was_derived {
                        if let Some((ref sym, ref tf, at)) = instrument {
                            let eid_str = self.active_exchange.as_str();
                            if self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                                self.bridge.request_bars_for_mode(
                                    self.active_exchange, sym, tf, at, &mlc_core::BarMode::TimeBased,
                                );
                            }
                        }
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
            // Chart settings modal  -  opened by gear dropdown "Chart Settings..." item.
            ChartOutEvent::OpenChartSettings => {
                self.panel_app.chart_settings_state.toggle();
                eprintln!("[ChartApp] chart_settings modal toggled via settings dropdown: {}", self.panel_app.chart_settings_state.is_open);
            }

            // User settings modal  -  opened by the chrome gear button.
            ChartOutEvent::OpenUserSettings => {
                self.panel_app.user_settings_state.toggle();
                eprintln!("[ChartApp] user_settings modal toggled: {}", self.panel_app.user_settings_state.is_open);
            }

            // Quick-settings toggles from the settings gear dropdown.
            ChartOutEvent::ToggleGrid => {
                // Collect active panel id  -  drops grid borrow before panels_store borrow.
                let active_pid = self.panel_app.panel_grid.active_container()
                    .map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                        &mut self.panels_store, pid,
                    );
                    let v = !panel.grid_options.vert_lines.visible;
                    panel.grid_options.vert_lines.visible = v;
                    panel.grid_options.horz_lines.visible = v;
                    Some((v, v))
                } else { None };
                if let Some((v, h)) = new_val {
                    // Broadcast to all panel states.
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers()
                        .values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                            &mut self.panels_store, pid,
                        );
                        panel.grid_options.vert_lines.visible = v;
                        panel.grid_options.horz_lines.visible = h;
                    }
                }
                eprintln!("[ChartApp] grid toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleCrosshair => {
                let new_val = if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    w.toggle_crosshair();
                    Some(w.crosshair.enabled)
                } else { None };
                if let Some(val) = new_val {
                    for w in self.panel_app.panel_grid.containers_mut().values_mut() {
                        w.crosshair.enabled = val;
                    }
                }
                eprintln!("[ChartApp] crosshair toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegend => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.visible = !panel.legend.visible;
                    eprintln!("[ChartApp] legend toggled -> {}", panel.legend.visible);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleWatermark => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    match &mut panel.watermark {
                        Some(wm) => { wm.visible = !wm.visible; Some(wm.visible) }
                        None => { panel.watermark = Some(mlc_core::Watermark::default()); Some(true) }
                    }
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        if let Some(ref mut wm) = panel.watermark { wm.visible = val; }
                    }
                }
                eprintln!("[ChartApp] watermark toggled (all windows)");
                state_mutated = true;
            }

            // Left panel  -  no sidebar in standalone mode.
            ChartOutEvent::ToggleLeftPanel => {
                eprintln!("[ChartApp] Not available in standalone mode: {:?}", event);
            }

            // Right sidebar panels  -  handled via SidebarState.
            // toggle_right_panel returns Option<(bool, f64)>:
            //   Some((true,  w))  ->  sidebar opened   ->  compensate viewport rightward
            //   Some((false, w))  ->  sidebar closed   ->  compensate viewport leftward
            //   None              ->  panel switched   ->  no viewport change needed
            ChartOutEvent::ToggleWatchlist => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Watchlist,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Watchlist panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAlerts => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Alerts,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Alerts panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleObjectTree => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::ObjectTree,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Object Tree panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleSignals => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Signals,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Signals panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleConnectors => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Connectors,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Connectors panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::TogglePerformance => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Performance,
                );
                if let Some((opening, _width)) = result {
                    eprintln!("[ChartApp] Performance panel: {}", if opening { "opened" } else { "closed" });
                }
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            ChartOutEvent::ToggleAgents => {
                let result = self.sidebar_state.toggle_right_panel(
                    mlc_sidebar_content::state::RightSidebarPanel::Agents,
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
                let panel = mlc_sidebar_content::state::RightSidebarPanel::from_slot_index(idx);
                let _ = self.sidebar_state.toggle_right_panel(panel);
                self.sidebar_data_dirty = true;
                self.persist_profile();
            }
            // Split layout events  -  wire to ChartPanelGrid split system.
            // After each split all new leaves receive a shared sync color tag so
            // that symbol / timeframe changes propagate across the split group.
            ChartOutEvent::InternalSplitHorizontal => { self.do_split(mlc_core::SplitKind::SplitRight); state_mutated = true; }
            ChartOutEvent::InternalSplitVertical => { self.do_split(mlc_core::SplitKind::SplitBottom); state_mutated = true; }
            ChartOutEvent::InternalSplitGrid2x2 => { self.do_split(mlc_core::SplitKind::Grid2x2); state_mutated = true; }
            ChartOutEvent::InternalSplit2Left1Right => { self.do_split(mlc_core::SplitKind::TwoLeftOneRight); state_mutated = true; }
            ChartOutEvent::InternalSplit1Left2Right => { self.do_split(mlc_core::SplitKind::OneLeftTwoRight); state_mutated = true; }
            ChartOutEvent::InternalSplit2Top1Bottom => { self.do_split(mlc_core::SplitKind::TwoTopOneBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit1Top2Bottom => { self.do_split(mlc_core::SplitKind::OneTopTwoBottom); state_mutated = true; }
            ChartOutEvent::InternalSplit3Columns => { self.do_split(mlc_core::SplitKind::ThreeColumns); state_mutated = true; }
            ChartOutEvent::InternalSplit3Rows => { self.do_split(mlc_core::SplitKind::ThreeRows); state_mutated = true; }
            ChartOutEvent::InternalSplit1Big3Small => { self.do_split(mlc_core::SplitKind::OneBig3Small); state_mutated = true; }
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
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.grid_options.vert_lines.visible = !panel.grid_options.vert_lines.visible;
                    Some(panel.grid_options.vert_lines.visible)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid)
                            .grid_options.vert_lines.visible = val;
                    }
                }
                eprintln!("[ChartApp] grid vertical toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleGridHorizontal => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.grid_options.horz_lines.visible = !panel.grid_options.horz_lines.visible;
                    Some(panel.grid_options.horz_lines.visible)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid)
                            .grid_options.horz_lines.visible = val;
                    }
                }
                eprintln!("[ChartApp] grid horizontal toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendOHLC => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.show_ohlc = !panel.legend.show_ohlc;
                    eprintln!("[ChartApp] legend OHLC toggled -> {}", panel.legend.show_ohlc);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendChange => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.show_change = !panel.legend.show_change;
                    eprintln!("[ChartApp] legend change toggled -> {}", panel.legend.show_change);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleLegendPercent => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.legend.show_percent = !panel.legend.show_percent;
                    eprintln!("[ChartApp] legend percent toggled -> {}", panel.legend.show_percent);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleTooltip => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.tooltip.visible = !panel.tooltip.visible;
                    Some(panel.tooltip.visible)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid).tooltip.visible = val;
                    }
                }
                eprintln!("[ChartApp] tooltip toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::ToggleTooltipFollow => {
                let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                let new_val = if let Some(pid) = active_pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.tooltip.follow_cursor = !panel.tooltip.follow_cursor;
                    Some(panel.tooltip.follow_cursor)
                } else { None };
                if let Some(val) = new_val {
                    let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                    for pid in all_pids {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid).tooltip.follow_cursor = val;
                    }
                }
                eprintln!("[ChartApp] tooltip follow toggled (all windows)");
                state_mutated = true;
            }
            ChartOutEvent::SetWatermarkText(text) => {
                let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                for pid in all_pids {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    match &mut panel.watermark {
                        Some(wm) => wm.set_text(text.to_string()),
                        None => { let mut wm = mlc_core::Watermark::default(); wm.set_text(text.to_string()); panel.watermark = Some(wm); }
                    }
                }
                eprintln!("[ChartApp] watermark text set to {} (all windows)", text);
                state_mutated = true;
            }
            ChartOutEvent::SetWatermarkPosition(pos) => {
                use mlc_core::{HorzAlign, VertAlign};
                let (horz, vert) = match pos.as_ref() {
                    "center"       => (HorzAlign::Center, VertAlign::Center),
                    "bottom_left"  => (HorzAlign::Left,   VertAlign::Bottom),
                    "bottom_right" => (HorzAlign::Right,  VertAlign::Bottom),
                    "top_left"     => (HorzAlign::Left,   VertAlign::Top),
                    "top_right"    => (HorzAlign::Right,  VertAlign::Top),
                    _              => (HorzAlign::Left,   VertAlign::Bottom),
                };
                let all_pids: Vec<_> = self.panel_app.panel_grid.containers().values().map(|w| w.chart_panel_id).collect();
                for pid in all_pids {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    match &mut panel.watermark {
                        Some(wm) => { wm.horz_align = horz; wm.vert_align = vert; }
                        None => { panel.watermark = Some(mlc_core::Watermark { horz_align: horz, vert_align: vert, ..mlc_core::Watermark::default() }); }
                    }
                }
                eprintln!("[ChartApp] watermark position set to {} (all windows)", pos);
                state_mutated = true;
            }
            // === Presets (in-memory HashMap) ===
            ChartOutEvent::SavePreset { name } => {
                // "Save As"  -  create a new named preset from current state.
                use mlc_core::preset::preset::unix_timestamp_parts;
                let (secs, nanos) = unix_timestamp_parts();
                let id = format!("preset_{}_{}", secs, nanos);
                eprintln!("[ChartApp] save-as preset '{}'  ->  id={}", name, id);
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
                        id, p.windows.len(), p.bubbles.len(), p.indicators.len()
                    );
                }
            }

            ChartOutEvent::LoadPreset { id } => {
                eprintln!("[ChartApp] load preset: {}", id);
                let prev_id = self.panel_app.active_preset_id.clone();

                // Is the CURRENT live state actually a materialized preset?
                // At startup active_preset_id is already set from the profile
                // BEFORE the restore runs, so a LoadPreset for that same id
                // must NOT be treated as a no-op — the windows have no bubble
                // membership yet. Treating it as a no-op both skipped the real
                // restore (eternal "Loading...", empty toolbar) and let the
                // trailing autosave bake the EMPTY state into the preset file.
                let already_materialized = self
                    .panel_app
                    .panel_grid
                    .iter_containers()
                    .any(|(_, w)| self.managers.bubble.bubble_for_chart(w.id).is_some());

                // Same tab AND live state real  -  no-op (and nothing changed,
                // so no autosave either).
                if prev_id == id && already_materialized {
                    eprintln!("[ChartApp] LoadPreset: already active, skipping");
                } else {

                // Save the outgoing preset (with bars) before switching away —
                // but never snapshot a non-materialized placeholder state over
                // a real preset file.
                if !prev_id.is_empty() && prev_id != "__default__" && already_materialized {
                    self.autosave_snapshot();
                }

                // Park outgoing state into live cache — only a materialized
                // state. Parking the startup placeholder under the SAME id we
                // are about to load would make unpark return the placeholder
                // and skip the cold restore entirely.
                if !prev_id.is_empty() && already_materialized {
                    self.park_active_preset(&prev_id);
                }

                self.panel_app.active_preset_id = id.clone();
                self.persist_profile();

                // Warm path: live cache hit  -  zero-flicker switch.
                if self.unpark_preset(&id) {
                    eprintln!("[ChartApp] LoadPreset '{}': live cache hit  -  zero-flicker switch", id);

                    // Cure: request 300 fresh bars for every window so gaps are
                    // healed, stale data is refreshed, and bar store is updated.
                    let bar_count = self.panel_app.user_manager.profile.bar_count as usize;
                    // Phase 4 S2.7.1c: read via branch_meta (B6 tuple-collect flip).
                    let window_bar_data: Vec<(String, String, mlc_core::state::Timeframe, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .map(|(_, w)| {
                            let cid = w.id;
                            let sym = self.managers.bubble.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let exch = self.managers.bubble.exchange_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let tf = self.managers.bubble.timeframe_for_chart(cid)
                                .cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let at = self.managers.bubble.account_type_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            (sym, exch, tf, at)
                        })
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
                } else if let Some(mut preset) = self.panel_app.presets.get(&id).cloned() {
                    // Recovery: preset files written by the startup-autosave bug
                    // carry empty symbol/exchange. Resurrect them with the
                    // standard default instead of an eternally-loading empty
                    // chart; the next (now-guarded) autosave heals the file.
                    for w in preset.windows.iter_mut() {
                        if w.symbol.is_empty() {
                            w.symbol = "BTCUSDT".to_string();
                            w.exchange = "binance".to_string();
                        }
                    }
                    for b in preset.bubbles.iter_mut() {
                        if b.symbol.is_empty() {
                            b.symbol = "BTCUSDT".to_string();
                            b.exchange = "binance".to_string();
                        }
                    }
                    eprintln!(
                        "[ChartApp] applying preset '{}': {} windows, {} groups, {} indicators",
                        preset.name, preset.windows.len(), preset.bubbles.len(), preset.indicators.len()
                    );

                    // ----------------------------------------------------------------
                    // Collect current (exchange_id, symbol) pairs before switching.
                    // Used after the preset is loaded to unsubscribe stale trade actors.
                    // ----------------------------------------------------------------
                    // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                    let old_subscriptions: std::collections::HashSet<(digdigdig3::ExchangeId, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .filter_map(|(_, w)| {
                            let cid = w.id;
                            let exch = self.managers.bubble.exchange_for_chart(cid)?;
                            let sym = self.managers.bubble.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            digdigdig3::ExchangeId::from_str(exch).map(|eid| (eid, sym))
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
                                let mut new_containers = std::collections::HashMap::new();
                                let mut new_leaf_to_chart = std::collections::HashMap::new();

                                for snap in &preset.windows {
                                    let leaf_id = mlc_core::LeafId(snap.leaf_id);
                                    let chart_id = mlc_core::state::chart_container::ChartId(snap.window_id);

                                    let mut window = mlc_core::state::chart_container::ChartContainer::with_id(
                                        chart_id,
                                        &snap.symbol,
                                        snap.timeframe.clone(),
                                    );

                                    // Build a data_provider for this window's own exchange and account_type.
                                    let snap_exchange_id = digdigdig3::ExchangeId::from_str(&snap.exchange)
                                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                                    let snap_at = crate::account_type_from_label(&snap.account_type);
                                    window.data_provider = std::sync::Arc::new(
                                        mlc_live_data::LiveDataProvider::new(
                                            snap_exchange_id,
                                            snap_exchange_id.as_str().to_string(),
                                            snap_at,
                                            std::sync::Arc::clone(&self.bridge),
                                        ),
                                    );

                                    // Apply all snapshot fields
                                    window.viewport = snap.viewport.clone();
                                    window.price_scale = snap.price_scale.clone();
                                    window.scale_settings = snap.scale_settings.clone();

                                    // Phase 4 C+S2.8: restore display/series fields + drawing_manager into panel state.
                                    {
                                        let static_ct: &'static str = match snap.chart_type.as_str() {
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
                                        let mut panel_state = mlc_core::ChartPanelState::new(window.chart_panel_id);
                                        panel_state.grid_options = snap.grid_options.clone();
                                        panel_state.crosshair_options = snap.crosshair_options.clone();
                                        panel_state.legend = snap.legend.clone();
                                        panel_state.watermark = snap.watermark.clone();
                                        panel_state.tooltip = snap.tooltip.clone();
                                        panel_state.show_candles = snap.show_candles;
                                        panel_state.show_bars = snap.show_bars;
                                        panel_state.show_hollow_candles = snap.show_hollow_candles;
                                        panel_state.show_heikin_ashi = snap.show_heikin_ashi;
                                        panel_state.show_line = snap.show_line;
                                        panel_state.show_step_line = snap.show_step_line;
                                        panel_state.show_line_markers = snap.show_line_markers;
                                        panel_state.show_area = snap.show_area;
                                        panel_state.show_hlc_area = snap.show_hlc_area;
                                        panel_state.show_histogram = snap.show_histogram;
                                        panel_state.show_columns = snap.show_columns;
                                        panel_state.show_baseline = snap.show_baseline;
                                        panel_state.chart_type = static_ct;
                                        // S2.8: restore drawings into panel_state.drawing_manager
                                        panel_state.drawing_manager.set_current_symbol_key(
                                            &snap.symbol, &snap.exchange, &snap.account_type,
                                        );
                                        panel_state.drawing_manager.clear_all_primitives();
                                        if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                            for prim_snap in &snap.drawings.primitives {
                                                if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                    panel_state.drawing_manager.add_external_primitive(prim);
                                                }
                                            }
                                        }
                                        panel_state.drawing_manager.migrate_all_legacy_timestamps();
                                        self.panels_store.chart_panel.insert(window.chart_panel_id, panel_state);
                                    }

                                    // Restore command history
                                    if let Some(history) = &snap.command_history {
                                        window.command_history = history.clone();
                                    }

                                    // Create a fresh auto-bubble for this chart and restore
                                    // stashed state (pre-tag primitives, pre_tag_indicator_ids,
                                    // stashed command_history) into it.
                                    let auto_bid = self.managers.bubble.create_group_auto(
                                        [0.0, 0.0, 0.0, 0.0],
                                        snap.symbol.clone(),
                                        snap.timeframe.clone(),
                                    );
                                    window.auto_bubble_id = auto_bid;
                                    // current_bubble_id is set later when we attach to the
                                    // color-bubble (Step 5); default to auto_bid for now.
                                    window.current_bubble_id = auto_bid;
                                    let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, auto_bid);

                                    // Restore stashed primitives into auto_bubble.primitives.
                                    if let Some(auto_bub) = self.managers.bubble.bubble_mut(auto_bid) {
                                        auto_bub.primitives.clear();
                                        if !snap.stashed_primitives.is_empty() {
                                            if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                                for prim_snap in &snap.stashed_primitives {
                                                    if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                        auto_bub.primitives.push(prim);
                                                    }
                                                }
                                            }
                                        }
                                        // Restore pre_tag_indicator_ids into auto_bubble.
                                        auto_bub.pre_tag_indicator_ids = snap.pre_tag_indicator_ids.clone();
                                        // Restore stashed command_history into auto_bubble.
                                        if let Some(h) = snap.stashed_command_history.clone() {
                                            auto_bub.command_history = h;
                                        }
                                    }

                                    // Restore per-symbol drawing cache
                                    window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                    // Restore stashed scale mode (pre-join A/M/F, restored on desync).
                                    window.stashed_scale_mode = snap.stashed_scale_mode;

                                    // Restore bar_spacing (user's zoom level). chart_width is NOT restored  - 
                                    // it comes from layout on the first frame; view_start is NOT restored  - 
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
                                    // Cold preset restore: stale cached bars arrive via BarsLoaded
                                    // first; fresh exchange data arrives later via BackfillComplete.
                                    // Snap to end only after the fresh data lands.
                                    window.needs_snap_after_backfill = true;

                                    eprintln!(
                                        "[ChartApp] built window {} -> {}/{} ({} drawings)",
                                        snap.window_id, snap.symbol, snap.timeframe.name,
                                        snap.drawings.primitives.len()
                                    );

                                    if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                                        if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                                            bubble.symbol = snap.symbol.clone();
                                            bubble.timeframe = snap.timeframe.clone();
                                            bubble.exchange = snap.exchange.clone();
                                            bubble.account_type = snap.account_type.clone();
                                        }
                                    }
                                    // Phase 4 C: panel state already written above.

                                    new_containers.insert(chart_id, window);
                                    new_leaf_to_chart.insert(leaf_id, chart_id);
                                }

                                // Bump global ChartId counter past any restored window IDs.
                                for snap in &preset.windows {
                                    mlc_core::bump_chart_id_past(snap.window_id);
                                }

                                // Step 3: Restore the docking tree.
                                //
                                // The factory emits a placeholder Panel::Chart(ChartId(0))
                                // for every leaf; each leaf is then patched with the real
                                // ChartId by walking `new_leaf_to_chart`.
                                match layout_snap.restore_tree(|_type_id| {
                                    Some(mlc_core::state::Panel::Chart(
                                        mlc_core::state::ChartId(0),
                                    ))
                                }) {
                                    Ok(mut restored_tree) => {
                                        for (&leaf_id, &chart_id) in &new_leaf_to_chart {
                                            if let Some(leaf) = restored_tree.leaf_mut(leaf_id) {
                                                if let Some(panel) = leaf.active_panel_mut() {
                                                    *panel = mlc_core::state::Panel::Chart(chart_id);
                                                }
                                            }
                                        }
                                        // Step 4: Replace panel grid state
                                        self.panel_app.panel_grid.replace_docking(restored_tree);
                                        self.panel_app.panel_grid.replace_containers(new_containers);
                                        self.panel_app.panel_grid.replace_leaf_to_chart(new_leaf_to_chart);
                                        eprintln!("[ChartApp] layout topology restored");
                                        true
                                    }
                                    Err(e) => {
                                        // Salvage: build a flat docking tree containing one
                                        // leaf per built window. Iterate in deterministic order
                                        // (sorted by original LeafId so visual order is stable).
                                        let mut sorted_entries: Vec<(uzor::panels::LeafId, mlc_core::state::ChartId)> =
                                            new_leaf_to_chart.into_iter().collect();
                                        sorted_entries.sort_by_key(|(leaf_id, _)| leaf_id.0);

                                        let n = sorted_entries.len();
                                        let mut flat_tree: uzor::panels::DockingTree<mlc_core::state::Panel> =
                                            uzor::panels::DockingTree::new();
                                        let mut fresh_leaf_to_chart: std::collections::HashMap<uzor::panels::LeafId, mlc_core::state::ChartId> =
                                            std::collections::HashMap::new();
                                        let mut first_leaf: Option<uzor::panels::LeafId> = None;

                                        for (_orig_leaf_id, chart_id) in sorted_entries {
                                            let panel = mlc_core::state::Panel::Chart(chart_id);
                                            let new_lid = flat_tree.add_leaf(panel);
                                            fresh_leaf_to_chart.insert(new_lid, chart_id);
                                            if first_leaf.is_none() {
                                                first_leaf = Some(new_lid);
                                            }
                                        }

                                        eprintln!("[ChartApp] restore_tree failed: {} - salvaged flat layout with {} windows", e, n);

                                        self.panel_app.panel_grid.replace_docking(flat_tree);
                                        self.panel_app.panel_grid.replace_containers(new_containers);
                                        self.panel_app.panel_grid.replace_leaf_to_chart(fresh_leaf_to_chart);
                                        if let Some(lid) = first_leaf {
                                            self.panel_app.panel_grid.set_active_leaf(lid);
                                        }
                                        true
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[ChartApp] layout deserialize failed: {} - falling back to patch", e);
                                false
                            }
                        }
                    } else {
                        eprintln!("[ChartApp] preset has no layout snapshot - falling back to patch");
                        false
                    };

                    // ----------------------------------------------------------------
                    // Fallback: patch existing windows in-place (old presets)
                    // ----------------------------------------------------------------
                    if !layout_restored {
                        // Collect (chart_panel_id, display/series state) from windows
                        // before the panel_grid mut borrow; apply to panels_store after.
                        let mut panel_patches: Vec<(mlc_core::state::ChartPanelId, mlc_core::ChartPanelState)> = Vec::new();
                        {
                            let windows = self.panel_app.panel_grid.containers_mut();
                            for snap in &preset.windows {
                                let chart_id = mlc_core::state::chart_container::ChartId(snap.window_id);
                                if let Some(window) = windows.get_mut(&chart_id) {
                                    // Rebuild data_provider for this window's (possibly new) exchange.
                                    let patch_exchange_id = digdigdig3::ExchangeId::from_str(&snap.exchange)
                                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                                    let patch_at = crate::account_type_from_label(&snap.account_type);
                                    window.data_provider = std::sync::Arc::new(
                                        mlc_live_data::LiveDataProvider::new(
                                            patch_exchange_id,
                                            patch_exchange_id.as_str().to_string(),
                                            patch_at,
                                            std::sync::Arc::clone(&self.bridge),
                                        ),
                                    );
                                    window.viewport = snap.viewport.clone();
                                    window.price_scale = snap.price_scale.clone();
                                    window.scale_settings = snap.scale_settings.clone();

                                    // Restore command history
                                    if let Some(history) = &snap.command_history {
                                        window.command_history = history.clone();
                                    }

                                    // Pre-auto-bubble presets serialised no auto_bubble_id —
                                    // mint one now so stashed primitives have a home.
                                    let existing_auto_bid = window.auto_bubble_id;
                                    let auto_bid_for_window = if existing_auto_bid == mlc_core::tag_manager::BubbleId(0) {
                                        let fresh = self.managers.bubble.create_group_auto(
                                            [0.0, 0.0, 0.0, 0.0],
                                            snap.symbol.clone(),
                                            snap.timeframe.clone(),
                                        );
                                        window.auto_bubble_id = fresh;
                                        window.current_bubble_id = fresh;
                                        let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, fresh);
                                        fresh
                                    } else {
                                        existing_auto_bid
                                    };
                                    if let Some(auto_bub) = self.managers.bubble.bubble_mut(auto_bid_for_window) {
                                        auto_bub.primitives.clear();
                                        if !snap.stashed_primitives.is_empty() {
                                            if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                                for prim_snap in &snap.stashed_primitives {
                                                    if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                        auto_bub.primitives.push(prim);
                                                    }
                                                }
                                            }
                                        }
                                        auto_bub.pre_tag_indicator_ids = snap.pre_tag_indicator_ids.clone();
                                        if let Some(h) = snap.stashed_command_history.clone() {
                                            auto_bub.command_history = h;
                                        }
                                    }

                                    // Restore per-symbol drawing cache
                                    window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                    // Restore stashed scale mode (pre-join A/M/F, restored on desync).
                                    window.stashed_scale_mode = snap.stashed_scale_mode;

                                    // Restore bar_spacing (user's zoom level).
                                    if snap.viewport.bar_spacing > 0.0 {
                                        window.viewport.bar_spacing = snap.viewport.bar_spacing;
                                    }
                                    window.restore_scale_mode = Some(snap.price_scale.scale_mode);

                                    // Bars arrive asynchronously via BarsLoaded.
                                    window.pending_symbol_load = true;
                                    // Cold preset restore: stale cached bars arrive via BarsLoaded
                                    // first; fresh exchange data arrives later via BackfillComplete.
                                    // Snap to end only after the fresh data lands.
                                    window.needs_snap_after_backfill = true;

                                    // Collect panel state for post-borrow write to panels_store.
                                    let static_ct: &'static str = match snap.chart_type.as_str() {
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
                                    let mut ps = mlc_core::ChartPanelState::new(window.chart_panel_id);
                                    ps.grid_options = snap.grid_options.clone();
                                    ps.crosshair_options = snap.crosshair_options.clone();
                                    ps.legend = snap.legend.clone();
                                    ps.watermark = snap.watermark.clone();
                                    ps.tooltip = snap.tooltip.clone();
                                    ps.show_candles = snap.show_candles;
                                    ps.show_bars = snap.show_bars;
                                    ps.show_hollow_candles = snap.show_hollow_candles;
                                    ps.show_heikin_ashi = snap.show_heikin_ashi;
                                    ps.show_line = snap.show_line;
                                    ps.show_step_line = snap.show_step_line;
                                    ps.show_line_markers = snap.show_line_markers;
                                    ps.show_area = snap.show_area;
                                    ps.show_hlc_area = snap.show_hlc_area;
                                    ps.show_histogram = snap.show_histogram;
                                    ps.show_columns = snap.show_columns;
                                    ps.show_baseline = snap.show_baseline;
                                    ps.chart_type = static_ct;
                                    // S2.8: restore drawings into ps.drawing_manager
                                    ps.drawing_manager.set_current_symbol_key(
                                        &snap.symbol, &snap.exchange, &snap.account_type,
                                    );
                                    ps.drawing_manager.clear_all_primitives();
                                    if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                                        for prim_snap in &snap.drawings.primitives {
                                            if let Some(prim) = reg.from_json(&prim_snap.type_id, &prim_snap.json) {
                                                ps.drawing_manager.add_external_primitive(prim);
                                            }
                                        }
                                    }
                                    ps.drawing_manager.migrate_all_legacy_timestamps();
                                    panel_patches.push((window.chart_panel_id, ps));

                                    eprintln!("[ChartApp] patched window {}  ->  {}/{} ({} drawings)",
                                        snap.window_id, snap.symbol, snap.timeframe.name,
                                        snap.drawings.primitives.len());
                                }
                            }
                        } // windows (panel_grid borrow) dropped here
                        // Restore instrument fields into the bubble for each window.
                        for snap in &preset.windows {
                            let chart_id = mlc_core::state::chart_container::ChartId(snap.window_id);
                            if let Some(bid) = self.managers.bubble.bubble_for_chart(chart_id) {
                                if let Some(bubble) = self.managers.bubble.bubble_mut(bid) {
                                    bubble.symbol = snap.symbol.clone();
                                    bubble.timeframe = snap.timeframe.clone();
                                    bubble.exchange = snap.exchange.clone();
                                    bubble.account_type = snap.account_type.clone();
                                }
                            }
                            // Note: pre_tag_indicator_ids and stashed_command_history are
                            // already written into auto_bubble above; no separate map needed.
                        }
                        // Phase 4 C: write panel states (panel_grid borrow is dropped above).
                        for (chart_panel_id, ps) in panel_patches {
                            self.panels_store.chart_panel.insert(chart_panel_id, ps);
                        }
                    }

                    // ----------------------------------------------------------------
                    // Step 5: Restore TagManager (sync groups)
                    // ----------------------------------------------------------------
                    self.managers.bubble.clear();
                    for sg_snap in &preset.bubbles {
                        let mut group = mlc_core::tag_manager::BubbleStore::group_from_snapshot(sg_snap);
                        if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                            for ps in &sg_snap.primitives {
                                if let Some(mut prim) = reg.from_json(&ps.type_id, &ps.json) {
                                    // Backfill symbol/exchange/account_type for primitives
                                    // saved before these fields existed (serde(default) gives
                                    // empty string). Use the group's shared instrument as
                                    // source of truth  -  prevents them falling into Memory on load.
                                    if prim.data().symbol.is_empty() && !sg_snap.symbol.is_empty() {
                                        prim.data_mut().symbol = sg_snap.symbol.clone();
                                    }
                                    if prim.data().exchange.is_empty() && !sg_snap.exchange.is_empty() {
                                        prim.data_mut().exchange = sg_snap.exchange.clone();
                                    }
                                    if prim.data().account_type.is_empty() && !sg_snap.account_type.is_empty() {
                                        prim.data_mut().account_type = sg_snap.account_type.clone();
                                    }
                                    group.primitives.push(prim);
                                }
                            }
                        }
                        self.managers.bubble.insert_group_raw(group);
                        // Restore per-group synced_panels list. Each panel id stored
                        // in synced_panel_ids was Synced (= follows active chart) at
                        // save time; re-register via set_synced so that next active
                        // chart change re-attaches the panel correctly.
                        let gid = mlc_core::tag_manager::BubbleId(sg_snap.id);
                        for &pid in &sg_snap.synced_panel_ids {
                            let member = mlc_core::tag_manager::SyncMemberId::Panel(pid);
                            self.managers.bubble.set_synced(member, gid);
                        }
                    }
                    eprintln!("[ChartApp] restored {} sync groups", preset.bubbles.len());

                    // Rebind every window's DM to its group's shared style store now
                    // that all groups are fully reconstructed.  Windows whose group_id
                    // is set but the group has no entry yet keep their own private Arc
                    // (fresh HashMap) as a safe fallback.
                    {
                        // Collect (chart_id, group_id) pairs without holding borrow.
                        let rebind_info: Vec<(mlc_core::ChartId, mlc_core::tag_manager::BubbleId)> =
                            self.panel_app.panel_grid.containers().keys()
                                .copied()
                                .filter_map(|cid| {
                                    self.managers.bubble.bubble_for_chart(cid)
                                        .map(|gid| (cid, gid))
                                })
                                .collect();
                        for (chart_id, group_id) in rebind_info {
                            if let Some(style_arc) = self.managers.bubble
                                .bubble(group_id)
                                .map(|g| g.last_used_style.clone())
                            {
                                if let Some(window) = self.panel_app.panel_grid.containers()
                                    .get(&chart_id)
                                {
                                    let pid = window.chart_panel_id;
                                    if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                                        panel.drawing_manager.bind_style_store(style_arc);
                                    }
                                }
                            }
                        }
                    }

                    // Bump BubbleId counter past any restored bubble IDs so
                    // future create_group / create_group_auto calls never collide.
                    for sg_snap in &preset.bubbles {
                        mlc_core::tag_manager::BubbleId::bump_past(sg_snap.id);
                    }

                    // Seed the global primitive ID counter so future allocations
                    // never collide with any ID loaded from disk.
                    {
                        let max_id = self.panel_app.panel_grid.containers().values()
                            .flat_map(|w| self.panels_store.chart_panel.get(&w.chart_panel_id)
                                .map(|panel| panel.drawing_manager.primitives().iter()
                                    .collect::<Vec<_>>())
                                .unwrap_or_default())
                            .chain(
                                self.managers.bubble.bubbles()
                                    .flat_map(|g| g.primitives.iter())
                            )
                            .map(|p| p.data().id)
                            .max()
                            .unwrap_or(0);
                        mlc_core::drawing::seed_primitive_id_counter(max_id);
                    }

                    // Re-ID stashed primitives that collide with group primitives.
                    // Legacy autosave files may have saved both a stash primitive and
                    // its group counterpart with the same numeric ID, causing widget
                    // ID collisions in the Object Tree sidebar.  Assign fresh IDs to
                    // any stash primitive whose ID already exists in group.primitives.
                    {
                        // Phase 1: collect (window_key, group_id, set_of_group_prim_ids)
                        // using immutable borrows only.
                        let container_ids: Vec<mlc_core::ChartId> =
                            self.panel_app.panel_grid.containers().keys().copied().collect();
                        let collision_info: Vec<(mlc_core::ChartId, mlc_core::tag_manager::BubbleId, std::collections::HashSet<u64>)> =
                            container_ids.into_iter()
                                .filter_map(|cid| {
                                    let gid = self.managers.bubble.bubble_for_chart(cid)?;
                                    let group_ids: std::collections::HashSet<u64> =
                                        self.managers.bubble.bubble(gid)
                                            .map(|g| g.primitives.iter().map(|p| p.data().id).collect())
                                            .unwrap_or_default();
                                    Some((cid, gid, group_ids))
                                })
                                .collect();

                        // Phase 2: mutably iterate auto_bubble.primitives and re-ID colliding ones.
                        for (chart_id, _gid, group_ids) in &collision_info {
                            let mut reid_count = 0u32;
                            let auto_bid = self.panel_app.panel_grid.containers()
                                .get(chart_id)
                                .map(|w| w.auto_bubble_id);
                            if let Some(bid) = auto_bid {
                                if let Some(auto_bub) = self.managers.bubble.bubble_mut(bid) {
                                    for p in auto_bub.primitives.iter_mut() {
                                        if group_ids.contains(&p.data().id) {
                                            p.data_mut().id = mlc_core::drawing::alloc_primitive_id();
                                            reid_count += 1;
                                        }
                                    }
                                }
                            }
                            if reid_count > 0 {
                                eprintln!(
                                    "[ChartApp] Re-IDed {} auto-bubble primitive(s) in window {:?} (legacy autosave collision fix)",
                                    reid_count, chart_id
                                );
                            }
                        }
                    }

                    // Deduplicate IDs within each group.primitives collection.
                    // Old presets may have group primitives with identical IDs
                    // (from historical clone paths that didn't re-ID).
                    for group in self.managers.bubble.bubbles_mut() {
                        let mut seen_ids = std::collections::HashSet::new();
                        let mut reid_count = 0u32;
                        for p in &mut group.primitives {
                            let id = p.data().id;
                            if !seen_ids.insert(id) {
                                p.data_mut().id = mlc_core::drawing::alloc_primitive_id();
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
                    if preset.bubbles.is_empty() && !self.panel_app.panel_grid.containers().is_empty() {
                        // Auto groups get transparent color  -  never occupy palette slots
                        let Some((_, first_win)) = self.panel_app.panel_grid.iter_containers().next() else {
                            eprintln!("[sync_group] preset restore step 5b: iter_containers empty despite non-empty guard - skipping auto group");
                            return;
                        };
                        // Phase 4 S2.7.1c: read via branch_meta (B6 capture flip).
                        let first_cid = first_win.id;
                        let symbol = self.managers.bubble.symbol_for_chart(first_cid)
                            .map(String::from).unwrap_or_default();
                        let timeframe = self.managers.bubble.timeframe_for_chart(first_cid)
                            .cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                        let group_id = self.managers.bubble.create_group_auto([0.0, 0.0, 0.0, 0.0], symbol, timeframe);
                        let leaf_chart_ids: Vec<(mlc_core::LeafId, mlc_core::ChartId)> =
                            self.panel_app.panel_grid.iter_containers()
                                .map(|(leaf_id, w)| (leaf_id, w.id))
                                .collect();
                        let style_arc = self.managers.bubble
                            .bubble(group_id)
                            .map(|g| g.last_used_style.clone());
                        for &(leaf_id, chart_id) in &leaf_chart_ids {
                            let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, group_id);
                            if let Some(pid) = self.panel_app.panel_grid.container_for_leaf(leaf_id).map(|w| w.chart_panel_id) {
                                if let Some(ref arc) = style_arc {
                                    if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                                        panel.drawing_manager.bind_style_store(arc.clone());
                                    }
                                }
                            }
                        }
                        eprintln!("[ChartApp] Auto-created invisible sync group {:?} for {} orphaned windows", group_id, leaf_chart_ids.len());
                    }

                    // Step 5c: membership safety net. Step 5b only fires when the
                    // preset has NO groups at all — but poisoned files (startup-
                    // autosave bug) carry orphaned groups whose member lists do
                    // NOT include the windows. A window without bubble membership
                    // is a dead chart: symbol_for_chart=None -> empty tag/toolbar,
                    // no bars requested, the preset is never considered
                    // materialized so it never parks. Give every orphaned window
                    // its own auto group seeded from its (recovered) snapshot.
                    {
                        let orphan_windows: Vec<(mlc_core::LeafId, mlc_core::ChartId)> =
                            self.panel_app.panel_grid.iter_containers()
                                .filter(|(_, w)| self.managers.bubble.bubble_for_chart(w.id).is_none())
                                .map(|(leaf_id, w)| (leaf_id, w.id))
                                .collect();
                        for &(leaf_id, chart_id) in &orphan_windows {
                            let snap = preset.windows.iter().find(|s| s.window_id == chart_id.0);
                            let (sym, exch, at, tf) = snap
                                .map(|s| (
                                    s.symbol.clone(),
                                    s.exchange.clone(),
                                    s.account_type.clone(),
                                    s.timeframe.clone(),
                                ))
                                .unwrap_or_else(|| (
                                    "BTCUSDT".to_string(),
                                    "binance".to_string(),
                                    "S".to_string(),
                                    mlc_core::state::Timeframe::h1(),
                                ));
                            let group_id = self.managers.bubble.create_group_auto(
                                [0.0, 0.0, 0.0, 0.0],
                                sym.clone(),
                                tf,
                            );
                            if let Some(bubble) = self.managers.bubble.bubble_mut(group_id) {
                                bubble.exchange = exch.clone();
                                bubble.account_type = if at.is_empty() { "S".to_string() } else { at };
                            }
                            let _ = self.managers.bubble.attach_chart_to_bubble(chart_id, group_id);
                            if let Some(pid) = self.panel_app.panel_grid.container_for_leaf(leaf_id).map(|w| w.chart_panel_id) {
                                let panel = crate::chart_panel_bridge::ensure_chart_panel_state(
                                    &mut self.panels_store,
                                    pid,
                                );
                                panel.drawing_manager.set_current_symbol_key(&sym, &exch, "S");
                            }
                            eprintln!(
                                "[ChartApp] Step 5c: re-membered orphaned window {:?} into auto group {:?} ({})",
                                chart_id, group_id, sym
                            );
                        }
                    }

                    // ----------------------------------------------------------------
                    // Step 6: Restore indicators
                    // ----------------------------------------------------------------
                    self.indicator_manager.clear_all();
                    // Re-arm the one-shot indicator heal (tick.rs): restored
                    // instances reuse ids from the previous preset.
                    self.indicator_heal_attempted.clear();
                    for ind_snap in &preset.indicators {
                        if self.indicator_manager.create_instance_with_id(
                            ind_snap.id,
                            &ind_snap.type_id,
                        ) {
                            // Bind to the persisted window_id via the index  -  direct
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
                    // them to the indicator engine right now  -  no need to wait for the
                    // next BarsLoaded / TradeUpdate round-trip.
                    // Collect (symbol, window_id, bars) first to avoid a split borrow of
                    // `self` (panel_grid vs indicator_manager both live on `self`).
                    // ----------------------------------------------------------------
                    let window_bar_data: Vec<(u64, Vec<mlc_core::Bar>)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
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
                    self.pending_sub_pane_hidden.clear();
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
                        if !snap.sub_pane_hidden.is_empty() {
                            self.pending_sub_pane_hidden
                                .insert(snap.window_id, snap.sub_pane_hidden.clone());
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
                        let leaf_id = mlc_core::LeafId(leaf_raw);
                        self.panel_app.leaf_color_tags.insert(leaf_id, color);
                    }
                    eprintln!(
                        "[ChartApp] Step7: restored {} leaf_color_tags from snapshot",
                        self.panel_app.leaf_color_tags.len()
                    );

                    // ----------------------------------------------------------------
                    // Step 8: Restore alerts
                    // ----------------------------------------------------------------
                    self.managers.alert.restore(preset.alerts.clone());
                    eprintln!("[ChartApp] restored {} alerts", self.managers.alert.len());

                    // Step 8b: Restore per-slot Panel docking layouts.
                    // Pre-populate the store with panel state so the closure
                    // can construct the matching `Panel` variants.
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

                    // Helper: convert PersistedSymbolSource  ->  SymbolSource.
                    let convert_source = |src: &mlc_core::preset::preset::PersistedSymbolSource| -> mlc_panels::trading::SymbolSource {
                        match src {
                            mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => mlc_panels::trading::SymbolSource::HyperFocus,
                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, exchange, account_type } => mlc_panels::trading::SymbolSource::Fixed {
                                symbol: symbol.clone(),
                                exchange: exchange.clone(),
                                account_type: normalize_account_type(account_type),
                            },
                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { leaf_id } => mlc_panels::trading::SymbolSource::BoundToChart {
                                leaf_id: *leaf_id,
                            },
                        }
                    };

                    for i in 0..4 {
                        // Pre-insert state into the store keyed by the saved panel_id.
                        for pl in &preset.slot_leaves[i] {
                            let pid = mlc_core::state::PanelId(pl.panel_id);
                            use mlc_core::preset::preset::PersistedFreeItemKind;
                            match &pl.kind {
                                PersistedFreeItemKind::Dom { source, tick_size, levels_displayed, center_price } => {
                                    if !self.panels_store.dom.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::order_flow::dom::DomState::new(symbol, *tick_size);
                                        s.levels_displayed = *levels_displayed;
                                        s.center_price = *center_price;
                                        // Market-data panels no longer carry SymbolSource; TagManager tracks their group.
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
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
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::order_flow::footprint::FootprintState::new(symbol, *tick_size);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
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
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::order_flow::volume_profile::VolumeProfileState::new(symbol, *tick_size);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
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
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::order_flow::liquidity_heatmap::LiquidityHeatmapState::new(symbol, *tick_size, *snapshot_interval_ms);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
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
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::order_flow::big_trades::BigTradesState::new();
                                        s.symbol = symbol;
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
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
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::order_flow::l2_tape::L2TapeState::new();
                                        s.symbol = symbol;
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
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
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::order_flow::trade_tape::TradeTapeState::new();
                                        s.symbol = symbol;
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
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
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::trading::order_entry::OrderEntryState::new(symbol);
                                        s.source = convert_source(source);
                                        if let mlc_core::preset::preset::PersistedSymbolSource::Fixed { exchange, account_type, .. } = source {
                                            s.exchange = exchange.clone();
                                            s.account_type = normalize_account_type(account_type);
                                        }
                                        self.panels_store.order_entry.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::PositionManager => {
                                    if !self.panels_store.position_manager.contains_key(&pid) {
                                        self.panels_store.position_manager.insert(pid, mlc_panels::trading::trading::position_manager::PositionManagerState::new());
                                    }
                                }
                                PersistedFreeItemKind::TradeLog => {
                                    if !self.panels_store.trade_log.contains_key(&pid) {
                                        self.panels_store.trade_log.insert(pid, mlc_panels::trading::trading::trade_log::TradeLogState::new());
                                    }
                                }
                                PersistedFreeItemKind::RiskCalculator => {
                                    if !self.panels_store.risk_calculator.contains_key(&pid) {
                                        self.panels_store.risk_calculator.insert(pid, mlc_panels::trading::trading::risk_calculator::RiskCalculatorState::new());
                                    }
                                }
                                PersistedFreeItemKind::TradingContainer { source, tick_size, market_price } => {
                                    if !self.panels_store.trading_container.contains_key(&pid) {
                                        let symbol = match source {
                                            mlc_core::preset::preset::PersistedSymbolSource::Fixed { symbol, .. } => symbol.clone(),
                                            mlc_core::preset::preset::PersistedSymbolSource::BoundToChart { .. } | mlc_core::preset::preset::PersistedSymbolSource::HyperFocus => String::new(),
                                        };
                                        let mut s = mlc_panels::trading::trading::trading_container::TradingContainerState::new(symbol, *tick_size, *market_price);
                                        s.source = convert_source(source);
                                        self.panels_store.trading_container.insert(pid, s);
                                    }
                                }
                                PersistedFreeItemKind::IndicatorPanel {
                                    symbol, exchange, account_type, instance_id,
                                    indicator_config_id, price_min, price_max, auto_scale,
                                    bar_spacing, view_position, height_ratio,
                                } => {
                                    if !self.panels_store.indicator_panel.contains_key(&pid) {
                                        let mut s = mlc_panels::trading::order_flow::indicator_panel::IndicatorPanelState::with_instance(
                                            symbol.clone(), *instance_id,
                                        );
                                        s.exchange = exchange.clone();
                                        s.account_type = account_type.clone();
                                        s.indicator_config_id = *indicator_config_id;
                                        s.price_min = *price_min;
                                        s.price_max = *price_max;
                                        s.auto_scale = *auto_scale;
                                        s.bar_spacing = *bar_spacing;
                                        s.view_position = *view_position;
                                        s.height_ratio = *height_ratio;
                                        self.panels_store.indicator_panel.insert(pid, s);
                                    }
                                }
                            }
                        }

                        // Build a leaf_id  ->  PersistedFreeLeaf map for O(1) lookup
                        // inside the restore closure.  type_id() now returns only the
                        // variant kind (e.g. "free_dom")  -  panel identity comes from
                        // the leaf_id passed by restore_tree_with_id.
                        let leaves_by_id: std::collections::HashMap<u64, &mlc_core::preset::preset::PersistedFreeLeaf> =
                            preset.slot_leaves[i].iter().map(|l| (l.leaf_id, l)).collect();

                        let mgr = match preset.slot_layouts[i].as_deref() {
                            Some(json) => {
                                match uzor::panels::serialize::LayoutSnapshot::from_json(json) {
                                    Ok(snap) => {
                                        match snap.restore_tree_with_id::<mlc_core::state::Panel, _>(|leaf_id, _type_id| {
                                            // Look up the persisted leaf descriptor by leaf_id.
                                            // The panel_id and kind are stored there; type_id is
                                            // not used (it only carries the variant kind, no id).
                                            let pl = leaves_by_id.get(&leaf_id)?;
                                            let pid = mlc_core::state::PanelId(pl.panel_id);
                                            use mlc_core::state::Panel;
                                            let item = match &pl.kind {
                                                mlc_core::preset::preset::PersistedFreeItemKind::Dom { .. }               => Panel::Dom(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::Footprint { .. }         => Panel::Footprint(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::VolumeProfile { .. }     => Panel::VolumeProfile(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::LiquidityHeatmap { .. }  => Panel::LiquidityHeatmap(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::BigTrades { .. }         => Panel::BigTrades(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::L2Tape { .. }            => Panel::L2Tape(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::TradeTape { .. }         => Panel::TradeTape(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::OrderEntry { .. }        => Panel::OrderEntry(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::PositionManager          => Panel::PositionManager(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::TradeLog                 => Panel::TradeLog(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::RiskCalculator           => Panel::RiskCalculator(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::TradingContainer { .. }  => Panel::TradingContainer(pid),
                                                mlc_core::preset::preset::PersistedFreeItemKind::IndicatorPanel { .. }   => Panel::IndicatorPanel(pid),
                                            };
                                            Some(item)
                                        }) {
                                            Ok(tree) => {
                                                mlc_sidebar_content::SlotDockingManager(
                                                    uzor::layout::DockState::from_tree(tree),
                                                )
                                            }
                                            Err(e) => {
                                                eprintln!("[ChartApp] slot{} restore_tree failed: {} - empty fallback", i + 1, e);
                                                mlc_sidebar_content::SlotDockingManager::new()
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[ChartApp] slot{} from_json failed: {} - empty fallback", i + 1, e);
                                        mlc_sidebar_content::SlotDockingManager::new()
                                    }
                                }
                            }
                            None => mlc_sidebar_content::SlotDockingManager::new(),
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
                    // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                    let new_subscriptions: std::collections::HashSet<(digdigdig3::ExchangeId, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .filter_map(|(_, w)| {
                            let cid = w.id;
                            let exch = self.managers.bubble.exchange_for_chart(cid)?;
                            let sym = self.managers.bubble.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            digdigdig3::ExchangeId::from_str(exch).map(|eid| (eid, sym))
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
                    // Phase 4 S2.7.1c: read via branch_meta (B6 tuple-collect flip).
                    let window_bar_data: Vec<(String, String, mlc_core::state::Timeframe, String)> = self
                        .panel_app
                        .panel_grid
                        .iter_containers()
                        .map(|(_, w)| {
                            let cid = w.id;
                            let sym = self.managers.bubble.symbol_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let exch = self.managers.bubble.exchange_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            let tf = self.managers.bubble.timeframe_for_chart(cid)
                                .cloned().unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let at = self.managers.bubble.account_type_for_chart(cid)
                                .map(String::from).unwrap_or_default();
                            (sym, exch, tf, at)
                        })
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

                    // Prune orphan Panel members from all restored sync groups.
                    // SyncGroupSnapshot.panel_members can carry ids from the prior
                    // session that were closed-then-saved in a race; those ids
                    // have no entry in panels_store after Step 8b. Leaving them
                    // in group.members + member_to_group leaks zombie references
                    // that mis-route propagate_* / apply_key_to_panels_in_group.
                    {
                        use mlc_core::tag_manager::SyncMemberId;
                        use mlc_core::state::PanelId;
                        // Collect live PanelIds across every panel-state map.
                        let live_panel_ids: std::collections::HashSet<u64> = {
                            let store = &self.panels_store;
                            let mut s: std::collections::HashSet<u64> = std::collections::HashSet::new();
                            for PanelId(id) in store.dom.keys() { s.insert(*id); }
                            for PanelId(id) in store.footprint.keys() { s.insert(*id); }
                            for PanelId(id) in store.volume_profile.keys() { s.insert(*id); }
                            for PanelId(id) in store.liquidity_heatmap.keys() { s.insert(*id); }
                            for PanelId(id) in store.big_trades.keys() { s.insert(*id); }
                            for PanelId(id) in store.l2_tape.keys() { s.insert(*id); }
                            for PanelId(id) in store.trade_tape.keys() { s.insert(*id); }
                            for PanelId(id) in store.order_entry.keys() { s.insert(*id); }
                            for PanelId(id) in store.position_manager.keys() { s.insert(*id); }
                            for PanelId(id) in store.trade_log.keys() { s.insert(*id); }
                            for PanelId(id) in store.risk_calculator.keys() { s.insert(*id); }
                            for PanelId(id) in store.trading_container.keys() { s.insert(*id); }
                            s
                        };
                        // Collect orphans (Panel members in any group with no live state).
                        let mut orphans: Vec<SyncMemberId> = Vec::new();
                        for group in self.managers.bubble.bubbles() {
                            for member in group.members.iter() {
                                if let SyncMemberId::Panel(pid) = member {
                                    if !live_panel_ids.contains(pid) {
                                        orphans.push(SyncMemberId::Panel(*pid));
                                    }
                                }
                            }
                        }
                        let pruned = orphans.len();
                        for member in orphans {
                            self.managers.bubble.disconnect(member);
                            self.managers.bubble.synced_panels_remove(member);
                        }
                        if pruned > 0 {
                            eprintln!("[ChartApp] preset restore: pruned {} orphan panel members from sync groups", pruned);
                        }
                    }

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
                        // No tabs left  -  fall back to __default__.
                        self.panel_app.active_preset_id = "__default__".to_string();
                    }
                }
                self.persist_profile();
            }

            ChartOutEvent::OpenTab { id } => {
                eprintln!("[ChartApp] open tab: {}", id);
                if self.panel_app.open_tabs.contains(&id) {
                    // Preset already open  -  just switch to it.
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
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
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
                    self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
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
                    // No active named preset  -  treat as Save As
                    self.process_chart_out_event(ChartOutEvent::OpenPresetSaveAs);
                }
            }

            ChartOutEvent::ToggleAutosave => {
                self.panel_app.autosave_enabled = !self.panel_app.autosave_enabled;
                eprintln!("[ChartApp] autosave = {}", self.panel_app.autosave_enabled);
            }

            ChartOutEvent::OpenChartBrowser => {
                self.panel_app.chart_browser.open(0);
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::ChartBrowser;
                eprintln!("[ChartApp] chart browser opened");
            }

            ChartOutEvent::OpenChartBrowserInNewTab => {
                self.panel_app.chart_browser.open(0);
                self.panel_app.chart_browser.open_in_new_tab = true;
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::ChartBrowser;
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
                    use mlc_core::preset::preset::unix_timestamp_parts;
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

                // Now compute the next "Untitled N"  -  the auto-saved one is already counted.
                let max_n = self.panel_app.presets.values()
                    .filter_map(|p| p.name.strip_prefix("Untitled "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Untitled {}", max_n + 1);
                self.panel_app.preset_name_input.open_new_chart(&default_name, 0);
                self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::PresetNameInput;
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
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.managers.bubble.bubble_mut(gid) {
                        group.sync_flags.sync_symbol = !group.sync_flags.sync_symbol;
                        newly_enabled = group.sync_flags.sync_symbol;
                        eprintln!("[ChartApp] InternalToggleSyncSymbol: group={:?} sync_symbol={}", gid, group.sync_flags.sync_symbol);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncSymbol: no active group");
                }
                // When sync is turned ON, immediately propagate active leaf's instrument key to peers.
                // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                if newly_enabled {
                    if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let active_cid = self.panel_app.panel_grid.active_container_id();
                        let symbol = active_cid
                            .and_then(|cid| self.managers.bubble.symbol_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let exchange = active_cid
                            .and_then(|cid| self.managers.bubble.exchange_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        let account_type = active_cid
                            .and_then(|cid| self.managers.bubble.account_type_for_chart(cid))
                            .map(String::from)
                            .unwrap_or_default();
                        if !symbol.is_empty() {
                            self.propagate_symbol_to_sync_group(leaf, &symbol, &exchange, &account_type);
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncTimeframe => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.managers.bubble.bubble_mut(gid) {
                        group.sync_flags.sync_timeframe = !group.sync_flags.sync_timeframe;
                        newly_enabled = group.sync_flags.sync_timeframe;
                        eprintln!("[ChartApp] InternalToggleSyncTimeframe: group={:?} sync_timeframe={}", gid, group.sync_flags.sync_timeframe);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncTimeframe: no active group");
                }
                // When sync is turned ON, immediately propagate active leaf's TF to peers.
                // Phase 4 S2.7.1c: read via branch_meta (B1 flip).
                if newly_enabled {
                    if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        let tf = self.panel_app.panel_grid.active_container_id()
                            .and_then(|cid| self.managers.bubble.timeframe_for_chart(cid))
                            .cloned();
                        if let Some(tf) = tf {
                            self.propagate_timeframe_to_sync_group(leaf, tf);
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncCrosshair => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
                let mut newly_enabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.managers.bubble.bubble_mut(gid) {
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
                            .active_container()
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
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
                let mut newly_enabled = false;
                let mut newly_disabled = false;
                if let Some(gid) = gid {
                    if let Some(group) = self.managers.bubble.bubble_mut(gid) {
                        let was_on = group.sync_flags.sync_viewport;
                        group.sync_flags.sync_viewport = !was_on;
                        newly_enabled = !was_on;
                        newly_disabled = was_on;
                        eprintln!("[ChartApp] InternalToggleSyncViewport: group={:?} sync_viewport={}", gid, group.sync_flags.sync_viewport);
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncViewport: no active group");
                }
                // OFF -> ON: adopt the active leaf's current scale_mode as the group mode,
                // stash each peer's prior mode, then push mode + viewport to all peers.
                if newly_enabled {
                    let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                    if let Some(active_leaf) = active_leaf {
                        // Capture the active window's current scale_mode as the new group mode.
                        let active_mode = self.panel_app.panel_grid
                            .active_container()
                            .map(|w| w.price_scale.scale_mode);
                        if let Some(mode) = active_mode {
                            if let Some(gid) = gid {
                                if let Some(group) = self.managers.bubble.bubble_mut(gid) {
                                    group.scale_mode = mode;
                                }
                            }
                        }
                        // Stash each peer's current mode and apply the group mode.
                        // propagate_viewport_to_sync_group applies mode to peers via the
                        // scale_mode argument; stash happens here before the call.
                        if let Some(gid) = gid {
                            let peer_leaves: Vec<mlc_core::LeafId> =
                                self.managers.bubble.chart_member_ids(gid)
                                    .into_iter()
                                    .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                                    .filter(|&lid| lid != active_leaf)
                                    .collect();
                            for peer_leaf in peer_leaves {
                                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(peer_leaf) {
                                    // Stash existing mode if not already stashed (preserve
                                    // the window's pre-join mode across multiple sync toggles).
                                    if window.stashed_scale_mode.is_none() {
                                        window.stashed_scale_mode = Some(window.price_scale.scale_mode);
                                    }
                                }
                            }
                        }
                        let viewport_state = self.panel_app
                            .panel_grid
                            .active_container()
                            .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                        if let Some((view_start, bar_spacing)) = viewport_state {
                            self.propagate_viewport_to_sync_group(
                                active_leaf, view_start, bar_spacing,
                                active_mode,
                            );
                        }
                    }
                }
                // ON -> OFF: clear stashed_scale_mode on all peers (they keep whatever
                // mode they currently display, which was the group mode).
                if newly_disabled {
                    if let Some(gid) = gid {
                        let all_leaves: Vec<mlc_core::LeafId> =
                            self.managers.bubble.chart_member_ids(gid)
                                .into_iter()
                                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                                .collect();
                        for leaf in all_leaves {
                            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf) {
                                window.stashed_scale_mode = None;
                            }
                        }
                    }
                }
                state_mutated = true;
            }

            ChartOutEvent::InternalToggleSyncDrawings => {
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
                let chart_id = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let Some(gid) = gid {
                    // Toggle the flag and capture the new value.
                    let new_sync = if let Some(group) = self.managers.bubble.bubble_mut(gid) {
                        group.sync_flags.sync_drawings = !group.sync_flags.sync_drawings;
                        eprintln!("[BubbleStore] sync_drawings toggled to {}", group.sync_flags.sync_drawings);
                        group.sync_flags.sync_drawings
                    } else {
                        return;
                    };

                    // Get auto_bubble_id and chart_panel_id before any mutable borrows.
                    let auto_bid = chart_id.and_then(|cid| {
                        self.panel_app.panel_grid.containers().get(&cid).map(|w| w.auto_bubble_id)
                    });
                    let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);

                    if new_sync {
                        // sync_drawings OFF -> ON: the chart was showing its own drawings (from
                        // auto_bubble); now it will show the color-group drawings via per-frame
                        // sync.  Save current drawing_manager locals into auto_bubble.primitives
                        // (they are the "home" state; auto_bubble must hold them for desync).
                        if let (Some(bid), Some(pid)) = (auto_bid, active_pid) {
                            if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                                let local: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                                    panel.drawing_manager.primitives().iter()
                                        .filter(|p| p.data().origin_id.is_none())
                                        .map(|p| p.clone_box())
                                        .collect();
                                let stash_count = local.len();
                                if let Some(auto_bub) = self.managers.bubble.bubble_mut(bid) {
                                    auto_bub.primitives = local;
                                }
                                panel.drawing_manager.clear_all_primitives();
                                eprintln!(
                                    "[BubbleStore] sync_drawings ON: moved {} window-local primitives into auto-bubble {:?}",
                                    stash_count, bid
                                );
                            }
                        }
                    } else {
                        // sync_drawings ON -> OFF: restore home drawings from auto_bubble into
                        // drawing_manager so the chart shows its own drawings again.
                        if let (Some(bid), Some(pid)) = (auto_bid, active_pid) {
                            let restored: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>> =
                                self.managers.bubble.bubble(bid)
                                    .map(|bub| bub.primitives.iter().map(|p| p.clone_box()).collect())
                                    .unwrap_or_default();
                            let count = restored.len();
                            if let Some(panel) = self.panels_store.chart_panel.get_mut(&pid) {
                                panel.drawing_manager.clear_all_primitives();
                                panel.drawing_manager.add_synced_primitives(restored);
                            }
                            eprintln!(
                                "[BubbleStore] sync_drawings OFF: restored {} primitives from auto-bubble {:?}",
                                count, bid
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
                let gid = self.panel_app.panel_grid.active_container_id().and_then(|cid| self.managers.bubble.bubble_for_chart(cid));
                let chart_id = self.panel_app.panel_grid.active_container().map(|w| w.id);
                if let (Some(gid), Some(chart_id)) = (gid, chart_id) {
                    // Toggle the flag and capture the new value.
                    let new_sync = if let Some(group) = self.managers.bubble.bubble_mut(gid) {
                        group.sync_flags.sync_indicators = !group.sync_flags.sync_indicators;
                        eprintln!("[BubbleStore] sync_indicators toggled to {}", group.sync_flags.sync_indicators);
                        group.sync_flags.sync_indicators
                    } else {
                        return;
                    };

                    let pre_tag_ids: Vec<u64> = {
                        let auto_bid = self.panel_app.panel_grid.containers()
                            .get(&chart_id)
                            .map(|w| w.auto_bubble_id);
                        auto_bid
                            .and_then(|bid| self.managers.bubble.bubble(bid))
                            .map(|bub| bub.pre_tag_indicator_ids.clone())
                            .unwrap_or_default()
                    };

                    if new_sync {
                        // OFF  ->  ON: hide window-local indicators (in pre_tag_indicator_ids
                        // but not yet in the group's indicator_configs).
                        let group_indicator_ids: std::collections::HashSet<u64> = self
                            .managers
                            .bubble
                            .bubble(gid)
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
                            "[BubbleStore] sync_indicators ON: hid {} window-local indicators",
                            to_hide.len()
                        );
                    } else {
                        // ON  ->  OFF: unhide window-local (pre_tag) indicators.
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
                            "[BubbleStore] sync_indicators OFF: unhid {} window-local indicators",
                            to_show.len()
                        );
                    }
                } else {
                    eprintln!("[ChartApp] InternalToggleSyncIndicators: no active group");
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
