//! Compare/Indicator overlay click and action handlers.
//! Handles overlay popups, settings clicks, and indicator instance lifecycle.

use crate::ChartApp;
use zengeld_chart::ui::modal_settings::DualSliderHandle;

impl ChartApp {
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
                    let leaf_id = zengeld_chart::LeafId(leaf_raw);
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
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let new_vis = window.compare_overlay.toggle_visibility(idx);
                        eprintln!("[ChartApp] cmp_overlay vis toggled: idx={} -> visible={}", idx, new_vis);
                    }
                }
            }
            _ if action.starts_with("delete:") => {
                if let Ok(idx) = action["delete:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(symbol) = window.compare_overlay.remove_series(idx) {
                            eprintln!("[ChartApp] cmp_overlay deleted: idx={} symbol={}", idx, symbol);
                        }
                    }
                }
            }
            _ if action.starts_with("settings:") => {
                if let Ok(idx) = action["settings:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window() {
                        if let Some(series) = window.compare_overlay.series.get(idx) {
                            self.panel_app.compare_settings_state.open(
                                idx,
                                &series.symbol.clone(),
                                &series.name.clone(),
                                &series.color.clone(),
                                series.line_width,
                                &series.line_style.clone(),
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
                    let price = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.bars.last())
                        .map(|b| b.close)
                        .unwrap_or(0.0);
                    let symbol = self.panel_app.panel_grid.active_window()
                        .map(|w| w.symbol.clone())
                        .unwrap_or_else(|| "BTCUSD".to_string());
                    let source_name = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.compare_overlay.series.get(idx))
                        .map(|s| format!("Compare: {}", s.symbol))
                        .unwrap_or_else(|| format!("Compare series {}", idx));
                    let source = alerts::AlertSource::Price { symbol: symbol.clone() };
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
    pub(super) fn handle_leaf_cmp_overlay_action(&mut self, leaf_id: zengeld_chart::LeafId, action: &str) {
        // Route to the same per-window logic. In split mode compare overlays are
        // per-window; we currently act on the active window's compare overlay.
        match action {
            _ if action.starts_with("vis:") => {
                if let Ok(idx) = action["vis:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let new_vis = window.compare_overlay.toggle_visibility(idx);
                        eprintln!("[ChartApp] cmp_overlay leaf {} vis toggled: idx={} -> visible={}", leaf_id.0, idx, new_vis);
                    }
                }
            }
            _ if action.starts_with("delete:") => {
                if let Ok(idx) = action["delete:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(symbol) = window.compare_overlay.remove_series(idx) {
                            eprintln!("[ChartApp] cmp_overlay leaf {} deleted: idx={} symbol={}", leaf_id.0, idx, symbol);
                        }
                    }
                }
            }
            _ if action.starts_with("settings:") => {
                if let Ok(idx) = action["settings:".len()..].parse::<usize>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window() {
                        if let Some(series) = window.compare_overlay.series.get(idx) {
                            self.panel_app.compare_settings_state.open(
                                idx,
                                &series.symbol.clone(),
                                &series.name.clone(),
                                &series.color.clone(),
                                series.line_width,
                                &series.line_style.clone(),
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
                    let price = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.bars.last())
                        .map(|b| b.close)
                        .unwrap_or(0.0);
                    let symbol = self.panel_app.panel_grid.active_window()
                        .map(|w| w.symbol.clone())
                        .unwrap_or_else(|| "BTCUSD".to_string());
                    let source_name = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.compare_overlay.series.get(idx))
                        .map(|s| format!("Compare: {}", s.symbol))
                        .unwrap_or_else(|| format!("Compare series {}", idx));
                    let source = alerts::AlertSource::Price { symbol: symbol.clone() };
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
                let orig_tf = self.panel_app.compare_settings_state.original_timeframe_visibility.clone();
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.compare_overlay.set_series_color_by_index(idx, &orig_color);
                    window.compare_overlay.set_series_line_width_by_index(idx, orig_width);
                    window.compare_overlay.set_series_line_style_by_index(idx, &orig_style);
                    if let Some(tf) = orig_tf.clone() {
                        window.compare_overlay.set_series_timeframe_visibility(idx, tf);
                    }
                }
                self.panel_app.compare_settings_state.cached_color = orig_color;
                self.panel_app.compare_settings_state.cached_line_width = orig_width;
                self.panel_app.compare_settings_state.cached_line_style = orig_style;
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
                use zengeld_chart::ui::modal_settings::CompareSettingsTab;
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.compare_overlay.set_series_line_style_by_index(idx, style);
                }
                self.autosave_snapshot();
                eprintln!("[ChartApp] cmp_settings line_style set to: {}", style);
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
                            .unwrap_or_else(zengeld_chart::drawing::TimeframeVisibilityConfig::all);
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
                        .unwrap_or_else(zengeld_chart::drawing::TimeframeVisibilityConfig::all);
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
                    let cursor = text.len();
                    self.panel_app.compare_settings_state.editing_text = Some(
                        zengeld_chart::ui::modal_settings::TextEditingState {
                            field_id,
                            text,
                            cursor,
                            selection_start: Some(0),
                            blink_time: 0,
                        }
                    );
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
                let max_n = self.panel_app.template_manager.compare_templates
                    .iter()
                    .filter_map(|t| t.name.strip_prefix(prefix))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("{}{}", prefix, max_n + 1);
                let default_cursor = default_name.chars().count();
                self.panel_app.compare_settings_state.template_name_editing = Some(
                    zengeld_chart::ui::modal_settings::TextEditingState {
                        field_id: "template_name".to_string(),
                        text: default_name,
                        cursor: default_cursor,
                        selection_start: None,
                        blink_time: 0,
                    }
                );
                eprintln!("[ChartApp] cmp_settings template save-as opened");
            }
            "template_default" => {
                self.panel_app.compare_settings_state.template_dropdown_open = false;
                self.panel_app.compare_settings_state.applied_template_id = None;
                let defaults = zengeld_chart::templates::CompareTemplate::new_with_defaults("__default__");
                self.panel_app.compare_settings_state.cached_color = defaults.color.clone();
                self.panel_app.compare_settings_state.cached_line_width = defaults.line_width;
                self.panel_app.compare_settings_state.cached_line_style = defaults.line_style.clone();
                let idx = self.panel_app.compare_settings_state.series_index;
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
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
                if let Some(tmpl) = self.panel_app.template_manager.get_compare_template(tmpl_id).cloned() {
                    let idx = self.panel_app.compare_settings_state.series_index;
                    self.panel_app.compare_settings_state.applied_template_id = Some(tmpl.id.clone());
                    self.panel_app.compare_settings_state.template_dropdown_open = false;
                    // Apply template styles immediately
                    self.panel_app.compare_settings_state.cached_color = tmpl.color.clone();
                    self.panel_app.compare_settings_state.cached_line_width = tmpl.line_width;
                    self.panel_app.compare_settings_state.cached_line_style = tmpl.line_style.clone();
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
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
        use zengeld_chart::drawing::TimeframeVisibilityConfig;
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
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.compare_overlay.set_series_timeframe_visibility(series_idx, tf_config);
        }
        self.autosave_snapshot();
        eprintln!("[ChartApp] cmp_settings tf_{} toggled", tf_idx);
    }

    /// Apply a committed min/max value from the compare settings text input.
    pub(super) fn apply_cmp_tf_text_commit(&mut self, field_id: &str, text: &str) {
        use zengeld_chart::drawing::TimeframeVisibilityConfig;
        use zengeld_chart::ui::modal_settings::DualSliderHandle;

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
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.compare_overlay.set_series_timeframe_visibility(series_idx, tf_config);
        }
        self.autosave_snapshot();
        eprintln!("[ChartApp] cmp_settings tf_{} {:?} committed: {}", tf_idx, handle, new_val);
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
        // In split mode the widget ID has the form `leaf{leaf_id}:{action}`.
        // Detect that prefix and route to per-leaf state.
        if rest.starts_with("leaf") {
            // Extract leaf_id: rest = "leaf{n}:{action}"
            if let Some(colon_pos) = rest.find(':') {
                let leaf_id_str = &rest["leaf".len()..colon_pos];
                let action = &rest[colon_pos + 1..];
                if let Ok(leaf_raw) = leaf_id_str.parse::<u64>() {
                    let leaf_id = zengeld_chart::LeafId(leaf_raw);
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
                    self.indicator_manager.toggle_visibility(id);
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
                        let price = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.bars.last())
                            .map(|b| b.close)
                            .unwrap_or(0.0);
                        let label = self.indicator_manager.get_instance(id)
                            .map(|inst| inst.name.clone())
                            .unwrap_or_else(|| format!("Indicator {}", id));
                        let symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_else(|| "BTCUSD".to_string());
                        let source = alerts::AlertSource::Signal {
                            indicator_id: id,
                            label: label.clone(),
                            direction_filter: alerts::SignalDirection::Any,
                            bar_state: alerts::SignalBarState::Forming,
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
    pub(super) fn handle_leaf_indicator_overlay_action(&mut self, leaf_id: zengeld_chart::LeafId, action: &str) {
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
                    self.indicator_manager.toggle_visibility(id);
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
                        let price = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.bars.last())
                            .map(|b| b.close)
                            .unwrap_or(0.0);
                        let label = self.indicator_manager.get_instance(id)
                            .map(|inst| inst.name.clone())
                            .unwrap_or_else(|| format!("Indicator {}", id));
                        let symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_else(|| "BTCUSD".to_string());
                        let source = alerts::AlertSource::Signal {
                            indicator_id: id,
                            label: label.clone(),
                            direction_filter: alerts::SignalDirection::Any,
                            bar_state: alerts::SignalBarState::Forming,
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
    pub(super) fn delete_indicator_instance(&mut self, id: u64) {
        let type_id = match self.indicator_manager.get_instance(id) {
            Some(inst) => inst.type_id.clone(),
            None => return,
        };

        self.push_undo_command(zengeld_chart::Command::RemoveIndicator {
            instance_id: id,
            type_id: type_id.clone(),
            params_json: String::new(),
        });
        eprintln!("[ChartApp] Recorded RemoveIndicator {} id={}", type_id, id);

        // Collect group members for the active window (empty vec if no group).
        let group_members: Vec<u64> = self.panel_app.panel_grid.docking()
            .active_leaf()
            .and_then(|leaf| self.panel_app.panel_grid.chart_id_for_leaf(leaf))
            .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            .and_then(|gid| self.panel_app.tag_manager.group(gid))
            .map(|g| g.members.iter().filter_map(|m| m.as_chart()).collect())
            .unwrap_or_default();

        // Remove at most ONE indicator_config with this type_id from the group.
        // Using retain-all would incorrectly delete configs for duplicate indicators
        // of the same type (e.g. two "ac" indicators on the same window).
        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
            if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                if let Some(group_id) = self.panel_app.tag_manager.group_for_window(chart_id) {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                        if let Some(pos) = group.indicator_configs.iter().position(|c| c.type_id == type_id) {
                            group.indicator_configs.remove(pos);
                            eprintln!(
                                "[TagManager] Removed 1 indicator config with type_id={} from group {:?}",
                                type_id,
                                group_id,
                            );
                        }
                    }
                }
            }
        }

        // Find the window that owns the specific instance being deleted.
        let origin_window_id = self.indicator_manager.instances_iter()
            .find(|inst| inst.id == id)
            .and_then(|inst| inst.window_id);

        // Sweep only PEER windows (same type_id, same group, but NOT the originating window).
        // This prevents deleting a second "ac" indicator on the same window when only one
        // was requested to be removed.
        let peer_instances: Vec<u64> = self.indicator_manager.instances_iter()
            .filter(|inst| {
                inst.type_id == type_id
                    && inst.id != id
                    && inst.window_id.map(|wid| group_members.contains(&wid)).unwrap_or(false)
                    && inst.window_id != origin_window_id
            })
            .map(|inst| inst.id)
            .collect();

        // Build the full removal list: the specific instance + its peers on other windows.
        let mut all_to_remove = vec![id];
        all_to_remove.extend(peer_instances);

        for inst_id in &all_to_remove {
            self.indicator_manager.remove_instance(*inst_id);
            self.alert_manager.remove_alerts_for_indicator(*inst_id);
        }

        // Remove stale entries from every window's pre_tag_indicator_ids so the
        // Object Tree does not show phantom items for deleted indicators.
        for window in self.panel_app.panel_grid.windows_mut().values_mut() {
            window.pre_tag_indicator_ids.retain(|iid| !all_to_remove.contains(iid));
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
}
