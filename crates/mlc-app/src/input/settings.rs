//! Settings modal handlers: chart settings, primitive settings, indicator settings,
//! alert settings, template modals (per-modal commit/apply for editing text).

use crate::ChartApp;
use mlc_core::{
    CrosshairMode,
    HorzAlign, VertAlign,
    LegendPosition,
    cycle_precision,
    DateFormat,
    PriceScalePosition,
    TimeScalePosition,
    ScaleCornerVisibility,
    ThemeSettingsPanel,
    UIStyle,
};
use mlc_app_layout::ui::modal_settings::DualSliderHandle;
use mlc_core::drawing::TimeframeVisibilityConfig;

impl ChartApp {
    // -------------------------------------------------------------------------
    // Modal widget click handlers
    // -------------------------------------------------------------------------

    /// Handle clicks on widgets registered with the "chart_settings:" prefix.
    pub(super) fn handle_chart_settings_click(&mut self, rest: &str, x: f64, _y: f64) {
        use mlc_app_layout::ui::modal_settings::ChartSettingsTab;

        // Auto-commit: if an input field is being edited and the click lands on a
        // DIFFERENT widget inside the same modal, commit the value (same as Enter).
        if self.panel_app.chart_settings_state.active_field_id.is_some() {
            let editing_field = self.panel_app.chart_settings_state.active_field_id
                .as_deref()
                .unwrap_or_default()
                .to_string();
            let click_targets_same_field = rest
                .strip_prefix("item:")
                .map(|item_id| item_id == editing_field)
                .unwrap_or(false);
            if !click_targets_same_field {
                self.commit_chart_settings_editing_text();
            }
        }

        match rest {
            "close" => {
                self.panel_app.chart_settings_state.close();
            }
            "modal_bg" => {
                // Click inside modal body — close template dropdown if open
                self.panel_app.chart_settings_state.template_dropdown_open = false;
            }
            _ if rest.starts_with("tab:") => {
                self.panel_app.chart_settings_state.template_dropdown_open = false;
                let tab_id = &rest["tab:".len()..];
                if let Some(tab) = ChartSettingsTab::from_id(tab_id) {
                    self.panel_app.chart_settings_state.set_tab(tab);
                    eprintln!("[ChartApp] chart_settings tab: {}", tab_id);
                }
            }
            _ if rest.starts_with("item:") => {
                self.panel_app.chart_settings_state.template_dropdown_open = false;
                let item_id = &rest["item:".len()..];
                // Click-to-cursor: if the clicked field is already being edited,
                // reposition cursor using active_input_char_positions.
                let already_editing = self.panel_app.chart_settings_state.active_field_id
                    .as_deref() == Some(item_id);
                if already_editing {
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.chart_settings.as_ref())
                        .map(|cs| cs.active_input_char_positions.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let wid = uzor::WidgetId::from(crate::text_input::CHART_SETTINGS_FIELD);
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        let mut coord = self.input_coordinator.borrow_mut();
                        if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                        return;
                    }
                }
                self.handle_chart_settings_item(item_id);
                // Snapshot after any item change (color pickers open lazily, so
                // their changes are captured via apply_chart_settings_color).
                self.snapshot_chart_settings_to_user_manager();
            }
            _ if rest.starts_with("footer:") => {
                let btn_id = &rest["footer:".len()..];
                match btn_id {
                    // No "cancel": this modal edits a chart that already
                    // exists and every control applies as it is touched, so
                    // OK, the ✕ and Escape are one and the same exit.
                    "ok" => {
                        self.panel_app.chart_settings_state.close();
                        eprintln!("[ChartApp] chart_settings closed via footer: {}", btn_id);
                    }
                    "template" => {
                        self.panel_app.chart_settings_state.template_dropdown_open =
                            !self.panel_app.chart_settings_state.template_dropdown_open;
                        eprintln!("[ChartApp] chart_settings template_dropdown toggled");
                    }
                    "template_save_as" => {
                        self.panel_app.chart_settings_state.template_dropdown_open = false;
                        self.panel_app.chart_settings_state.save_template_mode = true;
                        let prefix = "Мой шаблон ";
                        let max_n = self.core.templates().chart_templates()
                            .iter()
                            .filter_map(|t| t.name.strip_prefix(prefix))
                            .filter_map(|s| s.parse::<u32>().ok())
                            .max()
                            .unwrap_or(0);
                        let default_name = format!("{}{}", prefix, max_n + 1);
                        let field_id = uzor::WidgetId::from(crate::text_input::CHART_SETTINGS_TEMPLATE_NAME);
                        self.input_coordinator.borrow_mut().text_fields_mut().focus(field_id.clone());
                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&field_id, &default_name);
                        eprintln!("[ChartApp] chart_settings template save-as opened");
                    }
                    "template_default" => {
                        self.panel_app.chart_settings_state.template_dropdown_open = false;
                        self.panel_app.chart_settings_state.applied_template_id = None;
                        let defaults = mlc_core::templates::ChartTemplate::developer_defaults();
                        self.apply_chart_template(&defaults);
                        self.snapshot_chart_settings_to_user_manager();
                        eprintln!("[ChartApp] chart_settings reset to developer defaults");
                    }
                    "template_dropdown_menu" => {
                        // Absorb click inside dropdown, keep it open
                    }
                    _ if btn_id.starts_with("template_delete:") => {
                        let tmpl_id = &btn_id["template_delete:".len()..];
                        eprintln!("[ChartApp] chart_settings deleted chart template: {}", tmpl_id);
                        if self.panel_app.chart_settings_state.applied_template_id.as_deref() == Some(tmpl_id) {
                            self.panel_app.chart_settings_state.applied_template_id = None;
                        }
                        self.template_actions.push(crate::TemplateAction::RemoveChart { id: tmpl_id.to_string() });
                    }
                    _ if btn_id.starts_with("template_option:") => {
                        let tmpl_id = &btn_id["template_option:".len()..];
                        let tmpl_opt = self.core.templates().get_chart_template(tmpl_id).cloned();
                        if let Some(tmpl) = tmpl_opt {
                            self.panel_app.chart_settings_state.applied_template_id = Some(tmpl.id.clone());
                            self.panel_app.chart_settings_state.template_dropdown_open = false;
                            self.apply_chart_template(&tmpl);
                            self.snapshot_chart_settings_to_user_manager();
                            eprintln!("[ChartApp] chart_settings template applied: {}", tmpl.name);
                        }
                    }
                    _ => {
                        eprintln!("[ChartApp] chart_settings footer: {}", btn_id);
                    }
                }
            }
            _ => {
                eprintln!("[ChartApp] chart_settings unhandled: {}", rest);
            }
        }
    }

    /// Commit whatever text is currently being edited in the chart settings modal.
    ///
    /// Applies the same logic as the Enter-key handler in `on_char_input` so that
    /// clicking a different widget inside the same modal auto-commits the in-progress value.
    pub(crate) fn commit_chart_settings_editing_text(&mut self) {
        let field = match self.panel_app.chart_settings_state.active_field_id.take() {
            Some(f) => f,
            None => return,
        };
        let wid = uzor::WidgetId::from(crate::text_input::CHART_SETTINGS_FIELD);
        let text = self.input_coordinator.borrow().text_fields().text(&wid).to_string();
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        if field == "status:watermark_text" {
            let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            if let Some(pid) = pid {
                let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                if let Some(ref mut watermark) = panel.watermark {
                    if let Some(line) = watermark.lines.first_mut() {
                        line.text = text.clone();
                    }
                }
            }
            eprintln!("[ChartApp] chart_settings watermark_text auto-committed: {}", text);
        } else {
            eprintln!("[ChartApp] chart_settings '{}' editing auto-closed (value: {})", field, text);
        }
    }

    /// Apply all settings from a [`ChartTemplate`] to the current chart state.
    ///
    /// Deserializes the stored JSON fields back into the concrete settings structs
    /// and updates both the `chart_settings_state` cached flags and the active
    /// chart window's live state.
    fn apply_chart_template(&mut self, tmpl: &mlc_core::templates::ChartTemplate) {
        use mlc_app_layout::layout::modals::chart_settings::{
            InstrumentSettings, ScalesLinesSettings, StatusLineSettings,
        };

        // ── Instrument settings ───────────────────────────────────────────────
        if let Ok(inst) = serde_json::from_value::<InstrumentSettings>(tmpl.instrument.clone()) {
            let css = &mut self.panel_app.chart_settings_state;
            css.instrument_use_prev_close = inst.use_prev_close_color;
            css.instrument_body_enabled   = inst.body_enabled;
            css.instrument_border_enabled = inst.border_enabled;
            css.instrument_wick_enabled   = inst.wick_enabled;

            // Apply candle colors to the theme
            let rt = self.panel_app.theme_manager.current_mut();
            rt.series.candle_up_body   = inst.body_up_color.clone();
            rt.series.candle_down_body = inst.body_down_color.clone();
            rt.series.candle_up_wick   = inst.wick_up_color.clone();
            rt.series.candle_down_wick = inst.wick_down_color.clone();
            rt.series.footprint_buy    = inst.footprint_buy_color.clone();
            rt.series.footprint_sell   = inst.footprint_sell_color.clone();
            rt.series.renko_up         = inst.renko_up_color.clone();
            rt.series.renko_down       = inst.renko_down_color.clone();
            rt.series.pnf_x            = inst.pnf_x_color.clone();
            rt.series.pnf_o            = inst.pnf_o_color.clone();
            rt.series.tlb_up           = inst.tlb_up_color.clone();
            rt.series.tlb_down         = inst.tlb_down_color.clone();
            rt.series.kagi_yang        = inst.kagi_yang_color.clone();
            rt.series.kagi_yin         = inst.kagi_yin_color.clone();
            rt.series.hollow_up        = inst.hollow_up_color.clone();
            rt.series.hollow_down      = inst.hollow_down_color.clone();
            rt.series.line_color       = inst.line_color.clone();
            rt.series.step_line_color  = inst.step_line_color.clone();
            rt.series.area_line        = inst.area_line_color.clone();
            rt.series.area_top         = inst.area_top_color.clone();
            rt.series.area_bottom      = inst.area_bottom_color.clone();
            rt.series.baseline_top_line    = inst.baseline_top_line_color.clone();
            rt.series.baseline_top_fill    = inst.baseline_top_fill_color.clone();
            rt.series.baseline_bottom_line = inst.baseline_bottom_line_color.clone();
            rt.series.baseline_bottom_fill = inst.baseline_bottom_fill_color.clone();
            rt.series.bar_up            = inst.bars_up_color.clone();
            rt.series.bar_down          = inst.bars_down_color.clone();
            rt.series.line_markers_color = inst.line_markers_color.clone();

            css.footprint_show_abs_markers = inst.footprint_show_abs_markers;
            css.footprint_show_volume_text = inst.footprint_show_volume_text;
            css.tick_intensity = inst.tick_intensity;
            css.bar_family_wicks = inst.bar_family_wicks;
            let heat_intensity = mlc_core::chart::FootprintHeatIntensity::from_id(
                &inst.footprint_heat_intensity_label.to_lowercase(),
            );
            let imbalance_threshold = inst.footprint_imbalance_threshold_label
                .trim_end_matches('x')
                .parse::<f64>()
                .unwrap_or(1.5);
            let cell_mode = mlc_core::chart::FootprintCellMode::from_label(
                &inst.footprint_cell_mode_label,
            );

            // Footprint display gates actually drive the renderer (unlike
            // the JC checkboxes above, which are UI-cache only — see recon
            // 2026-07-05 §4b) — write through to the live per-panel state
            // too, not just the modal's UI cache. `tick_intensity` /
            // `bar_family_wicks` / `tpo_display` follow the same rule
            // (mirrors `footprint_display`).
            let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            if let Some(pid) = active_pid {
                let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                panel.footprint_display.show_abs_markers = inst.footprint_show_abs_markers;
                panel.footprint_display.show_volume_text = inst.footprint_show_volume_text;
                panel.footprint_display.heat_intensity = heat_intensity;
                panel.footprint_display.imbalance_threshold = imbalance_threshold;
                panel.footprint_display.cell_mode = cell_mode;
                panel.tick_intensity = inst.tick_intensity;
                panel.bar_family_wicks = inst.bar_family_wicks;
                panel.tpo_display.show_session_dates = inst.tpo_show_session_dates;
                panel.tpo_display.show_poc = inst.tpo_show_poc;
                panel.tpo_display.show_value_area = inst.tpo_show_value_area;
                panel.tpo_display.show_single_prints = inst.tpo_show_single_prints;
                panel.tpo_display.show_poor_hl = inst.tpo_show_poor_hl;
                panel.tpo_display.show_initial_balance = inst.tpo_show_initial_balance;
                panel.tpo_display.ib_periods = inst.tpo_ib_periods_label.parse::<u8>().unwrap_or(2);
                panel.line_family_display.line_width = inst.line_width_label.parse::<f64>().unwrap_or(2.0);
                panel.line_family_display.step_line_width = inst.step_line_width_label.parse::<f64>().unwrap_or(2.0);
                panel.line_family_display.area_line_width = inst.area_line_width_label.parse::<f64>().unwrap_or(2.0);
                panel.line_family_display.baseline_line_width = inst.baseline_line_width_label.parse::<f64>().unwrap_or(2.0);
                panel.line_family_display.marker_style = mlc_core::chart::LineMarkerStyle::from_id(
                    &inst.line_markers_style_label.to_lowercase(),
                );
                panel.line_family_display.marker_size = inst.line_markers_size_label.parse::<f64>().unwrap_or(3.0);
            }
        }

        // ── Scales & Lines settings ───────────────────────────────────────────
        if let Ok(scales) = serde_json::from_value::<ScalesLinesSettings>(tmpl.scales.clone()) {
            let active_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            if let Some(pid) = active_pid {
                let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                panel.grid_options.vert_lines.visible = scales.vert_lines;
                panel.grid_options.horz_lines.visible = scales.horz_lines;
            }
            if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                w.scale_settings.price_scale_width  = scales.price_scale_width;
                w.scale_settings.time_scale_height  = scales.time_scale_height;
                if scales.auto_scale {
                    w.set_scale_mode(mlc_core::ScaleMode::Auto);
                }
                // 24h format and day-of-week
                w.scale_settings.time_format.use_24h         = scales.use_24h;
                w.scale_settings.time_format.show_day_of_week = scales.show_day_of_week;
            }
        }

        // ── Status Line settings ──────────────────────────────────────────────
        // Status line fields are not yet wired to live window state;
        // they are captured in the snapshot for forward-compatibility.
        let _status: Result<StatusLineSettings, _> =
            serde_json::from_value(tmpl.status_line.clone());
    }

    /// Handle chart settings item clicks (checkboxes, toggles, dropdowns, color pickers).
    fn handle_chart_settings_item(&mut self, item_id: &str) {
        let screen_w = self.width as f64;
        let screen_h = self.height as f64;

        // ── Instrument tab ────────────────────────────────────────────────────
        if let Some(field) = item_id.strip_prefix("instrument:") {
            match field {
                "use_prev_close" => {
                    self.panel_app.chart_settings_state.instrument_use_prev_close =
                        !self.panel_app.chart_settings_state.instrument_use_prev_close;
                    eprintln!("[ChartApp] instrument:use_prev_close = {}", self.panel_app.chart_settings_state.instrument_use_prev_close);
                }
                "body_enabled" => {
                    self.panel_app.chart_settings_state.instrument_body_enabled =
                        !self.panel_app.chart_settings_state.instrument_body_enabled;
                    eprintln!("[ChartApp] instrument:body_enabled = {}", self.panel_app.chart_settings_state.instrument_body_enabled);
                }
                "border_enabled" => {
                    self.panel_app.chart_settings_state.instrument_border_enabled =
                        !self.panel_app.chart_settings_state.instrument_border_enabled;
                    eprintln!("[ChartApp] instrument:border_enabled = {}", self.panel_app.chart_settings_state.instrument_border_enabled);
                }
                "wick_enabled" => {
                    self.panel_app.chart_settings_state.instrument_wick_enabled =
                        !self.panel_app.chart_settings_state.instrument_wick_enabled;
                    eprintln!("[ChartApp] instrument:wick_enabled = {}", self.panel_app.chart_settings_state.instrument_wick_enabled);
                }
                "footprint_show_abs_markers" => {
                    let new_value = !self.panel_app.chart_settings_state.footprint_show_abs_markers;
                    self.panel_app.chart_settings_state.footprint_show_abs_markers = new_value;
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.footprint_display.show_abs_markers = new_value;
                    }
                    eprintln!("[ChartApp] instrument:footprint_show_abs_markers = {}", new_value);
                }
                "footprint_show_volume_text" => {
                    let new_value = !self.panel_app.chart_settings_state.footprint_show_volume_text;
                    self.panel_app.chart_settings_state.footprint_show_volume_text = new_value;
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.footprint_display.show_volume_text = new_value;
                    }
                    eprintln!("[ChartApp] instrument:footprint_show_volume_text = {}", new_value);
                }
                "tick_intensity" => {
                    let new_value = !self.panel_app.chart_settings_state.tick_intensity;
                    self.panel_app.chart_settings_state.tick_intensity = new_value;
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tick_intensity = new_value;
                    }
                    eprintln!("[ChartApp] instrument:tick_intensity = {}", new_value);
                }
                "bar_family_wicks" => {
                    let new_value = !self.panel_app.chart_settings_state.bar_family_wicks;
                    self.panel_app.chart_settings_state.bar_family_wicks = new_value;
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.bar_family_wicks = new_value;
                    }
                    eprintln!("[ChartApp] instrument:bar_family_wicks = {}", new_value);
                }
                "tpo_show_session_dates" => {
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tpo_display.show_session_dates = !panel.tpo_display.show_session_dates;
                        eprintln!("[ChartApp] instrument:tpo_show_session_dates = {}", panel.tpo_display.show_session_dates);
                    }
                }
                "tpo_show_poc" => {
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tpo_display.show_poc = !panel.tpo_display.show_poc;
                        eprintln!("[ChartApp] instrument:tpo_show_poc = {}", panel.tpo_display.show_poc);
                    }
                }
                "tpo_show_value_area" => {
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tpo_display.show_value_area = !panel.tpo_display.show_value_area;
                        eprintln!("[ChartApp] instrument:tpo_show_value_area = {}", panel.tpo_display.show_value_area);
                    }
                }
                "tpo_show_single_prints" => {
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tpo_display.show_single_prints = !panel.tpo_display.show_single_prints;
                        eprintln!("[ChartApp] instrument:tpo_show_single_prints = {}", panel.tpo_display.show_single_prints);
                    }
                }
                "tpo_show_poor_hl" => {
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tpo_display.show_poor_hl = !panel.tpo_display.show_poor_hl;
                        eprintln!("[ChartApp] instrument:tpo_show_poor_hl = {}", panel.tpo_display.show_poor_hl);
                    }
                }
                "tpo_show_initial_balance" => {
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tpo_display.show_initial_balance = !panel.tpo_display.show_initial_balance;
                        eprintln!("[ChartApp] instrument:tpo_show_initial_balance = {}", panel.tpo_display.show_initial_balance);
                    }
                }
                "use_24h" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.use_24h = !w.scale_settings.time_format.use_24h;
                        eprintln!("[ChartApp] instrument:use_24h = {}", w.scale_settings.time_format.use_24h);
                    }
                }
                "show_utc_prefix" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.show_utc_prefix = !w.scale_settings.time_format.show_utc_prefix;
                        eprintln!("[ChartApp] instrument:show_utc_prefix = {}", w.scale_settings.time_format.show_utc_prefix);
                    }
                }
                "date_format_cycle" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.date_format = w.scale_settings.time_format.date_format.next();
                        eprintln!("[ChartApp] instrument:date_format_cycle = {:?}", w.scale_settings.time_format.date_format);
                    }
                }
                "date_format_menu" => {
                    self.panel_app.chart_settings_state.active_dropdown =
                        if self.panel_app.chart_settings_state.active_dropdown.as_deref() == Some("instrument_date_format") {
                            None
                        } else {
                            Some("instrument_date_format".to_string())
                        };
                }
                "show_day_of_week" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.show_day_of_week = !w.scale_settings.time_format.show_day_of_week;
                        eprintln!("[ChartApp] instrument:show_day_of_week = {}", w.scale_settings.time_format.show_day_of_week);
                    }
                }
                "show_bar_countdown" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.show_bar_countdown = !w.scale_settings.show_bar_countdown;
                        eprintln!("[ChartApp] instrument:show_bar_countdown = {}", w.scale_settings.show_bar_countdown);
                    }
                }
                "price_tick_extend_right" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_tick_extend_right = !w.scale_settings.price_tick_extend_right;
                        eprintln!("[ChartApp] instrument:price_tick_extend_right = {}", w.scale_settings.price_tick_extend_right);
                    }
                }
                "price_tick_extend_left" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_tick_extend_left = !w.scale_settings.price_tick_extend_left;
                        eprintln!("[ChartApp] instrument:price_tick_extend_left = {}", w.scale_settings.price_tick_extend_left);
                    }
                }
                "body_up_color" | "body_down_color"
                | "border_up_color" | "border_down_color"
                | "wick_up_color" | "wick_down_color"
                | "footprint_buy_color" | "footprint_sell_color"
                | "renko_up_color" | "renko_down_color"
                | "pnf_x_color" | "pnf_o_color"
                | "tlb_up_color" | "tlb_down_color"
                | "kagi_yang_color" | "kagi_yin_color"
                | "hollow_up_color" | "hollow_down_color"
                | "line_color" | "step_line_color"
                | "area_line_color" | "area_top_color" | "area_bottom_color"
                | "baseline_top_line_color" | "baseline_top_fill_color"
                | "baseline_bottom_line_color" | "baseline_bottom_fill_color"
                | "bars_up_color" | "bars_down_color"
                | "line_markers_color" => {
                    let current_color: Option<String> = {
                        let s = &self.panel_app.theme_manager.current().series;
                        match field {
                            "body_up_color"     => Some(s.candle_up_body.clone()),
                            "body_down_color"   => Some(s.candle_down_body.clone()),
                            "border_up_color"   => s.candle_up_border.clone().or_else(|| Some(s.candle_up_body.clone())),
                            "border_down_color" => s.candle_down_border.clone().or_else(|| Some(s.candle_down_body.clone())),
                            "wick_up_color"     => Some(s.candle_up_wick.clone()),
                            "wick_down_color"   => Some(s.candle_down_wick.clone()),
                            "footprint_buy_color"  => Some(s.footprint_buy.clone()),
                            "footprint_sell_color" => Some(s.footprint_sell.clone()),
                            "renko_up_color"    => Some(s.renko_up.clone()),
                            "renko_down_color"  => Some(s.renko_down.clone()),
                            "pnf_x_color"       => Some(s.pnf_x.clone()),
                            "pnf_o_color"       => Some(s.pnf_o.clone()),
                            "tlb_up_color"      => Some(s.tlb_up.clone()),
                            "tlb_down_color"    => Some(s.tlb_down.clone()),
                            "kagi_yang_color"   => Some(s.kagi_yang.clone()),
                            "kagi_yin_color"    => Some(s.kagi_yin.clone()),
                            "hollow_up_color"   => Some(s.hollow_up.clone()),
                            "hollow_down_color" => Some(s.hollow_down.clone()),
                            "line_color"        => Some(s.line_color.clone()),
                            "step_line_color"   => Some(s.step_line_color.clone()),
                            "area_line_color"   => Some(s.area_line.clone()),
                            "area_top_color"    => Some(s.area_top.clone()),
                            "area_bottom_color" => Some(s.area_bottom.clone()),
                            "baseline_top_line_color"    => Some(s.baseline_top_line.clone()),
                            "baseline_top_fill_color"    => Some(s.baseline_top_fill.clone()),
                            "baseline_bottom_line_color" => Some(s.baseline_bottom_line.clone()),
                            "baseline_bottom_fill_color" => Some(s.baseline_bottom_fill.clone()),
                            "bars_up_color"     => Some(s.bar_up.clone()),
                            "bars_down_color"   => Some(s.bar_down.clone()),
                            "line_markers_color" => Some(s.line_markers_color.clone()),
                            _ => None,
                        }
                    };
                    let widget_id_str = format!("chart_settings:item:{}", item_id);
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                    let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                    // Store the field name (without prefix) in color_picker_field
                    self.panel_app.chart_settings_state.open_color_picker_smart(
                        field, ax, ay, aw, ah, screen_w, screen_h, current_color.as_deref(),
                    );
                    eprintln!("[ChartApp] chart_settings opened color picker for: {}", field);
                }
                _ => {
                    eprintln!("[ChartApp] chart_settings instrument unknown: {}", field);
                }
            }
            return;
        }

        // ── Scales tab ────────────────────────────────────────────────────────
        if let Some(field) = item_id.strip_prefix("scales:") {
            match field {
                "show_grid" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        let current = panel.grid_options.vert_lines.visible && panel.grid_options.horz_lines.visible;
                        panel.grid_options.vert_lines.visible = !current;
                        panel.grid_options.horz_lines.visible = !current;
                        eprintln!("[ChartApp] scales:show_grid = {}", !current);
                    }
                }
                "vert_lines" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.grid_options.vert_lines.visible = !panel.grid_options.vert_lines.visible;
                        eprintln!("[ChartApp] scales:vert_lines = {}", panel.grid_options.vert_lines.visible);
                    }
                }
                "horz_lines" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.grid_options.horz_lines.visible = !panel.grid_options.horz_lines.visible;
                        eprintln!("[ChartApp] scales:horz_lines = {}", panel.grid_options.horz_lines.visible);
                    }
                }
                "price_scale_right" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_scale_position = match w.scale_settings.price_scale_position {
                            PriceScalePosition::Hidden => PriceScalePosition::Right,
                            _ => PriceScalePosition::Hidden,
                        };
                        eprintln!("[ChartApp] scales:price_scale_right toggled: {:?}", w.scale_settings.price_scale_position);
                    }
                }
                "time_scale_bottom" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_scale_position = match w.scale_settings.time_scale_position {
                            TimeScalePosition::Hidden => TimeScalePosition::Bottom,
                            _ => TimeScalePosition::Hidden,
                        };
                        eprintln!("[ChartApp] scales:time_scale_bottom toggled: {:?}", w.scale_settings.time_scale_position);
                    }
                }
                "auto_scale" => {
                    let next = self.panel_app.panel_grid
                        .active_container()
                        .map(|w| w.price_scale().scale_mode.next());
                    if let Some(next) = next {
                        let auto_series = self.active_chart_active_series();
                        if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                            w.set_scale_mode_with_recalc(next, auto_series.as_active_series());
                            eprintln!("[ChartApp] scales:auto_scale -> {:?}", next);
                        }
                        // Propagate scale_mode change to sync-group peers.
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            let viewport_state = self.panel_app.panel_grid
                                .active_container()
                                .map(|w| (w.viewport().view_start, w.viewport().bar_spacing));
                            if let Some((view_start, bar_spacing)) = viewport_state {
                                self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, Some(next));
                            }
                        }
                    }
                }
                "show_bar_countdown" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.show_bar_countdown = !w.scale_settings.show_bar_countdown;
                        eprintln!("[ChartApp] scales:show_bar_countdown = {}", w.scale_settings.show_bar_countdown);
                    }
                }
                "show_prev_close" => {
                    let pc_bars: Vec<mlc_core::Bar> = self.active_chart_bars();
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.show_prev_close_line = !w.scale_settings.show_prev_close_line;
                        w.update_prev_close_line(&pc_bars);
                        eprintln!("[ChartApp] scales:show_prev_close = {}", w.scale_settings.show_prev_close_line);
                    }
                }
                "use_24h" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.use_24h = !w.scale_settings.time_format.use_24h;
                        eprintln!("[ChartApp] scales:use_24h = {}", w.scale_settings.time_format.use_24h);
                    }
                }
                "show_day_of_week" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.show_day_of_week = !w.scale_settings.time_format.show_day_of_week;
                        eprintln!("[ChartApp] scales:show_day_of_week = {}", w.scale_settings.time_format.show_day_of_week);
                    }
                }
                "crosshair_line_color" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    let current_color: Option<String> = pid
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .map(|p| p.crosshair_options.vert_line.color.clone());
                    let widget_id_str = format!("chart_settings:item:{}", item_id);
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                    let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                    self.panel_app.chart_settings_state.open_color_picker_smart(
                        "crosshair_line_color", ax, ay, aw, ah, screen_w, screen_h, current_color.as_deref(),
                    );
                    eprintln!("[ChartApp] chart_settings opened color picker for crosshair_line_color");
                }
                "price_width_increase" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        let new_w = (w.scale_settings.price_scale_width + 10.0).min(150.0);
                        w.scale_settings.price_scale_width = new_w;
                        eprintln!("[ChartApp] price_scale_width = {}", new_w);
                    }
                }
                "price_width_decrease" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        let new_w = (w.scale_settings.price_scale_width - 10.0).max(50.0);
                        w.scale_settings.price_scale_width = new_w;
                        eprintln!("[ChartApp] price_scale_width = {}", new_w);
                    }
                }
                "time_height_increase" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        let new_h = (w.scale_settings.time_scale_height + 5.0).min(60.0);
                        w.scale_settings.time_scale_height = new_h;
                        eprintln!("[ChartApp] time_scale_height = {}", new_h);
                    }
                }
                "time_height_decrease" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        let new_h = (w.scale_settings.time_scale_height - 5.0).max(20.0);
                        w.scale_settings.time_scale_height = new_h;
                        eprintln!("[ChartApp] time_scale_height = {}", new_h);
                    }
                }
                "dropdown_cycle:date_format" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.date_format = w.scale_settings.time_format.date_format.next();
                        eprintln!("[ChartApp] scales:date_format cycled: {:?}", w.scale_settings.time_format.date_format);
                    }
                }
                "dropdown_menu:date_format" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("date_format");
                }
                _ => {
                    eprintln!("[ChartApp] chart_settings scales unknown: {}", field);
                }
            }
            return;
        }

        // ── Status Line tab ───────────────────────────────────────────────────
        if let Some(field) = item_id.strip_prefix("status:") {
            match field {
                "dropdown_cycle:legend_position" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.legend.position = match panel.legend.position {
                            LegendPosition::TopLeft    => LegendPosition::TopRight,
                            LegendPosition::TopRight   => LegendPosition::BottomRight,
                            LegendPosition::BottomRight => LegendPosition::BottomLeft,
                            LegendPosition::BottomLeft  => LegendPosition::TopLeft,
                        };
                        eprintln!("[ChartApp] legend position cycled: {:?}", panel.legend.position);
                    }
                }
                "dropdown_menu:legend_position" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("legend_position");
                }
                "legend_position" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.legend.position = match panel.legend.position {
                            LegendPosition::TopLeft    => LegendPosition::TopRight,
                            LegendPosition::TopRight   => LegendPosition::BottomLeft,
                            LegendPosition::BottomLeft  => LegendPosition::BottomRight,
                            LegendPosition::BottomRight => LegendPosition::TopLeft,
                        };
                    }
                }
                "legend_show_ohlc" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.legend.show_ohlc = !panel.legend.show_ohlc;
                        eprintln!("[ChartApp] legend_show_ohlc = {}", panel.legend.show_ohlc);
                    }
                }
                "legend_show_change" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.legend.show_change = !panel.legend.show_change;
                        eprintln!("[ChartApp] legend_show_change = {}", panel.legend.show_change);
                    }
                }
                "legend_show_percent" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.legend.show_percent = !panel.legend.show_percent;
                        eprintln!("[ChartApp] legend_show_percent = {}", panel.legend.show_percent);
                    }
                }
                "tooltip_visible" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tooltip.visible = !panel.tooltip.visible;
                        eprintln!("[ChartApp] tooltip_visible = {}", panel.tooltip.visible);
                    }
                }
                "tooltip_follow" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tooltip.follow_cursor = !panel.tooltip.follow_cursor;
                        eprintln!("[ChartApp] tooltip_follow = {}", panel.tooltip.follow_cursor);
                    }
                }
                "dropdown_cycle:watermark_position" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        if let Some(ref mut watermark) = panel.watermark {
                            match (watermark.horz_align, watermark.vert_align) {
                                (HorzAlign::Left, VertAlign::Top) => { watermark.horz_align = HorzAlign::Right; watermark.vert_align = VertAlign::Top; }
                                (HorzAlign::Right, VertAlign::Top) => { watermark.horz_align = HorzAlign::Right; watermark.vert_align = VertAlign::Bottom; }
                                (HorzAlign::Right, VertAlign::Bottom) => { watermark.horz_align = HorzAlign::Left; watermark.vert_align = VertAlign::Bottom; }
                                (HorzAlign::Left, VertAlign::Bottom) => { watermark.horz_align = HorzAlign::Center; watermark.vert_align = VertAlign::Center; }
                                _ => { watermark.horz_align = HorzAlign::Left; watermark.vert_align = VertAlign::Top; }
                            }
                            eprintln!("[ChartApp] watermark position cycled: {:?} {:?}", watermark.horz_align, watermark.vert_align);
                        }
                    }
                }
                "dropdown_menu:watermark_position" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("watermark_position");
                }
                "watermark_position" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        if let Some(ref mut watermark) = panel.watermark {
                            match (watermark.horz_align, watermark.vert_align) {
                                (HorzAlign::Left, VertAlign::Top) => { watermark.horz_align = HorzAlign::Right; watermark.vert_align = VertAlign::Top; }
                                (HorzAlign::Right, VertAlign::Top) => { watermark.horz_align = HorzAlign::Left; watermark.vert_align = VertAlign::Bottom; }
                                (HorzAlign::Left, VertAlign::Bottom) => { watermark.horz_align = HorzAlign::Right; watermark.vert_align = VertAlign::Bottom; }
                                (HorzAlign::Right, VertAlign::Bottom) => { watermark.horz_align = HorzAlign::Center; watermark.vert_align = VertAlign::Center; }
                                _ => { watermark.horz_align = HorzAlign::Left; watermark.vert_align = VertAlign::Top; }
                            }
                        }
                    }
                }
                "watermark_visible" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        if let Some(ref mut watermark) = panel.watermark {
                            watermark.visible = !watermark.visible;
                            eprintln!("[ChartApp] watermark_visible = {}", watermark.visible);
                        }
                    }
                }
                "watermark_color" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    let current_color: Option<String> = pid
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|p| p.watermark.as_ref())
                        .and_then(|wm| wm.lines.first())
                        .map(|line| line.color.clone());
                    let widget_id_str = format!("chart_settings:item:{}", item_id);
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                    let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                    self.panel_app.chart_settings_state.open_color_picker_smart(
                        "watermark_color", ax, ay, aw, ah, screen_w, screen_h, current_color.as_deref(),
                    );
                    eprintln!("[ChartApp] chart_settings opened color picker for watermark_color");
                }
                "watermark_text" => {
                    let pid2 = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    let current_text = pid2
                        .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                        .and_then(|p| p.watermark.as_ref())
                        .and_then(|wm| wm.lines.first())
                        .map(|line| line.text.clone())
                        .unwrap_or_default();
                    let wid = uzor::WidgetId::from(crate::text_input::CHART_SETTINGS_FIELD);
                    self.panel_app.chart_settings_state.active_field_id = Some("status:watermark_text".to_string());
                    let mut coord = self.input_coordinator.borrow_mut();
                    coord.text_fields_mut().set_text(&wid, &current_text);
                    coord.text_fields_mut().focus(wid.clone());
                    if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
                        fs.selection_start = Some(0);
                    }
                    eprintln!("[ChartApp] chart_settings watermark_text editing started");
                }
                "show_indicator_overlay" => {
                    self.panel_app.indicator_overlay_state.visible = !self.panel_app.indicator_overlay_state.visible;
                    eprintln!("[ChartApp] indicator_overlay visible = {}", self.panel_app.indicator_overlay_state.visible);
                    self.autosave_snapshot();
                }
                _ => {
                    eprintln!("[ChartApp] chart_settings status unknown: {}", field);
                }
            }
            return;
        }

        // ── Appearance tab ────────────────────────────────────────────────────
        if let Some(field) = item_id.strip_prefix("appearance:") {
            // Theme preset buttons
            if let Some(theme_id) = field.strip_prefix("theme_") {
                self.panel_app.theme_manager.set_preset(theme_id);
                // Signal the App-level coordinator to propagate this change to all windows.
                self.theme_changed = Some(theme_id.to_string());
                eprintln!("[ChartApp] theme preset: {}", theme_id);
                return;
            }

            // UI Style buttons
            if let Some(ui_style_str) = field.strip_prefix("ui_style:") {
                let style_index = ui_style_str.parse::<usize>().unwrap_or(0);
                if let Some(style) = UIStyle::from_index(style_index) {
                    self.panel_app.theme_manager.current_mut().set_style(style);
                    eprintln!("[ChartApp] UI style: {:?}", style);
                }
                return;
            }

            // Color swatch
            let current_color = ThemeSettingsPanel::get_color_by_id(
                self.panel_app.theme_manager.current(),
                field,
            );
            if let Some(color) = current_color {
                let widget_id_str = format!("chart_settings:item:{}", item_id);
                let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                self.panel_app.chart_settings_state.open_color_picker_smart(
                    field, ax, ay, aw, ah, screen_w, screen_h, Some(color),
                );
                eprintln!("[ChartApp] chart_settings opened color picker for appearance:{}", field);
            } else {
                eprintln!("[ChartApp] chart_settings appearance unknown: {}", field);
            }
            return;
        }

        // ── Dropdown option selection ─────────────────────────────────────────
        if let Some(rest) = item_id.strip_prefix("dropdown_option:") {
            let mut parts = rest.splitn(2, ':');
            let field = parts.next().unwrap_or_default().to_string();
            let value = parts.next().unwrap_or_default().to_string();

            match field.as_str() {
                "crosshair_mode" => {
                    let new_mode = match value.as_str() {
                        "Normal" => CrosshairMode::Normal,
                        "Magnet" => CrosshairMode::Magnet,
                        "MagnetOHLC" => CrosshairMode::MagnetOHLC,
                        "Hidden" => CrosshairMode::Hidden,
                        _ => CrosshairMode::Normal,
                    };
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.crosshair.mode = new_mode;
                    }
                    eprintln!("[ChartApp] crosshair_mode = {:?}", new_mode);
                }
                "crosshair_line_style" => {
                    use mlc_core::drawing::primitives_v2::LineStyle;
                    let new_style = match value.as_str() {
                        "Solid" => LineStyle::Solid,
                        "Dashed" => LineStyle::Dashed,
                        "Dotted" => LineStyle::Dotted,
                        "LargeDashed" => LineStyle::LargeDashed,
                        "SparseDotted" => LineStyle::SparseDotted,
                        _ => LineStyle::Solid,
                    };
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.crosshair_options.vert_line.style = new_style;
                        panel.crosshair_options.horz_line.style = new_style;
                    }
                    eprintln!("[ChartApp] crosshair_line_style = {:?}", new_style);
                }
                "price_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_scale_position = match value.as_str() {
                            "left"   => PriceScalePosition::Left,
                            "right"  => PriceScalePosition::Right,
                            "hidden" => PriceScalePosition::Hidden,
                            _ => PriceScalePosition::Right,
                        };
                        eprintln!("[ChartApp] price_scale_position = {:?}", w.scale_settings.price_scale_position);
                    }
                }
                "time_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_scale_position = match value.as_str() {
                            "top"    => TimeScalePosition::Top,
                            "bottom" => TimeScalePosition::Bottom,
                            "hidden" => TimeScalePosition::Hidden,
                            _ => TimeScalePosition::Bottom,
                        };
                        eprintln!("[ChartApp] time_scale_position = {:?}", w.scale_settings.time_scale_position);
                    }
                }
                "corner_visibility" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.corner_visibility = match value.as_str() {
                            "always" => ScaleCornerVisibility::Always,
                            "never"  => ScaleCornerVisibility::Never,
                            _ => ScaleCornerVisibility::Always,
                        };
                        eprintln!("[ChartApp] corner_visibility = {:?}", w.scale_settings.corner_visibility);
                    }
                }
                "price_tick_style" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_tick_style = match value.as_str() {
                            "dashed" => "dashed".to_string(),
                            "solid"  => "solid".to_string(),
                            _        => "dotted".to_string(),
                        };
                        eprintln!("[ChartApp] price_tick_style = {}", w.scale_settings.price_tick_style);
                    }
                }
                "date_format" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.date_format = match value.as_str() {
                            "day_month_year" | "day_month_year_dots" => DateFormat::DayMonthYear,
                            "month_day_year" | "day_month_year_slash" => DateFormat::MonthDayYear,
                            "day_month_short" | "day_month_full" => DateFormat::DayMonthShort,
                            _ => DateFormat::YearMonthDay,
                        };
                        eprintln!("[ChartApp] date_format = {:?}", w.scale_settings.time_format.date_format);
                    }
                }
                "legend_position" => {
                    let new_pos = match value.as_str() {
                        "top_left"     => LegendPosition::TopLeft,
                        "top_right"    => LegendPosition::TopRight,
                        "bottom_left"  => LegendPosition::BottomLeft,
                        "bottom_right" => LegendPosition::BottomRight,
                        _ => LegendPosition::TopLeft,
                    };
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid).legend.position = new_pos;
                    }
                    eprintln!("[ChartApp] legend_position = {:?}", new_pos);
                }
                "watermark_position" => {
                    let (horz, vert) = match value.as_str() {
                        "top_left"     => (HorzAlign::Left,   VertAlign::Top),
                        "top_right"    => (HorzAlign::Right,  VertAlign::Top),
                        "bottom_left"  => (HorzAlign::Left,   VertAlign::Bottom),
                        "bottom_right" => (HorzAlign::Right,  VertAlign::Bottom),
                        "center"       => (HorzAlign::Center, VertAlign::Center),
                        _ => (HorzAlign::Left, VertAlign::Top),
                    };
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        if let Some(ref mut watermark) = panel.watermark {
                            watermark.horz_align = horz;
                            watermark.vert_align = vert;
                            eprintln!("[ChartApp] watermark_position = {:?} {:?}", horz, vert);
                        }
                    }
                }
                "chart_type_param" => {
                    // Instrument-tab top slot: preset dropdown for the active
                    // derived chart type's single editable numeric parameter.
                    // Applies immediately through the same event the (dead)
                    // toolbar param field was meant to fire — see recon
                    // 2026-07-05 §3. Footprint's bucket interval is NOT part
                    // of this catalog (duplicate of the toolbar TF picker,
                    // removed 2026-07-05) so every remaining entry is numeric.
                    use mlc_app_layout::layout::modals::chart_settings::chart_type_param_spec;
                    let active_chart_type: &'static str = self.panel_app.panel_grid
                        .active_container_id()
                        .and_then(|cid| self.chart_type_for_chart(cid))
                        .unwrap_or("candles");
                    if chart_type_param_spec(active_chart_type).is_some() {
                        if let Ok(v) = value.parse::<f64>() {
                            self.process_chart_out_event(
                                mlc_core::events::ChartOutEvent::UpdateChartTypeParam { value: v }
                            );
                        }
                    }
                    eprintln!("[ChartApp] chart_type_param = {} (chart_type={})", value, active_chart_type);
                }
                "footprint_heat_intensity" => {
                    let intensity = mlc_core::chart::FootprintHeatIntensity::from_id(&value.to_lowercase());
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.footprint_display.heat_intensity = intensity;
                    }
                    eprintln!("[ChartApp] instrument:footprint_heat_intensity = {}", value);
                }
                "footprint_row_grouping" => {
                    // Row-grouping preset (0/2/5/10 ticks, canon-fix wave 2
                    // item 5) — reseeds the footprint fold on change, same
                    // as any other setting that reaches `FoldParams` (mirrors
                    // `UpdateChartTypeParam`'s resubscribe path).
                    let ticks: u32 = value.parse().unwrap_or(0);
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.footprint_display.group_ticks = ticks;
                    }
                    self.process_chart_out_event(
                        mlc_core::events::ChartOutEvent::UpdateFootprintGroupTicks { ticks }
                    );
                    eprintln!("[ChartApp] instrument:footprint_row_grouping = {} ticks", ticks);
                }
                "footprint_imbalance_threshold" => {
                    // Stacked-imbalance cell ratio preset (1.5x/2x/3x/4x,
                    // owner-approved settings matrix wave 1 item 8) — pure
                    // render-gate value, no resubscribe needed (unlike
                    // `footprint_row_grouping`, which reshapes the fold).
                    let ratio: f64 = value.trim_end_matches('x').parse().unwrap_or(1.5);
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.footprint_display.imbalance_threshold = ratio;
                    }
                    eprintln!("[ChartApp] instrument:footprint_imbalance_threshold = {}x", ratio);
                }
                "footprint_cell_mode" => {
                    // Cell layout mode (doctrine §8) — pure render-gate
                    // value, no resubscribe needed. Dropdown option value
                    // IS the exact UI label (`from_label`), same shape as
                    // `footprint_heat_intensity`'s id-based lookup but
                    // keyed on the full label since ids are multi-word.
                    let mode = mlc_core::chart::FootprintCellMode::from_label(&value);
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.footprint_display.cell_mode = mode;
                    }
                    eprintln!("[ChartApp] instrument:footprint_cell_mode = {}", value);
                }
                "tpo_ib_periods" => {
                    let periods: u8 = value.parse().unwrap_or(2);
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.tpo_display.ib_periods = periods;
                    }
                    eprintln!("[ChartApp] instrument:tpo_ib_periods = {}", periods);
                }
                "line_width" | "step_line_width" | "area_line_width" | "baseline_line_width" => {
                    // Simple line-family per-type width presets (owner-
                    // approved settings matrix wave 3, 2026-07-11) — pure
                    // render-gate value, no resubscribe needed.
                    let px: f64 = value.parse().unwrap_or(2.0);
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        match field.as_str() {
                            "line_width" => panel.line_family_display.line_width = px,
                            "step_line_width" => panel.line_family_display.step_line_width = px,
                            "area_line_width" => panel.line_family_display.area_line_width = px,
                            "baseline_line_width" => panel.line_family_display.baseline_line_width = px,
                            _ => {}
                        }
                    }
                    eprintln!("[ChartApp] instrument:{} = {}px", field, px);
                }
                "line_markers_style" => {
                    let style = mlc_core::chart::LineMarkerStyle::from_id(&value.to_lowercase());
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.line_family_display.marker_style = style;
                    }
                    eprintln!("[ChartApp] instrument:line_markers_style = {}", value);
                }
                "line_markers_size" => {
                    let px: f64 = value.parse().unwrap_or(3.0);
                    if let Some(pid) = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id) {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.line_family_display.marker_size = px;
                    }
                    eprintln!("[ChartApp] instrument:line_markers_size = {}px", px);
                }
                "candle_style" => {
                    // Skin texture layer manual pick — sets the candle
                    // draw-form on the runtime theme; glow follows.
                    use mlc_core::theme::CandleStyle;
                    let style = match value.as_str() {
                        "thin_glow" => CandleStyle::ThinGlow,
                        "phosphor" => CandleStyle::Phosphor,
                        _ => CandleStyle::Solid,
                    };
                    let rt = self.panel_app.theme_manager.current_mut();
                    rt.panel_style.candle_style = style;
                    rt.panel_style.glow = style != CandleStyle::Solid;
                    eprintln!("[ChartApp] instrument:candle_style = {:?}", style);
                }
                _ => {
                    eprintln!("[ChartApp] chart_settings dropdown_option unknown field: {}", field);
                }
            }
            self.panel_app.chart_settings_state.active_dropdown = None;
            return;
        }

        // ── Top-level dropdown controls ───────────────────────────────────────
        if let Some(name) = item_id.strip_prefix("dropdown_cycle:") {
            match name {
                "precision" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.user_precision = cycle_precision(w.scale_settings.user_precision);
                        w.set_price_precision(w.scale_settings.user_precision);
                        eprintln!("[ChartApp] precision cycled: {:?}", w.scale_settings.user_precision);
                    }
                }
                "timezone" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_format.cycle_timezone();
                        eprintln!("[ChartApp] timezone cycled: {}", w.scale_settings.time_format.timezone_label());
                    }
                }
                "crosshair_mode" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        let new_mode = match w.crosshair.mode {
                            CrosshairMode::Normal    => CrosshairMode::Magnet,
                            CrosshairMode::Magnet    => CrosshairMode::MagnetOHLC,
                            CrosshairMode::MagnetOHLC => CrosshairMode::Hidden,
                            CrosshairMode::Hidden    => CrosshairMode::Normal,
                        };
                        w.crosshair.mode = new_mode;
                        eprintln!("[ChartApp] crosshair_mode cycled: {:?}", new_mode);
                    }
                }
                "crosshair_line_style" => {
                    use mlc_core::drawing::primitives_v2::LineStyle;
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        let new_style = match panel.crosshair_options.vert_line.style {
                            LineStyle::Solid       => LineStyle::Dashed,
                            LineStyle::Dashed      => LineStyle::Dotted,
                            LineStyle::Dotted      => LineStyle::LargeDashed,
                            LineStyle::LargeDashed => LineStyle::SparseDotted,
                            LineStyle::SparseDotted => LineStyle::Solid,
                        };
                        panel.crosshair_options.vert_line.style = new_style;
                        panel.crosshair_options.horz_line.style = new_style;
                        eprintln!("[ChartApp] crosshair_line_style cycled: {:?}", new_style);
                    }
                }
                "price_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_scale_position = w.scale_settings.price_scale_position.next();
                        eprintln!("[ChartApp] price_position cycled: {:?}", w.scale_settings.price_scale_position);
                    }
                }
                "time_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_scale_position = w.scale_settings.time_scale_position.next();
                        eprintln!("[ChartApp] time_position cycled: {:?}", w.scale_settings.time_scale_position);
                    }
                }
                "corner_visibility" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.corner_visibility = w.scale_settings.corner_visibility.next();
                        eprintln!("[ChartApp] corner_visibility cycled: {:?}", w.scale_settings.corner_visibility);
                    }
                }
                "price_tick_style" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_tick_style = match w.scale_settings.price_tick_style.as_str() {
                            "dotted" => "dashed".to_string(),
                            "dashed" => "solid".to_string(),
                            _        => "dotted".to_string(),
                        };
                        eprintln!("[ChartApp] price_tick_style cycled: {}", w.scale_settings.price_tick_style);
                    }
                }
                "chart_type_param" => {
                    // Unlike the fixed enum values above (precision, crosshair
                    // mode, ...), the preset list here is per-chart-type and
                    // not meaningfully "next"-cyclable — clicking the text
                    // area opens the picker instead, same as the chevron.
                    self.panel_app.chart_settings_state.toggle_dropdown("chart_type_param");
                }
                "footprint_heat_intensity" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("footprint_heat_intensity");
                }
                "footprint_row_grouping" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("footprint_row_grouping");
                }
                "footprint_imbalance_threshold" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("footprint_imbalance_threshold");
                }
                "footprint_cell_mode" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("footprint_cell_mode");
                }
                "tpo_ib_periods" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("tpo_ib_periods");
                }
                "line_width" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("line_width");
                }
                "step_line_width" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("step_line_width");
                }
                "area_line_width" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("area_line_width");
                }
                "baseline_line_width" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("baseline_line_width");
                }
                "line_markers_style" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("line_markers_style");
                }
                "line_markers_size" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("line_markers_size");
                }
                "candle_style" => {
                    // Skin texture layer manual switch — cycles the candle
                    // draw-form on the runtime theme; glow follows (Solid =
                    // no glow, ThinGlow/Phosphor = glow on).
                    use mlc_core::theme::CandleStyle;
                    let rt = self.panel_app.theme_manager.current_mut();
                    let next = match rt.panel_style.candle_style {
                        CandleStyle::Solid => CandleStyle::ThinGlow,
                        CandleStyle::ThinGlow => CandleStyle::Phosphor,
                        CandleStyle::Phosphor => CandleStyle::Solid,
                    };
                    rt.panel_style.candle_style = next;
                    rt.panel_style.glow = next != CandleStyle::Solid;
                    eprintln!("[ChartApp] candle_style cycled: {:?}", next);
                }
                _ => {
                    eprintln!("[ChartApp] chart_settings dropdown_cycle unknown: {}", name);
                }
            }
            return;
        }

        if let Some(name) = item_id.strip_prefix("dropdown_menu:") {
            self.panel_app.chart_settings_state.toggle_dropdown(name);
            eprintln!("[ChartApp] chart_settings dropdown_menu toggled: {}", name);
            return;
        }

        // ── Footer ────────────────────────────────────────────────────────────
        match item_id {
            "footer:ok" | "footer:cancel" => {
                self.panel_app.chart_settings_state.close();
                eprintln!("[ChartApp] chart_settings closed via: {}", item_id);
            }
            _ => {
                eprintln!("[ChartApp] chart_settings item unhandled: {}", item_id);
            }
        }
    }

    /// Handle clicks on widgets registered with the "prim_settings:" prefix.
    pub(super) fn handle_prim_settings_click(&mut self, rest: &str, x: f64, y: f64) {
        use mlc_app_layout::ui::modal_settings::PrimitiveSettingsTab;

        // Auto-commit: if an input field is being edited and the click lands on a
        // DIFFERENT widget inside the same modal, commit the value (same as Enter).
        if self.panel_app.primitive_settings_state.active_field_id.is_some() {
            let editing_field = self.panel_app.primitive_settings_state.active_field_id
                .as_deref()
                .unwrap_or_default()
                .to_string();
            // Determine if the click targets the *same* input field being edited.
            let click_targets_same_field = rest
                .strip_prefix("item:")
                .map(|item_id| item_id == editing_field)
                .unwrap_or(false);
            if !click_targets_same_field {
                self.commit_prim_editing_text();
            }
        }

        match rest {
            "close" => {
                self.panel_app.primitive_settings_state.close();
            }
            "modal_bg" => {
                // Click inside modal body — close template dropdown if open
                self.panel_app.primitive_settings_state.template_dropdown_open = false;
            }
            _ if rest.starts_with("tab:") => {
                self.panel_app.primitive_settings_state.template_dropdown_open = false;
                let tab_id = &rest["tab:".len()..];
                let tab = match tab_id {
                    "style"       => Some(PrimitiveSettingsTab::Style),
                    "text"        => Some(PrimitiveSettingsTab::Text),
                    "coordinates" => Some(PrimitiveSettingsTab::Coordinates),
                    "levels"      => Some(PrimitiveSettingsTab::Levels),
                    "visibility"  => Some(PrimitiveSettingsTab::Visibility),
                    _ => None,
                };
                if let Some(t) = tab {
                    self.panel_app.primitive_settings_state.set_tab(t);
                    eprintln!("[ChartApp] prim_settings tab: {}", tab_id);
                }
            }
            _ if rest.starts_with("item:") => {
                let item_id = &rest["item:".len()..];
                // Click-to-cursor: if the clicked field is already being edited,
                // reposition cursor using active_input_char_positions instead of restarting.
                let already_editing = self.panel_app.primitive_settings_state.active_field_id
                    .as_deref() == Some(item_id);
                if already_editing {
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.primitive_settings.as_ref())
                        .map(|ps| ps.active_input_char_positions.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let wid = uzor::WidgetId::from(crate::text_input::PRIMITIVE_SETTINGS_FIELD);
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        let mut coord = self.input_coordinator.borrow_mut();
                        if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                        return;
                    }
                }
                self.handle_prim_settings_item(item_id, x, y);
            }
            _ => {
                eprintln!("[ChartApp] prim_settings unhandled: {}", rest);
            }
        }
    }

    /// Commit whatever text is currently being edited in the primitive settings modal.
    ///
    /// This applies the same logic as the Enter-key handler in `on_char_input`, but
    /// is callable from click handlers so that clicking a different widget inside the
    /// same modal auto-commits the in-progress value.
    pub(crate) fn commit_prim_editing_text(&mut self) {
        let field = match self.panel_app.primitive_settings_state.active_field_id.take() {
            Some(f) => f,
            None => return,
        };
        let wid = uzor::WidgetId::from(crate::text_input::PRIMITIVE_SETTINGS_FIELD);
        let text = self.input_coordinator.borrow().text_fields().text(&wid).to_string();
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        let active_bid_prim = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);

        if field == "text_content" {
            if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                if let Some(bid) = active_bid_prim {
                    if let Some(mut data) = self.core.drawings().get_data(bid, prim_id) {
                        if let Some(ref mut t) = data.text {
                            t.content = text.clone();
                        } else {
                            use mlc_core::drawing::primitives_v2::PrimitiveText;
                            data.text = Some(PrimitiveText {
                                content: text.clone(),
                                ..PrimitiveText::default()
                            });
                        }
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                }
            }
            self.sync_drawing_back_to_group();
            eprintln!("[ChartApp] prim_settings text_content auto-committed: {}", text);
        } else if field == "stroke_width_value" || field == "stroke_width" {
            if let Ok(width) = text.trim().parse::<f64>() {
                if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                    if let Some(bid) = active_bid_prim {
                        if let Some(mut data) = self.core.drawings().get_data(bid, prim_id) {
                            data.width = width.clamp(0.5, 20.0);
                            self.core.drawings_mut().set_data(bid, prim_id, &data);
                        }
                    }
                }
            }
            self.sync_drawing_back_to_group();
        } else if field == "text_font_size" {
            if let Ok(font_size) = text.trim().parse::<f64>() {
                if let (Some(bid), Some(prim_id)) = (active_bid_prim, self.panel_app.primitive_settings_state.primitive_id) {
                    if let Some(mut data) = self.core.drawings().get_data(bid, prim_id) {
                        if let Some(ref mut t) = data.text {
                            t.font_size = font_size.clamp(8.0, 72.0);
                        } else {
                            use mlc_core::drawing::primitives_v2::PrimitiveText;
                            let mut t = PrimitiveText::default();
                            t.font_size = font_size.clamp(8.0, 72.0);
                            data.text = Some(t);
                        }
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                }
            }
            eprintln!("[ChartApp] prim_settings text_font_size auto-committed: {}", text);
        } else if field.starts_with("tf_") && field.ends_with("_min") {
            if let Ok(val) = text.trim().parse::<u32>() {
                if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                    if let Some(tf_idx) = field.strip_prefix("tf_")
                        .and_then(|s| s.strip_suffix("_min"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        self.apply_tf_min_value(prim_id, tf_idx, val);
                    }
                }
            }
        } else if field.starts_with("tf_") && field.ends_with("_max") {
            if let Ok(val) = text.trim().parse::<u32>() {
                if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                    if let Some(tf_idx) = field.strip_prefix("tf_")
                        .and_then(|s| s.strip_suffix("_max"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        self.apply_tf_max_value(prim_id, tf_idx, val);
                    }
                }
            }
        } else if field.starts_with("level_") && field.ends_with("_value") {
            if let Ok(val) = text.trim().parse::<f64>() {
                if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                    if let Some(level_idx) = field.strip_prefix("level_")
                        .and_then(|s| s.strip_suffix("_value"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if let Some(bid) = active_bid_prim {
                            let mut guard = self.core.drawings_mut();
                            if let Some(prim) = guard.primitives(bid).iter().find(|p| p.data().id == prim_id) {
                                if let Some(mut configs) = prim.level_configs() {
                                    if level_idx < configs.len() {
                                        configs[level_idx].level = val;
                                        guard.set_level_configs(bid, prim_id, configs);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if field.starts_with("coord_") && field.ends_with("_price") {
            if let Ok(price) = text.trim().parse::<f64>() {
                if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                    if let Some(pt_idx) = field.strip_prefix("coord_")
                        .and_then(|s| s.strip_suffix("_price"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if let Some(bid) = active_bid_prim {
                            let mut guard = self.core.drawings_mut();
                            if let Some(mut pts) = guard.primitives(bid).iter()
                                .find(|p| p.data().id == prim_id)
                                .map(|p| p.points())
                            {
                                if pt_idx < pts.len() {
                                    pts[pt_idx].1 = price;
                                    guard.set_points(bid, prim_id, &pts);
                                }
                            }
                        }
                    }
                }
            }
        } else if field.starts_with("coord_") && field.ends_with("_bar") {
            if let Ok(bar) = text.trim().parse::<f64>() {
                if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                    if let Some(pt_idx) = field.strip_prefix("coord_")
                        .and_then(|s| s.strip_suffix("_bar"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if let Some(_window) = self.panel_app.panel_grid.active_container() {
                            let coord_bars: Vec<mlc_core::Bar> = self.active_chart_bars();
                            let ts_ms = mlc_core::bar_f64_to_timestamp_ms(&coord_bars[..], bar);
                            if let Some(bid) = active_bid_prim {
                                let mut guard = self.core.drawings_mut();
                                if let Some(mut pts) = guard.primitives(bid).iter()
                                    .find(|p| p.data().id == prim_id)
                                    .map(|p| p.points())
                                {
                                    if pt_idx < pts.len() {
                                        pts[pt_idx].0 = ts_ms;
                                        guard.set_points(bid, prim_id, &pts);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if let Some(prop_id) = field.strip_prefix("text_prop:") {
            use mlc_core::drawing::primitives_v2::config::{PropertyValue, PropertyType};
            if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                let prop_type_opt = active_bid_prim
                    .and_then(|bid| {
                        let guard = self.core.drawings();
                        guard.primitives(bid).iter()
                            .find(|p| p.data().id == prim_id)
                            .and_then(|p| {
                                p.text_properties().and_then(|props| {
                                    props.into_iter()
                                        .find(|p| p.id == prop_id)
                                        .map(|p| p.prop_type)
                                })
                            })
                    });
                let value = match prop_type_opt {
                    Some(PropertyType::Text { .. }) => PropertyValue::String(text.clone()),
                    _ => {
                        if let Ok(val) = text.trim().parse::<f64>() {
                            PropertyValue::Number(val)
                        } else {
                            PropertyValue::String(text.clone())
                        }
                    }
                };
                if let Some(bid) = active_bid_prim {
                    self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Text, prop_id, &value);
                }
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            eprintln!("[ChartApp] prim_settings text_prop '{}' auto-committed: {}", prop_id, text);
        } else if let Some(prop_id) = field.strip_prefix("style_prop:") {
            use mlc_core::drawing::primitives_v2::config::PropertyValue;
            if let Ok(val) = text.trim().parse::<f64>() {
                if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                    if let Some(bid) = active_bid_prim {
                        self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Style, prop_id, &PropertyValue::Number(val));
                    }
                }
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            eprintln!("[ChartApp] prim_settings style_prop '{}' auto-committed: {}", prop_id, text);
        } else {
            eprintln!("[ChartApp] prim_settings '{}' editing auto-closed (value: {})", field, text);
        }
        // Snapshot after any committed text change.
        if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
            self.snapshot_primitive_settings_to_user_manager(prim_id);
        }
    }

    /// Start inline editing for a primitive settings sub-field.
    ///
    /// Sets `active_field_id`, loads `text` into the TextFieldStore, and focuses
    /// `PRIMITIVE_SETTINGS_FIELD`.  When `select_all` is true the initial selection
    /// covers the full text (for numeric inputs); for text content it starts unselected.
    fn start_prim_field_editing(&mut self, field_id: &str, text: &str, select_all: bool) {
        let wid = uzor::WidgetId::from(crate::text_input::PRIMITIVE_SETTINGS_FIELD);
        self.panel_app.primitive_settings_state.active_field_id = Some(field_id.to_string());
        let mut coord = self.input_coordinator.borrow_mut();
        coord.text_fields_mut().set_text(&wid, text);
        coord.text_fields_mut().focus(wid.clone());
        if select_all {
            if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
                fs.selection_start = Some(0);
            }
        }
    }

    /// Handle a click on a specific primitive settings item.
    fn handle_prim_settings_item(&mut self, item_id: &str, x: f64, _y: f64) {
        use mlc_core::drawing::primitives_v2::{LineStyle, TextAlign};
        use mlc_core::drawing::primitives_v2::config::{PropertyValue, PropertyType};
        let active_bid_prim = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);

        // Close template dropdown on any non-dropdown click
        if !item_id.starts_with("template_") {
            self.panel_app.primitive_settings_state.template_dropdown_open = false;
        }

        // Close select-property dropdown when clicking anything unrelated to it
        if !item_id.starts_with("style_prop_menu:")
            && !item_id.starts_with("style_prop_option:")
            && !item_id.starts_with("text_prop_menu:")
            && !item_id.starts_with("text_prop_option:")
            && !item_id.starts_with("level_prop_menu:")
            && !item_id.starts_with("level_prop_option:")
        {
            self.panel_app.primitive_settings_state.open_select_dropdown = None;
        }

        // ── Footer: OK (an exit, not a confirmation) ──────────────────────────
        if item_id == "ok" {
            self.panel_app.primitive_settings_state.close();
            eprintln!("[ChartApp] prim_settings closed via: {}", item_id);
            return;
        }

        // Get selected primitive id from settings state.
        let prim_id = match self.panel_app.primitive_settings_state.primitive_id {
            Some(i) => i,
            None => {
                eprintln!("[ChartApp] prim_settings item clicked but no primitive selected");
                return;
            }
        };

        let screen_w = self.width as f64;
        let screen_h = self.height as f64;

        // ── Color swatches ────────────────────────────────────────────────────
        if matches!(item_id, "stroke_color" | "fill_color" | "text_color") {
            let current_color: Option<String> = active_bid_prim
                .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                .and_then(|data| match item_id {
                    "stroke_color" => Some(data.color.stroke.clone()),
                    "fill_color"   => data.color.fill.clone(),
                    "text_color"   => data.text.as_ref().and_then(|t| t.color.clone())
                                          .or_else(|| Some(data.color.stroke.clone())),
                    _ => None,
                });
            let widget_id_str = format!("prim_settings:item:{}", item_id);
            let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
            let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
            self.panel_app.primitive_settings_state.open_color_picker_smart(
                item_id, ax, ay, aw, ah, screen_w, screen_h, current_color.as_deref(),
            );
            eprintln!("[ChartApp] prim_settings opened color picker for: {}", item_id);
            return;
        }

        // ── Line style cycling ────────────────────────────────────────────────
        if item_id == "line_style" {
            // Close any open dropdown first
            self.panel_app.primitive_settings_state.open_line_style_dropdown = false;
            let data_opt = active_bid_prim
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
                if let Some(bid) = active_bid_prim {
                    self.core.drawings_mut().set_data(bid, prim_id, &data);
                }
                self.sync_drawing_back_to_group();
                eprintln!("[ChartApp] prim_settings line_style cycled to: {}", style_str);
            }
            self.snapshot_primitive_settings_to_user_manager(prim_id);
            return;
        }

        // ── Line style menu (chevron) — toggle dropdown ───────────────────────
        if item_id == "line_style_menu" {
            self.panel_app.primitive_settings_state.open_line_style_dropdown =
                !self.panel_app.primitive_settings_state.open_line_style_dropdown;
            eprintln!("[ChartApp] prim_settings line_style_menu toggled: {}", self.panel_app.primitive_settings_state.open_line_style_dropdown);
            return;
        }

        // ── Line style option from dropdown ───────────────────────────────────
        if let Some(style_name) = item_id.strip_prefix("line_style_option:") {
            let new_style = match style_name {
                "solid"         => Some(LineStyle::Solid),
                "dashed"        => Some(LineStyle::Dashed),
                "dotted"        => Some(LineStyle::Dotted),
                "large_dashed"  => Some(LineStyle::LargeDashed),
                "sparse_dotted" => Some(LineStyle::SparseDotted),
                _               => None,
            };
            if let Some(style) = new_style {
                let data_opt = active_bid_prim
                    .and_then(|bid| self.core.drawings().get_data(bid, prim_id));
                if let Some(mut data) = data_opt {
                    data.style = style;
                    if let Some(bid) = active_bid_prim {
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings line_style set to: {}", style_name);
                }
            }
            self.panel_app.primitive_settings_state.open_line_style_dropdown = false;
            self.snapshot_primitive_settings_to_user_manager(prim_id);
            return;
        }

        // ── Select property menu (chevron) — toggle dropdown ─────────────────
        for prefix in &["style_prop_menu:", "text_prop_menu:", "level_prop_menu:"] {
            if let Some(prop_id) = item_id.strip_prefix(prefix) {
                let kind = if prefix.starts_with("style") { "style" }
                           else if prefix.starts_with("text") { "text" }
                           else { "level" };
                let key = (kind.to_string(), prop_id.to_string());
                if self.panel_app.primitive_settings_state.open_select_dropdown.as_ref() == Some(&key) {
                    self.panel_app.primitive_settings_state.open_select_dropdown = None;
                } else {
                    self.panel_app.primitive_settings_state.open_select_dropdown = Some(key);
                }
                eprintln!("[ChartApp] prim_settings {}{} toggled select dropdown", prefix, prop_id);
                return;
            }
        }

        // ── Select property option — apply value and close dropdown ───────────
        for prefix in &["style_prop_option:", "text_prop_option:", "level_prop_option:"] {
            if let Some(rest) = item_id.strip_prefix(prefix) {
                // rest = "{prop_id}:{value}"
                if let Some(colon_pos) = rest.find(':') {
                    let prop_id = &rest[..colon_pos];
                    let value = &rest[colon_pos + 1..];
                    let new_val = mlc_core::drawing::primitives_v2::config::PropertyValue::String(value.to_string());
                    if let Some(bid) = active_bid_prim {
                        let kind = if prefix.starts_with("style") {
                            mlc_core::drawing::primitives_v2::PropertyKind::Style
                        } else if prefix.starts_with("text") {
                            mlc_core::drawing::primitives_v2::PropertyKind::Text
                        } else {
                            mlc_core::drawing::primitives_v2::PropertyKind::Level
                        };
                        self.core.drawings_mut().apply_property(bid, prim_id, kind, prop_id, &new_val);
                    }
                    self.sync_drawing_back_to_group();
                    self.autosave_snapshot();
                    self.panel_app.primitive_settings_state.open_select_dropdown = None;
                    eprintln!("[ChartApp] prim_settings {}{}:{} applied", prefix, prop_id, value);
                }
                return;
            }
        }

        // ── Stroke width value — start inline text editing ───────────────────
        if item_id == "stroke_width_value" {
            let current_width = active_bid_prim
                .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                .map(|data| format!("{}", data.width as u32))
                .unwrap_or_default();
            self.start_prim_field_editing("stroke_width_value", &current_width, false);
            return;
        }

        // ── Text content — start inline text editing ──────────────────────────
        if item_id == "text_content" {
            let current_text = active_bid_prim
                .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                .and_then(|data| data.text.map(|t| t.content.clone()))
                .unwrap_or_default();
            self.start_prim_field_editing("text_content", &current_text, false);
            eprintln!("[ChartApp] prim_settings text_content editing started");
            return;
        }

        // ── Text font size — start inline text editing ────────────────────────
        if item_id == "text_font_size" {
            let current_font_size = active_bid_prim
                .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                .and_then(|data| data.text.map(|t| format!("{:.0}", t.font_size)))
                .unwrap_or_else(|| "14".to_string());
            self.start_prim_field_editing("text_font_size", &current_font_size, true);
            eprintln!("[ChartApp] prim_settings text_font_size editing started");
            return;
        }

        // ── Text bold toggle ──────────────────────────────────────────────────
        if item_id == "text_bold" {
            let data_opt = active_bid_prim
                .and_then(|bid| self.core.drawings().get_data(bid, prim_id));
            if let Some(mut data) = data_opt {
                if let Some(ref mut text) = data.text {
                    text.bold = !text.bold;
                    let new_bold = text.bold;
                    if let Some(bid) = active_bid_prim {
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings text_bold = {}", new_bold);
                }
            }
            return;
        }

        // ── Text italic toggle ────────────────────────────────────────────────
        if item_id == "text_italic" {
            let data_opt = active_bid_prim
                .and_then(|bid| self.core.drawings().get_data(bid, prim_id));
            if let Some(mut data) = data_opt {
                if let Some(ref mut text) = data.text {
                    text.italic = !text.italic;
                    let new_italic = text.italic;
                    if let Some(bid) = active_bid_prim {
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings text_italic = {}", new_italic);
                }
            }
            return;
        }

        // ── Text position: text_pos_{v}_{h} ──────────────────────────────────
        if let Some(text_pos_str) = item_id.strip_prefix("text_pos_") {
            let parts: Vec<&str> = text_pos_str.splitn(2, '_').collect();
            if parts.len() == 2 {
                let v_align = TextAlign::from_str(parts[0]);
                let h_align = TextAlign::from_str(parts[1]);
                let data_opt = active_bid_prim
                    .and_then(|bid| self.core.drawings().get_data(bid, prim_id));
                if let Some(mut data) = data_opt {
                    if data.text.is_none() {
                        data.text = Some(mlc_core::drawing::primitives_v2::PrimitiveText::default());
                    }
                    if let Some(ref mut text) = data.text {
                        text.v_align = v_align;
                        text.h_align = h_align;
                    }
                    if let Some(bid) = active_bid_prim {
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings text_pos: {}", item_id);
                }
            }
            return;
        }

        // ── Timeframe visibility toggle: tf_{i}_toggle ────────────────────────
        if item_id.starts_with("tf_") && item_id.ends_with("_toggle") {
            let data_opt = active_bid_prim
                .and_then(|bid| self.core.drawings().get_data(bid, prim_id));
            if let Some(mut data) = data_opt {
                let mut tf_config = data.timeframe_visibility.clone()
                    .unwrap_or_else(mlc_core::drawing::TimeframeVisibilityConfig::all);
                if let Some(tf_idx) = item_id.strip_prefix("tf_")
                    .and_then(|s| s.strip_suffix("_toggle"))
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    match tf_idx {
                        0 => tf_config.ticks = !tf_config.ticks,
                        1 => tf_config.seconds = if tf_config.seconds.is_some() { None } else { Some((1, 59)) },
                        2 => tf_config.minutes = if tf_config.minutes.is_some() { None } else { Some((1, 59)) },
                        3 => tf_config.hours = if tf_config.hours.is_some() { None } else { Some((1, 24)) },
                        4 => tf_config.days = if tf_config.days.is_some() { None } else { Some((1, 366)) },
                        5 => tf_config.weeks = if tf_config.weeks.is_some() { None } else { Some((1, 52)) },
                        6 => tf_config.months = if tf_config.months.is_some() { None } else { Some((1, 12)) },
                        _ => {}
                    }
                    data.timeframe_visibility = Some(tf_config);
                    if let Some(bid) = active_bid_prim {
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings tf_{}_toggle toggled", tf_idx);
                }
            }
            return;
        }

        // ── Timeframe min/max inputs — start inline text editing ─────────────
        if item_id.starts_with("tf_") && (item_id.ends_with("_min") || item_id.ends_with("_max")) {
            use mlc_core::drawing::TimeframeVisibilityConfig;

            let is_min = item_id.ends_with("_min");
            let current_val: String = if let Some(tf_idx) = item_id.strip_prefix("tf_")
                .and_then(|s| if is_min { s.strip_suffix("_min") } else { s.strip_suffix("_max") })
                .and_then(|s| s.parse::<usize>().ok())
            {
                active_bid_prim
                    .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                    .map(|data| {
                        let tf = data.timeframe_visibility.unwrap_or_else(TimeframeVisibilityConfig::all);
                        let range = match tf_idx {
                            1 => tf.seconds.unwrap_or((1, 59)),
                            2 => tf.minutes.unwrap_or((1, 59)),
                            3 => tf.hours.unwrap_or((1, 24)),
                            4 => tf.days.unwrap_or((1, 366)),
                            5 => tf.weeks.unwrap_or((1, 52)),
                            6 => tf.months.unwrap_or((1, 12)),
                            _ => (0, 0),
                        };
                        if is_min { range.0.to_string() } else { range.1.to_string() }
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            self.start_prim_field_editing(item_id, &current_val, true);
            eprintln!("[ChartApp] prim_settings {} editing started", item_id);
            return;
        }

        // ── Timeframe slider: tf_{i}_slider — click-to-position ──────────────
        if item_id.starts_with("tf_") && item_id.ends_with("_slider") {
            // Click-to-position: find the track info and jump closest handle to x.
            let track_info_opt: Option<(f64, f64, f64, f64)> = if let Some(ref result) = self.frame_result {
                result.primitive_settings.as_ref().and_then(|ps| {
                    ps.slider_tracks.iter().find(|t| t.field_id == item_id)
                        .map(|t| (t.track_x, t.track_width, t.min_val, t.max_val))
                })
            } else {
                None
            };

            if let Some((track_x, track_width, min_val, max_val)) = track_info_opt {
                let t = ((x - track_x) / track_width).clamp(0.0, 1.0);
                let clicked_val = min_val + t * (max_val - min_val);

                // Determine which handle is closest.
                let handle_and_range: Option<(DualSliderHandle, u32, u32)> = if let Some(tf_idx) = item_id
                    .strip_prefix("tf_").and_then(|s| s.strip_suffix("_slider"))
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    active_bid_prim
                        .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                        .and_then(|data| {
                            let tf_config = data.timeframe_visibility
                                .unwrap_or_else(TimeframeVisibilityConfig::all);
                            let (cur_min, cur_max): (u32, u32) = match tf_idx {
                                1 => tf_config.seconds.unwrap_or((1, 59)),
                                2 => tf_config.minutes.unwrap_or((1, 59)),
                                3 => tf_config.hours.unwrap_or((1, 24)),
                                4 => tf_config.days.unwrap_or((1, 366)),
                                5 => tf_config.weeks.unwrap_or((1, 52)),
                                6 => tf_config.months.unwrap_or((1, 12)),
                                _ => return None,
                            };
                            let min_pos = (cur_min as f64 - min_val) / (max_val - min_val);
                            let max_pos = (cur_max as f64 - min_val) / (max_val - min_val);
                            let handle = if (t - min_pos).abs() <= (t - max_pos).abs() {
                                DualSliderHandle::Min
                            } else {
                                DualSliderHandle::Max
                            };
                            Some((handle, cur_min, cur_max))
                        })
                } else {
                    None
                };

                if let Some((handle, _cur_min, _cur_max)) = handle_and_range {
                    self.apply_dual_slider_value(item_id, clicked_val.round() as u32, handle);
                } else {
                    // Fallback: use position to pick handle and apply
                    let handle = if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max };
                    self.apply_dual_slider_value(item_id, clicked_val.round() as u32, handle);
                }
            }
            return;
        }

        // ── Level visibility: level_{i}_visible ───────────────────────────────
        if item_id.starts_with("level_") && item_id.ends_with("_visible") && !item_id.starts_with("level_prop:") {
            let parts: Vec<&str> = item_id.split('_').collect();
            if parts.len() >= 3 {
                if let Ok(level_idx) = parts[1].parse::<usize>() {
                    if let Some(bid) = active_bid_prim {
                        let mut guard = self.core.drawings_mut();
                        if let Some(mut configs) = guard.primitives(bid).iter()
                            .find(|p| p.data().id == prim_id)
                            .and_then(|p| p.level_configs())
                        {
                            if level_idx < configs.len() {
                                configs[level_idx].visible = !configs[level_idx].visible;
                                let new_vis = configs[level_idx].visible;
                                guard.set_level_configs(bid, prim_id, configs);
                                eprintln!("[ChartApp] prim_settings level_{}_visible = {}", level_idx, new_vis);
                            }
                        }
                    }
                }
            }
            return;
        }

        // ── Level fill toggle: level_{i}_fill ─────────────────────────────────
        if item_id.starts_with("level_") && item_id.ends_with("_fill") && !item_id.starts_with("level_prop:") {
            let parts: Vec<&str> = item_id.split('_').collect();
            if parts.len() >= 3 {
                if let Ok(level_idx) = parts[1].parse::<usize>() {
                    if let Some(bid) = active_bid_prim {
                        let mut guard = self.core.drawings_mut();
                        if let Some(mut configs) = guard.primitives(bid).iter()
                            .find(|p| p.data().id == prim_id)
                            .and_then(|p| p.level_configs())
                        {
                            if level_idx < configs.len() {
                                configs[level_idx].fill_enabled = !configs[level_idx].fill_enabled;
                                let new_fill = configs[level_idx].fill_enabled;
                                guard.set_level_configs(bid, prim_id, configs);
                                eprintln!("[ChartApp] prim_settings level_{}_fill = {}", level_idx, new_fill);
                            }
                        }
                    }
                }
            }
            return;
        }

        // ── Level color: level_{i}_color ──────────────────────────────────────
        if item_id.starts_with("level_") && item_id.ends_with("_color") && !item_id.starts_with("level_prop:") {
            let parts: Vec<&str> = item_id.split('_').collect();
            if parts.len() >= 3 {
                if let Ok(level_idx) = parts[1].parse::<usize>() {
                    let current_color: Option<String> = active_bid_prim
                        .and_then(|bid| {
                            let guard = self.core.drawings();
                            guard.primitives(bid).iter()
                                .find(|p| p.data().id == prim_id)
                                .and_then(|p| {
                                    if let Some(configs) = p.level_configs() {
                                        if level_idx < configs.len() {
                                            return configs[level_idx].color.clone()
                                                .or_else(|| Some(p.data().color.stroke.clone()));
                                        }
                                    }
                                    None
                                })
                        });
                    let widget_id_str = format!("prim_settings:item:{}", item_id);
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                    let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                    self.panel_app.primitive_settings_state.open_color_picker_smart(
                        item_id, ax, ay, aw, ah, screen_w, screen_h, current_color.as_deref(),
                    );
                    eprintln!("[ChartApp] prim_settings opened color picker for level_{}_color", level_idx);
                }
            }
            return;
        }

        // ── Level value: level_{i}_value ─ start inline text editing ─────────
        if item_id.starts_with("level_") && item_id.ends_with("_value") && !item_id.starts_with("level_prop:") {
            let parts: Vec<&str> = item_id.split('_').collect();
            let current_val: String = if parts.len() >= 3 {
                if let Ok(level_idx) = parts[1].parse::<usize>() {
                    active_bid_prim
                        .and_then(|bid| {
                            let guard = self.core.drawings();
                            guard.primitives(bid).iter()
                                .find(|p| p.data().id == prim_id)
                                .and_then(|p| {
                                    if let Some(configs) = p.level_configs() {
                                        if level_idx < configs.len() {
                                            return Some(format!("{:.4}", configs[level_idx].level));
                                        }
                                    }
                                    None
                                })
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            self.start_prim_field_editing(item_id, &current_val, true);
            eprintln!("[ChartApp] prim_settings {} editing started", item_id);
            return;
        }

        // ── Coordinate editing: coord_{idx}_{price|bar} ── start inline editing
        if item_id.starts_with("coord_") {
            let is_price = item_id.ends_with("_price");
            let current_val: String = if let Some(pt_idx) = item_id.strip_prefix("coord_")
                .and_then(|s| if is_price { s.strip_suffix("_price") } else { s.strip_suffix("_bar") })
                .and_then(|s| s.parse::<usize>().ok())
            {
                active_bid_prim
                    .and_then(|bid| {
                        let guard = self.core.drawings();
                        guard.primitives(bid).iter()
                            .find(|p| p.data().id == prim_id)
                            .and_then(|p| {
                                let pts = p.points();
                                if pt_idx < pts.len() {
                                    if is_price {
                                        Some(format!("{:.4}", pts[pt_idx].1))
                                    } else {
                                        Some(format!("{:.0}", pts[pt_idx].0))
                                    }
                                } else {
                                    None
                                }
                            })
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            self.start_prim_field_editing(item_id, &current_val, true);
            eprintln!("[ChartApp] prim_settings {} editing started", item_id);
            return;
        }

        // ── Style properties: style_prop:{id} ────────────────────────────────
        if let Some(prop_id) = item_id.strip_prefix("style_prop:") {
            // Collect property info under immutable borrow
            let prop_info: Option<(PropertyType, PropertyValue)> = active_bid_prim
                .and_then(|bid| {
                    let guard = self.core.drawings();
                    guard.primitives(bid).iter()
                        .find(|p| p.data().id == prim_id)
                        .and_then(|p| {
                            let style_props = p.style_properties();
                            style_props.iter().find(|prop| prop.id == prop_id)
                                .map(|prop| (prop.prop_type.clone(), prop.value.clone()))
                        })
                });

            if let Some((prop_type, prop_value)) = prop_info {
                match prop_type {
                    PropertyType::Boolean => {
                        let current = prop_value.as_bool().unwrap_or(false);
                        let new_val = PropertyValue::Boolean(!current);
                        if let Some(bid) = active_bid_prim {
                            self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Style, prop_id, &new_val);
                        }
                        self.sync_drawing_back_to_group();
                        self.autosave_snapshot();
                        eprintln!("[ChartApp] prim_settings style_prop '{}' toggled: {} -> {}", prop_id, current, !current);
                    }
                    PropertyType::Select { ref options } => {
                        let current = prop_value.as_string().unwrap_or_default().to_string();
                        let current_idx = options.iter().position(|o| o.value == current).unwrap_or(0);
                        let next_idx = (current_idx + 1) % options.len().max(1);
                        if let Some(next_opt) = options.get(next_idx) {
                            let new_val = PropertyValue::String(next_opt.value.clone());
                            let next_value = next_opt.value.clone();
                            if let Some(bid) = active_bid_prim {
                                self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Style, prop_id, &new_val);
                            }
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
                            eprintln!("[ChartApp] prim_settings style_prop '{}' cycled: {} -> {}", prop_id, current, next_value);
                        }
                    }
                    PropertyType::Color => {
                        let current_color = prop_value.as_color().unwrap_or("#ffffff");
                        let widget_id_str = format!("prim_settings:item:{}", item_id);
                        let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                        let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                        self.panel_app.primitive_settings_state.open_color_picker_smart(
                            item_id, ax, ay, aw, ah, screen_w, screen_h, Some(current_color),
                        );
                        eprintln!("[ChartApp] prim_settings style_prop '{}' color picker opened", prop_id);
                    }
                    PropertyType::Number { .. } => {
                        let current_val = match prop_value {
                            PropertyValue::Number(f) => format!("{}", f),
                            PropertyValue::Integer(i) => format!("{}", i),
                            _ => String::new(),
                        };
                        self.start_prim_field_editing(item_id, &current_val, true);
                        eprintln!("[ChartApp] prim_settings style_prop '{}' number editing started", prop_id);
                    }
                    _ => {
                        eprintln!("[ChartApp] prim_settings style_prop '{}' unsupported type", prop_id);
                    }
                }
            } else {
                eprintln!("[ChartApp] prim_settings style_prop '{}' not found", prop_id);
            }
            return;
        }

        // ── Level properties: level_prop:{id} ────────────────────────────────
        if let Some(prop_id) = item_id.strip_prefix("level_prop:") {
            let prop_info: Option<(PropertyType, PropertyValue)> = active_bid_prim
                .and_then(|bid| {
                    let guard = self.core.drawings();
                    guard.primitives(bid).iter()
                        .find(|p| p.data().id == prim_id)
                        .and_then(|p| {
                            let level_props = p.level_properties();
                            level_props.iter().find(|prop| prop.id == prop_id)
                                .map(|prop| (prop.prop_type.clone(), prop.value.clone()))
                        })
                });

            if let Some((prop_type, prop_value)) = prop_info {
                match prop_type {
                    PropertyType::Boolean => {
                        let current = prop_value.as_bool().unwrap_or(false);
                        let new_val = PropertyValue::Boolean(!current);
                        if let Some(bid) = active_bid_prim {
                            self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Level, prop_id, &new_val);
                        }
                        eprintln!("[ChartApp] prim_settings level_prop '{}' toggled: {} -> {}", prop_id, current, !current);
                    }
                    PropertyType::Select { ref options } => {
                        let current = prop_value.as_string().unwrap_or_default().to_string();
                        let current_idx = options.iter().position(|o| o.value == current).unwrap_or(0);
                        let next_idx = (current_idx + 1) % options.len().max(1);
                        if let Some(next_opt) = options.get(next_idx) {
                            let new_val = PropertyValue::String(next_opt.value.clone());
                            let next_value = next_opt.value.clone();
                            if let Some(bid) = active_bid_prim {
                                self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Level, prop_id, &new_val);
                            }
                            eprintln!("[ChartApp] prim_settings level_prop '{}' cycled: {} -> {}", prop_id, current, next_value);
                        }
                    }
                    PropertyType::Color => {
                        let current_color = prop_value.as_color().unwrap_or("#ffffff");
                        let widget_id_str = format!("prim_settings:item:{}", item_id);
                        let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                        let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                        self.panel_app.primitive_settings_state.open_color_picker_smart(
                            item_id, ax, ay, aw, ah, screen_w, screen_h, Some(current_color),
                        );
                        eprintln!("[ChartApp] prim_settings level_prop '{}' color picker opened", prop_id);
                    }
                    _ => {
                        eprintln!("[ChartApp] prim_settings level_prop '{}' unsupported type", prop_id);
                    }
                }
            } else {
                eprintln!("[ChartApp] prim_settings level_prop '{}' not found", prop_id);
            }
            return;
        }

        // ── Text properties: text_prop:{id} ───────────────────────────────────
        if let Some(prop_id) = item_id.strip_prefix("text_prop:") {
            let prop_info: Option<(PropertyType, PropertyValue)> = active_bid_prim
                .and_then(|bid| {
                    let guard = self.core.drawings();
                    guard.primitives(bid).iter()
                        .find(|p| p.data().id == prim_id)
                        .and_then(|p| p.text_properties())
                        .and_then(|text_props| {
                            text_props.iter().find(|prop| prop.id == prop_id)
                                .map(|prop| (prop.prop_type.clone(), prop.value.clone()))
                        })
                });

            if let Some((prop_type, prop_value)) = prop_info {
                match prop_type {
                    PropertyType::Boolean => {
                        let current = prop_value.as_bool().unwrap_or(false);
                        let new_val = PropertyValue::Boolean(!current);
                        if let Some(bid) = active_bid_prim {
                            self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Text, prop_id, &new_val);
                        }
                        eprintln!("[ChartApp] prim_settings text_prop '{}' toggled: {} -> {}", prop_id, current, !current);
                    }
                    PropertyType::Select { ref options } => {
                        let current = prop_value.as_string().unwrap_or_default().to_string();
                        let current_idx = options.iter().position(|o| o.value == current).unwrap_or(0);
                        let next_idx = (current_idx + 1) % options.len().max(1);
                        if let Some(next_opt) = options.get(next_idx) {
                            let new_val = PropertyValue::String(next_opt.value.clone());
                            let next_value = next_opt.value.clone();
                            if let Some(bid) = active_bid_prim {
                                self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Text, prop_id, &new_val);
                            }
                            eprintln!("[ChartApp] prim_settings text_prop '{}' cycled: {} -> {}", prop_id, current, next_value);
                        }
                    }
                    PropertyType::Color => {
                        let current_color = prop_value.as_color().unwrap_or("#ffffff");
                        let widget_id_str = format!("prim_settings:item:{}", item_id);
                        let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                        let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                        self.panel_app.primitive_settings_state.open_color_picker_smart(
                            item_id, ax, ay, aw, ah, screen_w, screen_h, Some(current_color),
                        );
                        eprintln!("[ChartApp] prim_settings text_prop '{}' color picker opened", prop_id);
                    }
                    PropertyType::Text { .. } | PropertyType::Number { .. } => {
                        let current_val = match &prop_value {
                            PropertyValue::String(s) => s.clone(),
                            PropertyValue::Number(f) => format!("{}", f),
                            PropertyValue::Integer(i) => format!("{}", i),
                            _ => String::new(),
                        };
                        self.start_prim_field_editing(item_id, &current_val, true);
                        eprintln!("[ChartApp] prim_settings text_prop '{}' editing started", prop_id);
                    }
                    _ => {
                        eprintln!("[ChartApp] prim_settings text_prop '{}' unsupported type", prop_id);
                    }
                }
            } else {
                eprintln!("[ChartApp] prim_settings text_prop '{}' not found", prop_id);
            }
            return;
        }

        // ── Template dropdown button ──────────────────────────────────────────
        if item_id == "template_dropdown" {
            self.panel_app.primitive_settings_state.template_dropdown_open =
                !self.panel_app.primitive_settings_state.template_dropdown_open;
            eprintln!("[ChartApp] prim_settings template_dropdown toggled");
            return;
        }

        // ── Template dropdown menu background — absorb click, keep open ───────
        if item_id == "template_dropdown_menu" {
            return;
        }

        // ── Template delete button ────────────────────────────────────────────
        if let Some(tmpl_id) = item_id.strip_prefix("template_delete:") {
            eprintln!("[ChartApp] prim_settings deleted primitive template: {}", tmpl_id);
            if self.panel_app.primitive_settings_state.applied_template_id.as_deref() == Some(tmpl_id) {
                self.panel_app.primitive_settings_state.applied_template_id = None;
            }
            self.template_actions.push(crate::TemplateAction::RemovePrimitive { id: tmpl_id.to_string() });
            return;
        }

        // ── Template option selected ──────────────────────────────────────────
        if let Some(tmpl_id) = item_id.strip_prefix("template_option:") {
            let tmpl_opt = self.core.templates().get_primitive_template(tmpl_id).cloned();
            if let Some(tmpl) = tmpl_opt {
                self.panel_app.primitive_settings_state.applied_template_id = Some(tmpl.id.clone());
                self.panel_app.primitive_settings_state.template_dropdown_open = false;
                // Apply the template to the primitive
                if let Some(bid) = active_bid_prim {
                    if let Some(mut data) = self.core.drawings().get_data(bid, prim_id) {
                        tmpl.apply_to_primitive_data(&mut data);
                        self.core.drawings_mut().set_data(bid, prim_id, &data);
                    }
                }
                self.sync_drawing_back_to_group();
                self.autosave_snapshot();
                self.snapshot_primitive_settings_to_user_manager(prim_id);
                eprintln!("[ChartApp] prim_settings template applied: {}", tmpl.name);
            }
            return;
        }

        // ── Template "Save as..." (from dropdown) ────────────────────────────
        if item_id == "template_save_as" {
            self.panel_app.primitive_settings_state.template_dropdown_open = false;
            self.panel_app.primitive_settings_state.save_template_mode = true;
            let prefix = "Мой шаблон ";
            let max_n = self.core.templates().primitive_templates()
                .iter()
                .filter_map(|t| t.name.strip_prefix(prefix))
                .filter_map(|s| s.parse::<u32>().ok())
                .max()
                .unwrap_or(0);
            let default_name = format!("{}{}", prefix, max_n + 1);
            let field_id = uzor::WidgetId::from(crate::text_input::PRIMITIVE_SETTINGS_TEMPLATE_NAME);
            self.input_coordinator.borrow_mut().text_fields_mut().focus(field_id.clone());
            self.input_coordinator.borrow_mut().text_fields_mut().set_text(&field_id, &default_name);
            eprintln!("[ChartApp] prim_settings template save-as opened");
            return;
        }

        // ── Template "По умолчанию" (from dropdown) ───────────────────────────
        if item_id == "template_default" {
            self.panel_app.primitive_settings_state.template_dropdown_open = false;
            self.panel_app.primitive_settings_state.applied_template_id = None;
            // Reset primitive to factory defaults
            if let Some(bid) = active_bid_prim {
                if let Some(data) = self.core.drawings().get_data(bid, prim_id) {
                    let type_id = data.type_id.clone();
                    let default_tmpl = mlc_core::templates::PrimitiveTemplate::new_with_defaults(&type_id, "__default__");
                    let mut data_copy = data;
                    default_tmpl.apply_to_primitive_data(&mut data_copy);
                    self.core.drawings_mut().set_data(bid, prim_id, &data_copy);
                }
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            self.snapshot_primitive_settings_to_user_manager(prim_id);
            eprintln!("[ChartApp] prim_settings template reset to defaults");
            return;
        }

        eprintln!("[ChartApp] prim_settings item not handled: {}", item_id);
    }

    // =========================================================================
    // Template name overlay modal click handlers
    // =========================================================================

    /// Handle clicks in the template name modal for primitive settings ("prim_tmpl:*").
    pub(crate) fn handle_prim_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {
                // No-op — click inside the modal is absorbed.
            }
            "close" => {
                self.panel_app.primitive_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                eprintln!("[ChartApp] prim_tmpl modal cancelled");
            }
            "save" => {
                let field_id = uzor::WidgetId::from(crate::text_input::PRIMITIVE_SETTINGS_TEMPLATE_NAME);
                let name = self.input_coordinator.borrow().text_fields().text(&field_id).to_string();
                let prim_id = match self.panel_app.primitive_settings_state.primitive_id {
                    Some(i) => i,
                    None => {
                        self.panel_app.primitive_settings_state.save_template_mode = false;
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                        return;
                    }
                };
                let active_bid_tmpl = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if !name.is_empty() {
                    if let Some(data) = active_bid_tmpl
                        .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                    {
                        let tmpl = mlc_core::templates::PrimitiveTemplate::from_primitive_data(&data, &name);
                        eprintln!("[ChartApp] prim_tmpl saved: {}", name);
                        self.template_actions.push(crate::TemplateAction::AddPrimitive(tmpl));
                    }
                }
                self.panel_app.primitive_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            _ => {
                eprintln!("[ChartApp] prim_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks in the template name modal for indicator settings ("ind_tmpl:*").
    pub(crate) fn handle_ind_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {}
            "close" => {
                self.panel_app.indicator_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                eprintln!("[ChartApp] ind_tmpl modal cancelled");
            }
            "save" => {
                let field_id = uzor::WidgetId::from(crate::text_input::INDICATOR_SETTINGS_TEMPLATE_NAME);
                let name = self.input_coordinator.borrow().text_fields().text(&field_id).to_string();
                if !name.is_empty() {
                    if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                        let type_id = self.core.indicators().type_id_of(ind_id)
                            .unwrap_or_default();
                        let params = self.core.indicators().params_snapshot(ind_id)
                            .map(|params| {
                                params.iter().map(|(k, v): (&String, &mlc_backend_indicator_engine::IndicatorParamValue)| {
                                    let json_val = match v {
                                        mlc_backend_indicator_engine::IndicatorParamValue::Int(i) => serde_json::json!(i),
                                        mlc_backend_indicator_engine::IndicatorParamValue::Float(f) => serde_json::json!(f),
                                        mlc_backend_indicator_engine::IndicatorParamValue::String(s) => serde_json::json!(s),
                                        mlc_backend_indicator_engine::IndicatorParamValue::Bool(b) => serde_json::json!(b),
                                        _ => serde_json::Value::Null,
                                    };
                                    (k.clone(), json_val)
                                }).collect::<std::collections::HashMap<_, _>>()
                            })
                            .unwrap_or_default();
                        let tmpl = mlc_core::templates::IndicatorTemplate::new(
                            &type_id, &name, true, params, std::collections::HashMap::new(),
                        );
                        eprintln!("[ChartApp] ind_tmpl saved: {}", name);
                        self.template_actions.push(crate::TemplateAction::AddIndicator(tmpl));
                    }
                }
                self.panel_app.indicator_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            _ => {
                eprintln!("[ChartApp] ind_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks in the template name modal for compare settings ("cmp_tmpl:*").
    pub(crate) fn handle_cmp_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {}
            "close" => {
                self.panel_app.compare_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                eprintln!("[ChartApp] cmp_tmpl modal cancelled");
            }
            "save" => {
                let field_id = uzor::WidgetId::from(crate::text_input::COMPARE_SETTINGS_TEMPLATE_NAME);
                let name = self.input_coordinator.borrow().text_fields().text(&field_id).to_string();
                if !name.is_empty() {
                    let color = self.panel_app.compare_settings_state.cached_color.clone();
                    let line_width = self.panel_app.compare_settings_state.cached_line_width;
                    let line_style = self.panel_app.compare_settings_state.cached_line_style.clone();
                    let tmpl = mlc_core::templates::CompareTemplate::new(
                        &name, &color, line_width, &line_style,
                    );
                    eprintln!("[ChartApp] cmp_tmpl saved: {}", name);
                    self.template_actions.push(crate::TemplateAction::AddCompare(tmpl));
                }
                self.panel_app.compare_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            _ => {
                eprintln!("[ChartApp] cmp_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks in the template name modal for chart settings ("chart_tmpl:*").
    pub(crate) fn handle_chart_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {
                // No-op — click inside the modal is absorbed.
            }
            "close" => {
                self.panel_app.chart_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                eprintln!("[ChartApp] chart_tmpl modal cancelled");
            }
            "save" => {
                let field_id = uzor::WidgetId::from(crate::text_input::CHART_SETTINGS_TEMPLATE_NAME);
                let name = self.input_coordinator.borrow().text_fields().text(&field_id).to_string();
                if !name.is_empty() {
                    let data = self.build_chart_settings_data();
                    let tmpl = mlc_core::templates::ChartTemplate::new(&name, &data);
                    eprintln!("[ChartApp] chart_tmpl saved: {}", name);
                    self.template_actions.push(crate::TemplateAction::AddChart(tmpl));
                }
                self.panel_app.chart_settings_state.save_template_mode = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
            }
            _ => {
                eprintln!("[ChartApp] chart_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks on widgets registered with the "alert_set:" prefix.
    pub(super) fn handle_alert_settings_click(&mut self, rest: &str) {
        match rest {
            // Creator: `save` writes the alert, the ✕ aborts. There is no
            // Cancel — it was a second ✕ under another name.
            "close" => {
                self.panel_app.alert_settings_state.close();
            }
            "modal_bg" | "header" => {
                // No-op (catch-all, drag handled separately)
            }
            "save" => {
                // Snapshot all the AlertSettingsState fields we need up front
                // so we don't keep a borrow on `self.panel_app` while taking
                // `self.core.alerts_mut()` below.
                let (source, name, price, price2, percentage, condition,
                     trigger_mode, transports, editing_id) = {
                    let s = &self.panel_app.alert_settings_state;
                    (
                        s.source.clone(), s.name.clone(),
                        s.price, s.price2, s.percentage, s.condition,
                        s.build_trigger_mode(), s.build_transports(),
                        s.editing_alert_id,
                    )
                };

                // Resolve all chart-bound scoping fields BEFORE taking the
                // bubble write-lock.  On wasm32 std::sync::RwLock is
                // single-threaded and aborts on a reentrant read-after-write
                // (e.g. `bubble_mut().alert.get_mut(id)` keeping the write
                // guard alive while `exchange_for_chart(cid)` tries to take
                // a read lock on the same RwLock from inside the same call).
                // Cache the read results into plain values so the write
                // section below borrows nothing else.
                let active_cid = self.panel_app.panel_grid.active_container_id();
                let active_exchange = active_cid
                    .and_then(|cid| self.exchange_for_chart(cid))
                    .unwrap_or_default();
                let active_tf_name = active_cid
                    .and_then(|cid| self.timeframe_for_chart(cid))
                    .map(|tf| tf.name.clone())
                    .unwrap_or_default();
                let active_sym = active_cid
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .unwrap_or_default();
                let active_bid = active_cid.and_then(|cid| self.bubble_for_chart(cid));
                let active_group_id = active_bid.map(|g| g.0);
                let active_window_id = self.panel_app.panel_grid
                    .active_container().map(|w| w.id.0);

                let alert_id = if let Some(existing_id) = editing_id {
                    // Find which bubble owns this alert and update it.
                    let bid_for_existing = active_bid.or_else(|| {
                        self.core.alerts().find_owner(existing_id).map(|(bid, _)| bid)
                    });
                    if let Some(bid) = bid_for_existing {
                        self.core.alerts_mut().update(bid, existing_id, mlc_core::alert::AlertPatch {
                            source: Some(source),
                            name: Some(name),
                            price: Some(price),
                            price2: Some(price2),
                            percentage: Some(percentage),
                            condition: Some(condition),
                            trigger_mode: Some(trigger_mode),
                            transports: Some(transports),
                        });
                    }
                    existing_id
                } else {
                    // Create new alert on the active bubble.
                    if let Some(bid) = active_bid {
                        // Non-Price sources need the active window's symbol;
                        // Price-source alerts carry their symbol inside
                        // `AlertSource::Price` itself (from_spec skips it there).
                        let (exchange, timeframe, window_id_hint, group_id, symbol) =
                            if active_window_id.is_some() {
                                (active_exchange, active_tf_name, active_window_id, active_group_id, active_sym)
                            } else {
                                (String::new(), String::new(), None, None, String::new())
                            };
                        self.core.alerts_mut().add(bid, mlc_core::alert::AlertSpec {
                            source,
                            name,
                            price,
                            condition,
                            price2,
                            percentage,
                            trigger_mode,
                            transports,
                            exchange,
                            timeframe,
                            window_id_hint,
                            group_id,
                            symbol,
                        })
                    } else {
                        0
                    }
                };
                eprintln!("[AlertSettings] Alert saved: id={}", alert_id);
                self.sidebar_data_dirty = true;
                self.panel_app.alert_settings_state.close();
                self.autosave_snapshot();
            }
            _ if rest.starts_with("item:condition") => {
                // Toggle condition dropdown
                self.panel_app.alert_settings_state.condition_dropdown_open =
                    !self.panel_app.alert_settings_state.condition_dropdown_open;
            }
            _ if rest.starts_with("cond:") => {
                // Condition dropdown selection
                if let Some(idx_str) = rest.strip_prefix("cond:") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        use mlc_app_layout::ui::modal_settings::AlertCondition;
                        let conditions = AlertCondition::all();
                        if idx < conditions.len() {
                            self.panel_app.alert_settings_state.condition = conditions[idx];
                            self.panel_app.alert_settings_state.condition_dropdown_open = false;
                            eprintln!("[AlertSettings] Condition set to: {}", conditions[idx].display_name());
                        }
                    }
                }
            }
            // --- Tab switching ---
            _ if rest.starts_with("tab:") => {
                use mlc_app_layout::ui::modal_settings::AlertSettingsTab;
                let tab = match rest.strip_prefix("tab:").unwrap_or_default() {
                    "settings" => Some(AlertSettingsTab::Settings),
                    "notifications" => Some(AlertSettingsTab::Notifications),
                    "list" => Some(AlertSettingsTab::AlertsList),
                    _ => None,
                };
                if let Some(t) = tab {
                    self.panel_app.alert_settings_state.active_tab = t;
                    // Close dropdowns on tab switch
                    self.panel_app.alert_settings_state.condition_dropdown_open = false;
                    self.panel_app.alert_settings_state.trigger_mode_dropdown_open = false;
                    // Refresh alerts list when switching to list tab
                    if t == mlc_app_layout::ui::modal_settings::AlertSettingsTab::AlertsList {
                        self.panel_app.alert_settings_state.all_alerts = self.core.alerts().items_all();
                    }
                }
            }
            // --- Trigger mode dropdown ---
            "item:trigger_mode" => {
                self.panel_app.alert_settings_state.trigger_mode_dropdown_open =
                    !self.panel_app.alert_settings_state.trigger_mode_dropdown_open;
            }
            _ if rest.starts_with("tmode:") => {
                if let Some(idx_str) = rest.strip_prefix("tmode:") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        let mode = match idx {
                            0 => Some(mlc_core::alert::AlertTriggerMode::OneShot),
                            1 => Some(mlc_core::alert::AlertTriggerMode::EveryTime),
                            2 => Some(mlc_core::alert::AlertTriggerMode::OncePerBar),
                            3 => Some(mlc_core::alert::AlertTriggerMode::TimesN(
                                self.panel_app.alert_settings_state.times_n,
                            )),
                            _ => None,
                        };
                        if let Some(m) = mode {
                            self.panel_app.alert_settings_state.trigger_mode = m;
                            self.panel_app.alert_settings_state.trigger_mode_dropdown_open = false;
                        }
                    }
                }
            }
            // --- Legacy transport toggles (kept for backwards-compat) ---
            "transport:popup" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.toast_enabled = !s.notification_settings.toast_enabled;
            }
            "transport:sound" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.sound_enabled = !s.notification_settings.sound_enabled;
            }
            "transport:webhook" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.webhook.enabled = !s.notification_settings.webhook.enabled;
            }
            // --- Notification settings toggles ---
            "notif:toast" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.toast_enabled = !s.notification_settings.toast_enabled;
                s.notification_settings_dirty = true;
            }
            "notif:sound" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.sound_enabled = !s.notification_settings.sound_enabled;
                s.notification_settings_dirty = true;
            }
            "notif:telegram" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.telegram.enabled = !s.notification_settings.telegram.enabled;
                s.notification_settings_dirty = true;
                // Sync saved token into TextFieldStore when enabling
                if s.notification_settings.telegram.enabled {
                    let token = s.notification_settings.telegram.bot_token.clone();
                    let tg_wid = uzor::WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&tg_wid, &token);
                }
            }
            "notif:tg_token" => {
                let tg_wid = uzor::WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
                self.input_coordinator.borrow_mut().text_fields_mut().focus(tg_wid);
            }
            "notif:tg_screenshot" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.telegram.send_screenshots =
                    !s.notification_settings.telegram.send_screenshots;
                s.notification_settings_dirty = true;
            }
            "notif:tg_test" => {
                let tg_wid = uzor::WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
                let token = self.input_coordinator.borrow().text_fields().text(&tg_wid).trim().to_string();
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.telegram.bot_token = token;
                s.tg_test_pending = true;
                s.notification_settings_dirty = true;
                s.tg_status_message = "Sending test...".to_string();
                eprintln!("[AlertSettings] Telegram test requested");
            }
            "notif:tg_detect" => {
                let tg_wid = uzor::WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
                let token = self.input_coordinator.borrow().text_fields().text(&tg_wid).trim().to_string();
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.telegram.bot_token = token;
                s.tg_detect_pending = true;
                s.notification_settings_dirty = true;
                s.tg_status_message = "Detecting users...".to_string();
                eprintln!("[AlertSettings] Telegram detect users requested");
            }
            _ if rest.starts_with("notif:tg_sub_toggle:") => {
                if let Ok(idx) = rest.strip_prefix("notif:tg_sub_toggle:").unwrap_or_default().parse::<usize>() {
                    let s = &mut self.panel_app.alert_settings_state;
                    if let Some(sub) = s.notification_settings.telegram.subscribers.get_mut(idx) {
                        sub.active = !sub.active;
                        s.notification_settings_dirty = true;
                    }
                }
            }
            _ if rest.starts_with("notif:tg_sub_remove:") => {
                if let Ok(idx) = rest.strip_prefix("notif:tg_sub_remove:").unwrap_or_default().parse::<usize>() {
                    let s = &mut self.panel_app.alert_settings_state;
                    if idx < s.notification_settings.telegram.subscribers.len() {
                        s.notification_settings.telegram.subscribers.remove(idx);
                        s.notification_settings_dirty = true;
                    }
                }
            }
            _ if rest.starts_with("notif:tg_add_detected:") => {
                if let Ok(idx) = rest.strip_prefix("notif:tg_add_detected:").unwrap_or_default().parse::<usize>() {
                    let s = &mut self.panel_app.alert_settings_state;
                    if let Some((cid, name, uname)) = s.tg_detected_users.get(idx).cloned() {
                        // Only add if not already present
                        if !s.notification_settings.telegram.subscribers.iter().any(|sub| sub.chat_id == cid) {
                            s.notification_settings.telegram.subscribers.push(
                                mlc_alert_delivery::TelegramSubscriber {
                                    chat_id: cid,
                                    display_name: name,
                                    username: uname,
                                    active: true,
                                }
                            );
                            s.notification_settings_dirty = true;
                        }
                    }
                }
            }
            "notif:webhook" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.webhook.enabled = !s.notification_settings.webhook.enabled;
            }
            "item:webhook_url" => {
                // Focus webhook URL — handled via char input routing when webhook focused
                // (placeholder: actual URL editing uses the existing webhook_url field)
            }
            // --- Alerts list filter ---
            _ if rest.starts_with("filter:") => {
                use mlc_app_layout::ui::modal_settings::AlertListFilter;
                let filter = match rest.strip_prefix("filter:").unwrap_or_default() {
                    "all" => Some(AlertListFilter::All),
                    "active" => Some(AlertListFilter::Active),
                    "triggered" => Some(AlertListFilter::Triggered),
                    _ => None,
                };
                if let Some(f) = filter {
                    self.panel_app.alert_settings_state.list_filter = f;
                }
            }
            // --- Alerts list: click row to edit ---
            _ if rest.starts_with("list_item:") => {
                if let Some(id_str) = rest.strip_prefix("list_item:") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        let alert_clone = self.core.alerts().find_owner(id).map(|(_, a)| a);
                        if let Some(alert) = alert_clone {
                            self.panel_app.alert_settings_state.open_edit(&alert);
                            self.panel_app.alert_settings_state.pin_initial_position(
                                self.content_rect.width, self.content_rect.height,
                            );
                        }
                    }
                }
            }
            // --- Alerts list: delete ---
            _ if rest.starts_with("list_delete:") => {
                if let Some(id_str) = rest.strip_prefix("list_delete:") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        let bid_opt = self.core.alerts().find_owner(id).map(|(bid, _)| bid);
                        if let Some(bid) = bid_opt {
                            self.core.alerts_mut().remove(bid, id);
                        }
                        // Refresh the list
                        self.panel_app.alert_settings_state.all_alerts = self.core.alerts().items_all();
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                }
            }
            // --- Alerts list: pause/resume ---
            _ if rest.starts_with("list_pause:") => {
                if let Some(id_str) = rest.strip_prefix("list_pause:") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        let bid_opt = self.core.alerts().find_owner(id).map(|(bid, _)| bid);
                        if let Some(bid) = bid_opt {
                            self.core.alerts_mut().toggle_pause(bid, id);
                        }
                        self.panel_app.alert_settings_state.all_alerts = self.core.alerts().items_all();
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                }
            }
            _ => {
                eprintln!("[AlertSettings] Unhandled click: {}", rest);
            }
        }
    }

    /// Handle clicks on widgets registered with the "ind_settings:" prefix.
    pub(super) fn handle_ind_settings_click(&mut self, rest: &str, x: f64, y: f64) {
        use mlc_app_layout::ui::modal_settings::IndicatorSettingsTab;

        // Auto-commit: if a param input is being edited and the click lands on a
        // DIFFERENT widget inside the same modal, commit the value (same as Enter).
        if self.panel_app.indicator_settings_state.active_field_id.is_some() {
            let editing_field = self.panel_app.indicator_settings_state.active_field_id
                .as_deref()
                .unwrap_or_default()
                .to_string();
            // The active_field_id is "indicator_param:<name>", the click item_id is
            // "input:<name>", so reconstruct for comparison.
            let click_targets_same_field = rest
                .strip_prefix("item:")
                .and_then(|item_id| item_id.strip_prefix("input:"))
                .map(|param_name| format!("indicator_param:{}", param_name) == editing_field)
                .unwrap_or(false);
            if !click_targets_same_field {
                self.commit_ind_editing_text();
            }
        }

        match rest {
            "close" => {
                self.panel_app.indicator_settings_state.close();
                self.autosave_snapshot();
            }
            "modal_bg" => {
                // Click inside modal body — close template dropdown if open
                self.panel_app.indicator_settings_state.template_dropdown_open = false;
            }
            _ if rest.starts_with("tab:") => {
                self.panel_app.indicator_settings_state.template_dropdown_open = false;
                let tab_id = &rest["tab:".len()..];
                let tab = match tab_id {
                    "inputs"     => Some(IndicatorSettingsTab::Inputs),
                    "style"      => Some(IndicatorSettingsTab::Style),
                    "visibility" => Some(IndicatorSettingsTab::Visibility),
                    "signals"    => Some(IndicatorSettingsTab::Signals),
                    "info"       => Some(IndicatorSettingsTab::Info),
                    _ => None,
                };
                if let Some(t) = tab {
                    self.panel_app.indicator_settings_state.set_tab(t);
                }
            }
            _ if rest.starts_with("item:") => {
                let item_id = &rest["item:".len()..];
                // Click-to-cursor: if the clicked field corresponds to the field being edited,
                // reposition cursor using active_input_char_positions instead of restarting.
                //
                // active_field_id is "indicator_param:<name>" for input fields, or "tf_N_min/max"
                // for timeframe visibility range inputs.
                let already_editing = {
                    let active = self.panel_app.indicator_settings_state.active_field_id.as_deref().unwrap_or_default();
                    if let Some(input_name) = item_id.strip_prefix("input:") {
                        active == format!("indicator_param:{}", input_name)
                    } else if (item_id.starts_with("tf_") && item_id.ends_with("_min"))
                        || (item_id.starts_with("tf_") && item_id.ends_with("_max"))
                    {
                        active == item_id
                    } else {
                        false
                    }
                };
                if already_editing {
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.indicator_settings.as_ref())
                        .map(|is| is.active_input_char_positions.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let wid = uzor::WidgetId::from(crate::text_input::INDICATOR_SETTINGS_FIELD);
                        let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        let mut coord = self.input_coordinator.borrow_mut();
                        if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
                            fs.cursor = new_cursor;
                            fs.selection_start = None;
                        }
                        return;
                    }
                }
                self.handle_ind_settings_item(item_id, x, y);
            }
            _ if rest.starts_with("footer:") => {
                let btn_id = &rest["footer:".len()..];
                match btn_id {
                    // No "cancel" arm: the modal has no Cancel button, because
                    // every control here applies live and there was nothing to
                    // cancel. Escape and the ✕ close it the same way.
                    "ok" => {
                        self.panel_app.indicator_settings_state.close();
                        self.autosave_snapshot();
                        eprintln!("[ChartApp] ind_settings closed via footer: {}", btn_id);
                    }
                    "template_dropdown" => {
                        self.panel_app.indicator_settings_state.template_dropdown_open =
                            !self.panel_app.indicator_settings_state.template_dropdown_open;
                        eprintln!("[ChartApp] ind_settings template_dropdown toggled");
                    }
                    "template_save_as" => {
                        self.panel_app.indicator_settings_state.template_dropdown_open = false;
                        self.panel_app.indicator_settings_state.save_template_mode = true;
                        let prefix = "Мой шаблон ";
                        let max_n = self.core.templates().indicator_templates()
                            .iter()
                            .filter_map(|t| t.name.strip_prefix(prefix))
                            .filter_map(|s| s.parse::<u32>().ok())
                            .max()
                            .unwrap_or(0);
                        let default_name = format!("{}{}", prefix, max_n + 1);
                        let field_id = uzor::WidgetId::from(crate::text_input::INDICATOR_SETTINGS_TEMPLATE_NAME);
                        self.input_coordinator.borrow_mut().text_fields_mut().focus(field_id.clone());
                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&field_id, &default_name);
                        eprintln!("[ChartApp] ind_settings template save-as opened");
                    }
                    "template_default" => {
                        self.panel_app.indicator_settings_state.template_dropdown_open = false;
                        self.panel_app.indicator_settings_state.applied_template_id = None;
                        if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                            self.core.indicators_mut().reset_to_defaults(ind_id);
                            // Recalculate
                            if let Some(active_bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
                                let bars = self.active_chart_bars();
                                self.core.indicators_mut().recalc_bubble(active_bid, &bars);
                            }
                        }
                        self.snapshot_indicator_settings_to_user_manager();
                        eprintln!("[ChartApp] ind_settings template reset to defaults");
                    }
                    "template_dropdown_menu" => {
                        // Absorb click inside dropdown, keep it open
                    }
                    _ if btn_id.starts_with("template_delete:") => {
                        let tmpl_id = &btn_id["template_delete:".len()..];
                        eprintln!("[ChartApp] ind_settings deleted indicator template: {}", tmpl_id);
                        if self.panel_app.indicator_settings_state.applied_template_id.as_deref() == Some(tmpl_id) {
                            self.panel_app.indicator_settings_state.applied_template_id = None;
                        }
                        self.template_actions.push(crate::TemplateAction::RemoveIndicator { id: tmpl_id.to_string() });
                    }
                    _ if btn_id.starts_with("template_option:") => {
                        let tmpl_id = &btn_id["template_option:".len()..];
                        let tmpl_opt = self.core.templates().get_indicator_template(tmpl_id).cloned();
                        if let Some(tmpl) = tmpl_opt {
                            self.panel_app.indicator_settings_state.applied_template_id = Some(tmpl.id.clone());
                            self.panel_app.indicator_settings_state.template_dropdown_open = false;
                            // Apply params from template to the indicator instance
                            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                                use mlc_backend_indicator_engine::IndicatorParamValue as IndValue;
                                for (k, v) in &tmpl.params {
                                    let pv = match v {
                                        serde_json::Value::Number(n) if n.is_f64() => IndValue::Float(n.as_f64().unwrap_or(0.0)),
                                        serde_json::Value::Number(n) => IndValue::Int(n.as_i64().unwrap_or(0) as i32),
                                        serde_json::Value::String(s) => IndValue::String(s.clone()),
                                        serde_json::Value::Bool(b) => IndValue::Bool(*b),
                                        _ => continue,
                                    };
                                    self.core.indicators_mut().set_param(ind_id, k, pv);
                                }
                                if let Some(active_bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
                                    let bars = self.active_chart_bars();
                                    self.core.indicators_mut().recalc_bubble(active_bid, &bars);
                                }
                            }
                            self.snapshot_indicator_settings_to_user_manager();
                            eprintln!("[ChartApp] ind_settings template applied: {}", tmpl.name);
                        }
                    }
                    _ => {
                        eprintln!("[ChartApp] ind_settings footer: {}", btn_id);
                    }
                }
            }
            "signals_toggle" => {
                if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                    let new_val = self.core.indicators()
                        .signals_enabled_of(ind_id)
                        .map(|enabled| !enabled);
                    if let Some(enabled) = new_val {
                        self.core.indicators_mut().set_signals_enabled(ind_id, enabled);
                        if let Some(active_bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
                            let bars = self.active_chart_bars();
                            self.core.indicators_mut().recalc_bubble(active_bid, &bars);
                        }
                        eprintln!("[ChartApp] Signals toggle: {}", enabled);
                    }
                }
            }
            _ => {
                eprintln!("[ChartApp] ind_settings unhandled: {}", rest);
            }
        }
    }

    /// Commit whatever param value is currently being edited in the indicator settings modal.
    ///
    /// Applies the same logic as the Enter-key handler in `on_char_input` so that
    /// clicking a different widget inside the same modal auto-commits the in-progress value.
    pub(crate) fn commit_ind_editing_text(&mut self) {
        let field = match self.panel_app.indicator_settings_state.active_field_id.take() {
            Some(f) => f,
            None => return,
        };
        let wid = uzor::WidgetId::from(crate::text_input::INDICATOR_SETTINGS_FIELD);
        let text = self.input_coordinator.borrow().text_fields().text(&wid).to_string();
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        if let Some(param_name) = field.strip_prefix("indicator_param:") {
            use mlc_backend_indicator_engine::IndicatorParamValue as IndValue;
            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                let value = if let Ok(f) = text.trim().parse::<f64>() {
                    IndValue::Float(f)
                } else if let Ok(i) = text.trim().parse::<i32>() {
                    IndValue::Int(i)
                } else {
                    IndValue::String(text.trim().to_string())
                };
                self.core.indicators_mut().set_param(ind_id, param_name, value);
                if let Some(active_bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
                    let bars = self.active_chart_bars();
                    self.core.indicators_mut().recalc_bubble(active_bid, &bars);
                }
            }
            eprintln!("[ChartApp] ind_settings param '{}' auto-committed: {}", param_name, text);
        } else if field.starts_with("tf_") && field.ends_with("_min") {
            if let Ok(val) = text.trim().parse::<u32>() {
                if let Some(tf_idx) = field.strip_prefix("tf_")
                    .and_then(|s| s.strip_suffix("_min"))
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    self.apply_ind_tf_min_value(tf_idx, val);
                    eprintln!("[ChartApp] ind_settings tf_{}_min auto-committed: {}", tf_idx, val);
                }
            }
        } else if field.starts_with("tf_") && field.ends_with("_max") {
            if let Ok(val) = text.trim().parse::<u32>() {
                if let Some(tf_idx) = field.strip_prefix("tf_")
                    .and_then(|s| s.strip_suffix("_max"))
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    self.apply_ind_tf_max_value(tf_idx, val);
                    eprintln!("[ChartApp] ind_settings tf_{}_max auto-committed: {}", tf_idx, val);
                }
            }
        }
        // Snapshot after any committed indicator text change.
        self.snapshot_indicator_settings_to_user_manager();
    }

    /// Start inline editing for an indicator settings sub-field.
    ///
    /// Sets `active_field_id`, loads `text` into the TextFieldStore, and focuses
    /// `INDICATOR_SETTINGS_FIELD`.
    fn start_ind_field_editing(&mut self, field_id: &str, text: &str, select_all: bool) {
        let wid = uzor::WidgetId::from(crate::text_input::INDICATOR_SETTINGS_FIELD);
        self.panel_app.indicator_settings_state.active_field_id = Some(field_id.to_string());
        let mut coord = self.input_coordinator.borrow_mut();
        coord.text_fields_mut().set_text(&wid, text);
        coord.text_fields_mut().focus(wid.clone());
        if select_all {
            if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
                fs.selection_start = Some(0);
            }
        }
    }

    /// Handle a click on a specific indicator settings item.
    fn handle_ind_settings_item(&mut self, item_id: &str, _x: f64, _y: f64) {
        use mlc_backend_indicator_engine::IndicatorParamValue as IndValue;

        // Close template dropdown on any content item click
        self.panel_app.indicator_settings_state.template_dropdown_open = false;

        // ── Color swatch ─────────────────────────────────────────────────────
        if let Some(output_name) = item_id.strip_prefix("color:") {
            let screen_w = self.width as f64;
            let screen_h = self.height as f64;
            let current_color: Option<String> = if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                self.core.indicators().output_color(ind_id, output_name)
            } else {
                None
            };

            let widget_id_str = format!("ind_settings:item:{}", item_id);
            let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
            let (anchor_x, anchor_y, anchor_w, anchor_h) = rect
                .map(|r| (r.x, r.y, r.width, r.height))
                .unwrap_or((0.0, 0.0, 0.0, 0.0));

            self.panel_app.indicator_settings_state.open_color_picker_smart(
                output_name,
                anchor_x, anchor_y,
                anchor_w, anchor_h,
                screen_w, screen_h,
                current_color.as_deref(),
            );
            eprintln!("[ChartApp] ind_settings opened color picker for output: {}", output_name);
            return;
        }

        // ── Dropdown menu toggle ──────────────────────────────────────────────
        if let Some(param_name) = item_id.strip_prefix("dropdown_menu:") {
            if self.panel_app.indicator_settings_state.open_param_dropdown.as_deref() == Some(param_name) {
                self.panel_app.indicator_settings_state.open_param_dropdown = None;
            } else {
                self.panel_app.indicator_settings_state.open_param_dropdown = Some(param_name.to_string());
            }
            eprintln!("[ChartApp] ind_settings dropdown_menu toggled: {}", param_name);
            return;
        }

        // ── Dropdown cycle ────────────────────────────────────────────────────
        if let Some(param_name) = item_id.strip_prefix("dropdown_cycle:") {
            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                // Collect what we need with an immutable borrow first.
                let cycle_info: Option<(String, Vec<String>)> = {
                    self.core.indicators().type_id_of(ind_id).and_then(|type_id| {
                        let current_val = self.core.indicators().param_value(ind_id, param_name)
                            .and_then(|v| v.as_string().map(|s| s.to_string()));
                        self.core.indicators().definition(&type_id).and_then(|def| {
                            def.params.iter().find(|p| p.name == param_name).map(|p| {
                                let options = p.get_options_as_strings();
                                let next_val = if let Some(ref cur) = current_val {
                                    let idx = options.iter().position(|o| o == cur).unwrap_or(0);
                                    let next_idx = (idx + 1) % options.len().max(1);
                                    options.get(next_idx).cloned().unwrap_or_default()
                                } else {
                                    options.first().cloned().unwrap_or_default()
                                };
                                (next_val, options)
                            })
                        })
                    })
                };

                if let Some((next_val, _options)) = cycle_info {
                    self.core.indicators_mut().set_param(ind_id, param_name, IndValue::String(next_val.clone()));
                    // Recalculate.
                    if let Some(active_bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
                        let bars = self.active_chart_bars();
                        self.core.indicators_mut().recalc_bubble(active_bid, &bars);
                    }
                    // CVD Display flip toggles the sub-pane (Line) vs
                    // main-chart body-painter (Candles) — re-derive panes.
                    if param_name == "display" {
                        self.sync_sub_panes_from_manager();
                    }
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                    eprintln!("[ChartApp] ind_settings dropdown_cycle: {} = {}", param_name, next_val);
                } else {
                    eprintln!("[ChartApp] ind_settings dropdown_cycle: param '{}' not found or no options", param_name);
                }
            }
            return;
        }

        // ── Dropdown option selected: "param_option:{param_name}:{value}" ────
        if let Some(rest) = item_id.strip_prefix("param_option:") {
            let mut parts = rest.splitn(2, ':');
            let param_name = parts.next().unwrap_or_default().to_string();
            let value = parts.next().unwrap_or_default().to_string();

            // A venue row TOGGLES membership and leaves the list open — the
            // same multi-click contract the Multi-DOM roster picker has, and
            // for the same reason: choosing a set of five venues should not
            // cost five trips through the menu.
            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                if self.is_venue_param(ind_id, &param_name) {
                    self.toggle_indicator_venue(ind_id, &param_name, &value);
                    return;
                }
            }

            self.panel_app.indicator_settings_state.open_param_dropdown = None;

            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                self.core.indicators_mut().set_param(ind_id, &param_name, IndValue::String(value.clone()));
                if let Some(active_bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
                    let bars = self.active_chart_bars();
                    self.core.indicators_mut().recalc_bubble(active_bid, &bars);
                }
                // CVD Display flip toggles the sub-pane (Line) vs
                // main-chart body-painter (Candles) — re-derive panes.
                if param_name == "display" {
                    self.sync_sub_panes_from_manager();
                }
                self.autosave_snapshot();
                self.snapshot_indicator_settings_to_user_manager();
                eprintln!("[ChartApp] ind_settings param_option: {} = {}", param_name, value);
            }
            return;
        }

        // ── Toggle boolean parameter ──────────────────────────────────────────
        if let Some(param_name) = item_id.strip_prefix("toggle:") {
            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                let current_bool: Option<bool> = self.core.indicators()
                    .param_value(ind_id, param_name)
                    .and_then(|v| v.as_bool());

                if let Some(cur) = current_bool {
                    self.core.indicators_mut().set_param(ind_id, param_name, IndValue::Bool(!cur));
                    if let Some(active_bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
                        let bars = self.active_chart_bars();
                        self.core.indicators_mut().recalc_bubble(active_bid, &bars);
                    }
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                    eprintln!("[ChartApp] ind_settings toggle: {} = {}", param_name, !cur);
                } else {
                    eprintln!("[ChartApp] ind_settings toggle: param '{}' not found or not bool", param_name);
                }
            }
            return;
        }

        // ── Timeframe toggle buttons ──────────────────────────────────────────
        if item_id.starts_with("tf_") && item_id.ends_with("_toggle") {
            if let Some(tf_idx) = item_id.strip_prefix("tf_")
                .and_then(|s| s.strip_suffix("_toggle"))
                .and_then(|s| s.parse::<usize>().ok())
            {
                if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                    self.core.indicators_mut().update_timeframe_visibility(ind_id, |tf_config| {
                        match tf_idx {
                            0 => tf_config.ticks = !tf_config.ticks,
                            1 => tf_config.seconds = if tf_config.seconds.is_some() { None } else { Some((1, 59)) },
                            2 => tf_config.minutes = if tf_config.minutes.is_some() { None } else { Some((1, 59)) },
                            3 => tf_config.hours   = if tf_config.hours.is_some()   { None } else { Some((1, 24)) },
                            4 => tf_config.days    = if tf_config.days.is_some()    { None } else { Some((1, 366)) },
                            5 => tf_config.weeks   = if tf_config.weeks.is_some()   { None } else { Some((1, 52)) },
                            6 => tf_config.months  = if tf_config.months.is_some()  { None } else { Some((1, 12)) },
                            _ => {}
                        }
                    });
                    eprintln!("[ChartApp] ind_settings tf_{}_toggle toggled", tf_idx);
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                }
            }
            return;
        }

        // ── Timeframe min/max inputs — start inline text editing ─────────────
        if item_id.starts_with("tf_") && (item_id.ends_with("_min") || item_id.ends_with("_max")) {
            use mlc_core::drawing::TimeframeVisibilityConfig;

            let is_min = item_id.ends_with("_min");
            let current_val: String = if let Some(tf_idx) = item_id.strip_prefix("tf_")
                .and_then(|s| if is_min { s.strip_suffix("_min") } else { s.strip_suffix("_max") })
                .and_then(|s| s.parse::<usize>().ok())
            {
                self.panel_app.indicator_settings_state.indicator_id
                    .and_then(|ind_id| self.core.indicators().timeframe_visibility_of(ind_id).map(|tf_cfg| {
                        let tf = tf_cfg.unwrap_or_else(TimeframeVisibilityConfig::all);
                        let range = match tf_idx {
                            1 => tf.seconds.unwrap_or((1, 59)),
                            2 => tf.minutes.unwrap_or((1, 59)),
                            3 => tf.hours.unwrap_or((1, 24)),
                            4 => tf.days.unwrap_or((1, 366)),
                            5 => tf.weeks.unwrap_or((1, 52)),
                            6 => tf.months.unwrap_or((1, 12)),
                            _ => (0, 0),
                        };
                        if is_min { range.0.to_string() } else { range.1.to_string() }
                    }))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            self.start_ind_field_editing(item_id, &current_val, true);
            eprintln!("[ChartApp] ind_settings {} editing started", item_id);
            return;
        }

        // ── Text input — start editing the parameter value ───────────────────
        if let Some(param_name_str) = item_id.strip_prefix("input:") {
            let param_name = param_name_str.to_string();
            let current_value: String = self.panel_app.indicator_settings_state.indicator_id
                .and_then(|ind_id| self.core.indicators().param_value(ind_id, &param_name))
                .map(|v| v.to_display_string())
                .unwrap_or_default();
            let field_id = format!("indicator_param:{}", param_name);
            self.start_ind_field_editing(&field_id, &current_value, false);
            eprintln!("[ChartApp] ind_settings text input editing started: {}", param_name);
            return;
        }

        // ── Signal display config buttons ─────────────────────────────────────
        if item_id.starts_with("ind_set:") {
            use mlc_core::indicator_source::SignalShape;
            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                if let Some(shape_str) = item_id.strip_prefix("ind_set:signal_shape:") {
                    let shape = match shape_str {
                        "arrow"    => SignalShape::Arrow,
                        "triangle" => SignalShape::Triangle,
                        "circle"   => SignalShape::Circle,
                        "diamond"  => SignalShape::Diamond,
                        _ => {
                            eprintln!("[ChartApp] ind_settings unknown signal shape: {}", shape_str);
                            return;
                        }
                    };
                    self.core.indicators_mut().set_signal_shape(ind_id, shape);
                    eprintln!("[ChartApp] ind_settings signal_shape = {:?}", shape);
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_bullish_color" {
                    let screen_w = self.width as f64;
                    let screen_h = self.height as f64;
                    let current_color = self.core.indicators()
                        .signal_display_of(ind_id)
                        .map(|sd| sd.bullish_color);
                    let widget_id_str = format!("ind_settings:item:{}", item_id);
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                    let (anchor_x, anchor_y, anchor_w, anchor_h) = rect
                        .map(|r| (r.x, r.y, r.width, r.height))
                        .unwrap_or((0.0, 0.0, 0.0, 0.0));
                    self.panel_app.indicator_settings_state.open_color_picker_smart(
                        "signal_bullish_color",
                        anchor_x, anchor_y,
                        anchor_w, anchor_h,
                        screen_w, screen_h,
                        current_color.as_deref(),
                    );
                    eprintln!("[ChartApp] ind_settings opened color picker for signal_bullish_color");
                } else if item_id == "ind_set:signal_bearish_color" {
                    let screen_w = self.width as f64;
                    let screen_h = self.height as f64;
                    let current_color = self.core.indicators()
                        .signal_display_of(ind_id)
                        .map(|sd| sd.bearish_color);
                    let widget_id_str = format!("ind_settings:item:{}", item_id);
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                    let (anchor_x, anchor_y, anchor_w, anchor_h) = rect
                        .map(|r| (r.x, r.y, r.width, r.height))
                        .unwrap_or((0.0, 0.0, 0.0, 0.0));
                    self.panel_app.indicator_settings_state.open_color_picker_smart(
                        "signal_bearish_color",
                        anchor_x, anchor_y,
                        anchor_w, anchor_h,
                        screen_w, screen_h,
                        current_color.as_deref(),
                    );
                    eprintln!("[ChartApp] ind_settings opened color picker for signal_bearish_color");
                } else if item_id == "ind_set:signal_size_inc" {
                    let cur = self.core.indicators().signal_display_of(ind_id).map(|sd| sd.size).unwrap_or(0.0);
                    let new_size = (cur + 2.0).min(24.0);
                    self.core.indicators_mut().set_signal_size(ind_id, new_size);
                    eprintln!("[ChartApp] ind_settings signal_size = {}", new_size);
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_size_dec" {
                    let cur = self.core.indicators().signal_display_of(ind_id).map(|sd| sd.size).unwrap_or(0.0);
                    let new_size = (cur - 2.0).max(8.0);
                    self.core.indicators_mut().set_signal_size(ind_id, new_size);
                    eprintln!("[ChartApp] ind_settings signal_size = {}", new_size);
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_offset_inc" {
                    let cur = self.core.indicators().signal_display_of(ind_id).map(|sd| sd.offset).unwrap_or(0.0);
                    let new_offset = (cur + 2.0).min(16.0);
                    self.core.indicators_mut().set_signal_offset(ind_id, new_offset);
                    eprintln!("[ChartApp] ind_settings signal_offset = {}", new_offset);
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_offset_dec" {
                    let cur = self.core.indicators().signal_display_of(ind_id).map(|sd| sd.offset).unwrap_or(0.0);
                    let new_offset = (cur - 2.0).max(0.0);
                    self.core.indicators_mut().set_signal_offset(ind_id, new_offset);
                    eprintln!("[ChartApp] ind_settings signal_offset = {}", new_offset);
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else {
                    eprintln!("[ChartApp] ind_settings item unhandled: {}", item_id);
                }
            }
            return;
        }

        eprintln!("[ChartApp] ind_settings item unhandled: {}", item_id);
    }

    /// Is `param_name` on this instance the set-valued venue picker?
    fn is_venue_param(&self, instance_id: u64, param_name: &str) -> bool {
        use mlc_backend_indicator_engine::IndicatorParamType;
        let indicators = self.core.indicators();
        indicators
            .type_id_of(instance_id)
            .and_then(|tid| indicators.definition(&tid))
            .and_then(|def| {
                def.params
                    .iter()
                    .find(|p| p.name == param_name)
                    .map(|p| matches!(p.param_type, IndicatorParamType::Venues))
            })
            .unwrap_or(false)
    }

    /// Add or remove one venue from a set-valued parameter.
    ///
    /// The stored set is what the owner NARROWED to, so unticking the last
    /// venue stores the empty set — which reads as "all of them" and is the
    /// same state a fresh instance starts in. Unticking everything to mean
    /// "fold nothing" would be a third state the aggregate flag already
    /// expresses, and a picker that can silently blank a layer is a trap.
    fn toggle_indicator_venue(&mut self, instance_id: u64, param_name: &str, venue: &str) {
        use mlc_backend_indicator_engine::IndicatorParamValue as IndValue;

        let mut chosen: Vec<String> = self
            .core
            .indicators()
            .param_value(instance_id, param_name)
            .and_then(|v| v.as_string_list().map(|s| s.to_vec()))
            .unwrap_or_default();

        // An empty set means "every offered venue", so the first click has
        // to start from that full set and REMOVE one — otherwise unticking a
        // venue in the default state would silently narrow the layer to the
        // single venue just clicked, the exact opposite of the gesture.
        if chosen.is_empty() {
            chosen = self
                .panel_app
                .indicator_settings_state
                .venue_options
                .iter()
                .map(|(v, _)| v.clone())
                .collect();
        }

        if let Some(pos) = chosen.iter().position(|c| c.eq_ignore_ascii_case(venue)) {
            chosen.remove(pos);
        } else {
            chosen.push(venue.to_string());
        }

        // Back to the empty set when everything is ticked: same picture,
        // and it keeps following the offered list as connectors come and go.
        let offered = self.panel_app.indicator_settings_state.venue_options.len();
        if chosen.len() >= offered && offered > 0 {
            chosen.clear();
        }

        self.core
            .indicators_mut()
            .set_param(instance_id, param_name, IndValue::StringList(chosen.clone()));
        self.autosave_snapshot();
        self.snapshot_indicator_settings_to_user_manager();
        eprintln!(
            "[ChartApp] ind_settings venue toggled: {} {} -> [{}]",
            param_name, venue, chosen.join(",")
        );
    }
}
