//! Settings modal handlers: chart settings, primitive settings, indicator settings,
//! alert settings, template modals (per-modal commit/apply for editing text).

use crate::ChartApp;
use zengeld_chart::{
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
use zengeld_chart::ui::modal_settings::DualSliderHandle;
use zengeld_chart::drawing::TimeframeVisibilityConfig;

impl ChartApp {
    // -------------------------------------------------------------------------
    // Modal widget click handlers
    // -------------------------------------------------------------------------

    /// Handle clicks on widgets registered with the "chart_settings:" prefix.
    pub(super) fn handle_chart_settings_click(&mut self, rest: &str, x: f64, _y: f64) {
        use zengeld_chart::ui::modal_settings::ChartSettingsTab;

        // Auto-commit: if an input field is being edited and the click lands on a
        // DIFFERENT widget inside the same modal, commit the value (same as Enter).
        if self.panel_app.chart_settings_state.editing_text.is_some() {
            let editing_field = self.panel_app.chart_settings_state.editing_text
                .as_ref()
                .map(|e| e.field_id.clone())
                .unwrap_or_default();
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
                let already_editing = self.panel_app.chart_settings_state.editing_text
                    .as_ref()
                    .map(|e| e.field_id == item_id)
                    .unwrap_or(false);
                if already_editing {
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.chart_settings.as_ref())
                        .map(|cs| cs.active_input_char_positions.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(ref mut edit) = self.panel_app.chart_settings_state.editing_text {
                            edit.cursor = new_cursor;
                            edit.selection_start = None;
                            edit.reset_blink(0);
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
                    "ok" | "cancel" => {
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
                        let max_n = self.panel_app.template_manager.chart_templates
                            .iter()
                            .filter_map(|t| t.name.strip_prefix(prefix))
                            .filter_map(|s| s.parse::<u32>().ok())
                            .max()
                            .unwrap_or(0);
                        let default_name = format!("{}{}", prefix, max_n + 1);
                        let default_cursor = default_name.chars().count();
                        self.panel_app.chart_settings_state.template_name_editing = Some(
                            zengeld_chart::ui::modal_settings::TextEditingState {
                                field_id: "template_name".to_string(),
                                text: default_name,
                                cursor: default_cursor,
                                selection_start: None,
                                blink_time: 0,
                            }
                        );
                        eprintln!("[ChartApp] chart_settings template save-as opened");
                    }
                    "template_default" => {
                        self.panel_app.chart_settings_state.template_dropdown_open = false;
                        self.panel_app.chart_settings_state.applied_template_id = None;
                        let defaults = zengeld_chart::templates::ChartTemplate::developer_defaults();
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
                        if let Some(tmpl) = self.panel_app.template_manager.get_chart_template(tmpl_id).cloned() {
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
    fn commit_chart_settings_editing_text(&mut self) {
        let (text, field) = match self.panel_app.chart_settings_state.editing_text.take() {
            Some(e) => (e.text, e.field_id),
            None => return,
        };
        if field == "status:watermark_text" {
            if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                if let Some(ref mut watermark) = w.watermark {
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
    fn apply_chart_template(&mut self, tmpl: &zengeld_chart::templates::ChartTemplate) {
        use zengeld_chart::layout::modals::chart_settings::{
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
        }

        // ── Scales & Lines settings ───────────────────────────────────────────
        if let Ok(scales) = serde_json::from_value::<ScalesLinesSettings>(tmpl.scales.clone()) {
            if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                w.grid_options.vert_lines.visible = scales.vert_lines;
                w.grid_options.horz_lines.visible = scales.horz_lines;
                w.scale_settings.price_scale_width  = scales.price_scale_width;
                w.scale_settings.time_scale_height  = scales.time_scale_height;
                if scales.auto_scale {
                    w.price_scale.scale_mode = zengeld_chart::ScaleMode::Auto;
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
                "use_24h" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.time_format.use_24h = !w.scale_settings.time_format.use_24h;
                        eprintln!("[ChartApp] instrument:use_24h = {}", w.scale_settings.time_format.use_24h);
                    }
                }
                "date_format_cycle" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.time_format.show_day_of_week = !w.scale_settings.time_format.show_day_of_week;
                        eprintln!("[ChartApp] instrument:show_day_of_week = {}", w.scale_settings.time_format.show_day_of_week);
                    }
                }
                "show_bar_countdown" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.show_bar_countdown = !w.scale_settings.show_bar_countdown;
                        eprintln!("[ChartApp] instrument:show_bar_countdown = {}", w.scale_settings.show_bar_countdown);
                    }
                }
                "price_tick_extend_right" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.price_tick_extend_right = !w.scale_settings.price_tick_extend_right;
                        eprintln!("[ChartApp] instrument:price_tick_extend_right = {}", w.scale_settings.price_tick_extend_right);
                    }
                }
                "price_tick_extend_left" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.price_tick_extend_left = !w.scale_settings.price_tick_extend_left;
                        eprintln!("[ChartApp] instrument:price_tick_extend_left = {}", w.scale_settings.price_tick_extend_left);
                    }
                }
                "body_up_color" | "body_down_color"
                | "border_up_color" | "border_down_color"
                | "wick_up_color" | "wick_down_color" => {
                    let current_color: Option<String> = {
                        let s = &self.panel_app.theme_manager.current().series;
                        match field {
                            "body_up_color"     => Some(s.candle_up_body.clone()),
                            "body_down_color"   => Some(s.candle_down_body.clone()),
                            "border_up_color"   => s.candle_up_border.clone().or_else(|| Some(s.candle_up_body.clone())),
                            "border_down_color" => s.candle_down_border.clone().or_else(|| Some(s.candle_down_body.clone())),
                            "wick_up_color"     => Some(s.candle_up_wick.clone()),
                            "wick_down_color"   => Some(s.candle_down_wick.clone()),
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let current = w.grid_options.vert_lines.visible && w.grid_options.horz_lines.visible;
                        w.grid_options.vert_lines.visible = !current;
                        w.grid_options.horz_lines.visible = !current;
                        eprintln!("[ChartApp] scales:show_grid = {}", !current);
                    }
                }
                "vert_lines" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.grid_options.vert_lines.visible = !w.grid_options.vert_lines.visible;
                        eprintln!("[ChartApp] scales:vert_lines = {}", w.grid_options.vert_lines.visible);
                    }
                }
                "horz_lines" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.grid_options.horz_lines.visible = !w.grid_options.horz_lines.visible;
                        eprintln!("[ChartApp] scales:horz_lines = {}", w.grid_options.horz_lines.visible);
                    }
                }
                "price_scale_right" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.price_scale_position = match w.scale_settings.price_scale_position {
                            PriceScalePosition::Hidden => PriceScalePosition::Right,
                            _ => PriceScalePosition::Hidden,
                        };
                        eprintln!("[ChartApp] scales:price_scale_right toggled: {:?}", w.scale_settings.price_scale_position);
                    }
                }
                "time_scale_bottom" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.time_scale_position = match w.scale_settings.time_scale_position {
                            TimeScalePosition::Hidden => TimeScalePosition::Bottom,
                            _ => TimeScalePosition::Hidden,
                        };
                        eprintln!("[ChartApp] scales:time_scale_bottom toggled: {:?}", w.scale_settings.time_scale_position);
                    }
                }
                "auto_scale" => {
                    let next = self.panel_app.panel_grid
                        .active_window()
                        .map(|w| w.price_scale.scale_mode.next());
                    if let Some(next) = next {
                        if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                            w.price_scale.scale_mode = next;
                            if next.is_auto_y() {
                                w.calc_auto_scale();
                            }
                            let is_auto = next.is_auto_y();
                            for sp in &mut w.sub_panes {
                                // update_sub_pane_ranges() already bakes in symmetrization
                                // and 5% padding, so price_min/price_max already match
                                // what is displayed.  Just flip the flag.
                                sp.auto_scale = is_auto;
                            }
                            eprintln!("[ChartApp] scales:auto_scale -> {:?}", next);
                        }
                        // Propagate scale_mode change to sync-group peers.
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            let viewport_state = self.panel_app.panel_grid
                                .active_window()
                                .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                            if let Some((view_start, bar_spacing)) = viewport_state {
                                self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, Some(next));
                            }
                        }
                    }
                }
                "show_bar_countdown" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.show_bar_countdown = !w.scale_settings.show_bar_countdown;
                        eprintln!("[ChartApp] scales:show_bar_countdown = {}", w.scale_settings.show_bar_countdown);
                    }
                }
                "show_prev_close" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.show_prev_close_line = !w.scale_settings.show_prev_close_line;
                        w.update_prev_close_line();
                        eprintln!("[ChartApp] scales:show_prev_close = {}", w.scale_settings.show_prev_close_line);
                    }
                }
                "use_24h" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.time_format.use_24h = !w.scale_settings.time_format.use_24h;
                        eprintln!("[ChartApp] scales:use_24h = {}", w.scale_settings.time_format.use_24h);
                    }
                }
                "show_day_of_week" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.time_format.show_day_of_week = !w.scale_settings.time_format.show_day_of_week;
                        eprintln!("[ChartApp] scales:show_day_of_week = {}", w.scale_settings.time_format.show_day_of_week);
                    }
                }
                "crosshair_line_color" => {
                    let current_color: Option<String> = self.panel_app.panel_grid.active_window()
                        .map(|w| w.crosshair_options.vert_line.color.clone());
                    let widget_id_str = format!("chart_settings:item:{}", item_id);
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId::from(widget_id_str));
                    let (ax, ay, aw, ah) = rect.map(|r| (r.x, r.y, r.width, r.height)).unwrap_or((0.0, 0.0, 0.0, 0.0));
                    self.panel_app.chart_settings_state.open_color_picker_smart(
                        "crosshair_line_color", ax, ay, aw, ah, screen_w, screen_h, current_color.as_deref(),
                    );
                    eprintln!("[ChartApp] chart_settings opened color picker for crosshair_line_color");
                }
                "price_width_increase" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let new_w = (w.scale_settings.price_scale_width + 10.0).min(150.0);
                        w.scale_settings.price_scale_width = new_w;
                        eprintln!("[ChartApp] price_scale_width = {}", new_w);
                    }
                }
                "price_width_decrease" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let new_w = (w.scale_settings.price_scale_width - 10.0).max(50.0);
                        w.scale_settings.price_scale_width = new_w;
                        eprintln!("[ChartApp] price_scale_width = {}", new_w);
                    }
                }
                "time_height_increase" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let new_h = (w.scale_settings.time_scale_height + 5.0).min(60.0);
                        w.scale_settings.time_scale_height = new_h;
                        eprintln!("[ChartApp] time_scale_height = {}", new_h);
                    }
                }
                "time_height_decrease" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let new_h = (w.scale_settings.time_scale_height - 5.0).max(20.0);
                        w.scale_settings.time_scale_height = new_h;
                        eprintln!("[ChartApp] time_scale_height = {}", new_h);
                    }
                }
                "dropdown_cycle:date_format" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.legend.position = match w.legend.position {
                            LegendPosition::TopLeft    => LegendPosition::TopRight,
                            LegendPosition::TopRight   => LegendPosition::BottomRight,
                            LegendPosition::BottomRight => LegendPosition::BottomLeft,
                            LegendPosition::BottomLeft  => LegendPosition::TopLeft,
                        };
                        eprintln!("[ChartApp] legend position cycled: {:?}", w.legend.position);
                    }
                }
                "dropdown_menu:legend_position" => {
                    self.panel_app.chart_settings_state.toggle_dropdown("legend_position");
                }
                "legend_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.legend.position = match w.legend.position {
                            LegendPosition::TopLeft    => LegendPosition::TopRight,
                            LegendPosition::TopRight   => LegendPosition::BottomLeft,
                            LegendPosition::BottomLeft  => LegendPosition::BottomRight,
                            LegendPosition::BottomRight => LegendPosition::TopLeft,
                        };
                    }
                }
                "legend_show_ohlc" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.legend.show_ohlc = !w.legend.show_ohlc;
                        eprintln!("[ChartApp] legend_show_ohlc = {}", w.legend.show_ohlc);
                    }
                }
                "legend_show_change" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.legend.show_change = !w.legend.show_change;
                        eprintln!("[ChartApp] legend_show_change = {}", w.legend.show_change);
                    }
                }
                "legend_show_percent" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.legend.show_percent = !w.legend.show_percent;
                        eprintln!("[ChartApp] legend_show_percent = {}", w.legend.show_percent);
                    }
                }
                "tooltip_visible" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.tooltip.visible = !w.tooltip.visible;
                        eprintln!("[ChartApp] tooltip_visible = {}", w.tooltip.visible);
                    }
                }
                "tooltip_follow" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.tooltip.follow_cursor = !w.tooltip.follow_cursor;
                        eprintln!("[ChartApp] tooltip_follow = {}", w.tooltip.follow_cursor);
                    }
                }
                "dropdown_cycle:watermark_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(ref mut watermark) = w.watermark {
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(ref mut watermark) = w.watermark {
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(ref mut watermark) = w.watermark {
                            watermark.visible = !watermark.visible;
                            eprintln!("[ChartApp] watermark_visible = {}", watermark.visible);
                        }
                    }
                }
                "watermark_color" => {
                    let current_color: Option<String> = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.watermark.as_ref())
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
                    use zengeld_chart::ui::modal_settings::TextEditingState;
                    let current_text = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.watermark.as_ref())
                        .and_then(|wm| wm.lines.first())
                        .map(|line| line.text.clone())
                        .unwrap_or_default();
                    let cursor = current_text.chars().count();
                    self.panel_app.chart_settings_state.editing_text = Some(TextEditingState {
                        field_id: "status:watermark_text".to_string(),
                        text: current_text,
                        cursor,
                        selection_start: Some(0),
                        blink_time: 0,
                    });
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
            let field = parts.next().unwrap_or("").to_string();
            let value = parts.next().unwrap_or("").to_string();

            match field.as_str() {
                "crosshair_mode" => {
                    let new_mode = match value.as_str() {
                        "Normal" => CrosshairMode::Normal,
                        "Magnet" => CrosshairMode::Magnet,
                        "MagnetOHLC" => CrosshairMode::MagnetOHLC,
                        "Hidden" => CrosshairMode::Hidden,
                        _ => CrosshairMode::Normal,
                    };
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.crosshair.mode = new_mode;
                    }
                    eprintln!("[ChartApp] crosshair_mode = {:?}", new_mode);
                }
                "crosshair_line_style" => {
                    use zengeld_chart::drawing::primitives_v2::LineStyle;
                    let new_style = match value.as_str() {
                        "Solid" => LineStyle::Solid,
                        "Dashed" => LineStyle::Dashed,
                        "Dotted" => LineStyle::Dotted,
                        "LargeDashed" => LineStyle::LargeDashed,
                        "SparseDotted" => LineStyle::SparseDotted,
                        _ => LineStyle::Solid,
                    };
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.crosshair_options.vert_line.style = new_style;
                        w.crosshair_options.horz_line.style = new_style;
                    }
                    eprintln!("[ChartApp] crosshair_line_style = {:?}", new_style);
                }
                "price_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.corner_visibility = match value.as_str() {
                            "always" => ScaleCornerVisibility::Always,
                            "never"  => ScaleCornerVisibility::Never,
                            _ => ScaleCornerVisibility::Always,
                        };
                        eprintln!("[ChartApp] corner_visibility = {:?}", w.scale_settings.corner_visibility);
                    }
                }
                "date_format" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.legend.position = new_pos;
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(ref mut watermark) = w.watermark {
                            watermark.horz_align = horz;
                            watermark.vert_align = vert;
                            eprintln!("[ChartApp] watermark_position = {:?} {:?}", horz, vert);
                        }
                    }
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.user_precision = cycle_precision(w.scale_settings.user_precision);
                        w.price_scale.user_precision = w.scale_settings.user_precision;
                        eprintln!("[ChartApp] precision cycled: {:?}", w.scale_settings.user_precision);
                    }
                }
                "timezone" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.time_format.cycle_timezone();
                        eprintln!("[ChartApp] timezone cycled: {}", w.scale_settings.time_format.timezone_label());
                    }
                }
                "crosshair_mode" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
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
                    use zengeld_chart::drawing::primitives_v2::LineStyle;
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let new_style = match w.crosshair_options.vert_line.style {
                            LineStyle::Solid       => LineStyle::Dashed,
                            LineStyle::Dashed      => LineStyle::Dotted,
                            LineStyle::Dotted      => LineStyle::LargeDashed,
                            LineStyle::LargeDashed => LineStyle::SparseDotted,
                            LineStyle::SparseDotted => LineStyle::Solid,
                        };
                        w.crosshair_options.vert_line.style = new_style;
                        w.crosshair_options.horz_line.style = new_style;
                        eprintln!("[ChartApp] crosshair_line_style cycled: {:?}", new_style);
                    }
                }
                "price_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.price_scale_position = w.scale_settings.price_scale_position.next();
                        eprintln!("[ChartApp] price_position cycled: {:?}", w.scale_settings.price_scale_position);
                    }
                }
                "time_position" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.time_scale_position = w.scale_settings.time_scale_position.next();
                        eprintln!("[ChartApp] time_position cycled: {:?}", w.scale_settings.time_scale_position);
                    }
                }
                "corner_visibility" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.corner_visibility = w.scale_settings.corner_visibility.next();
                        eprintln!("[ChartApp] corner_visibility cycled: {:?}", w.scale_settings.corner_visibility);
                    }
                }
                "price_tick_style" => {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.scale_settings.price_tick_style = match w.scale_settings.price_tick_style.as_str() {
                            "dotted" => "dashed".to_string(),
                            "dashed" => "solid".to_string(),
                            _        => "dotted".to_string(),
                        };
                        eprintln!("[ChartApp] price_tick_style cycled: {}", w.scale_settings.price_tick_style);
                    }
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
        use zengeld_chart::ui::modal_settings::PrimitiveSettingsTab;

        // Auto-commit: if an input field is being edited and the click lands on a
        // DIFFERENT widget inside the same modal, commit the value (same as Enter).
        if self.panel_app.primitive_settings_state.editing_text.is_some() {
            let editing_field = self.panel_app.primitive_settings_state.editing_text
                .as_ref()
                .map(|e| e.field_id.clone())
                .unwrap_or_default();
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
                let already_editing = self.panel_app.primitive_settings_state.editing_text
                    .as_ref()
                    .map(|e| e.field_id == item_id)
                    .unwrap_or(false);
                if already_editing {
                    let char_positions: Vec<f64> = self.frame_result.as_ref()
                        .and_then(|r| r.primitive_settings.as_ref())
                        .map(|ps| ps.active_input_char_positions.clone())
                        .unwrap_or_default();
                    if !char_positions.is_empty() {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(ref mut edit) = self.panel_app.primitive_settings_state.editing_text {
                            edit.cursor = new_cursor;
                            edit.selection_start = None;
                            edit.reset_blink(0);
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
    fn commit_prim_editing_text(&mut self) {
        let (text, field) = match self.panel_app.primitive_settings_state.editing_text.take() {
            Some(e) => (e.text, e.field_id),
            None => return,
        };

        if field == "text_content" {
            if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(mut data) = window.drawing_manager.get_data_at(idx) {
                        if let Some(ref mut t) = data.text {
                            t.content = text.clone();
                        } else {
                            use zengeld_chart::drawing::primitives_v2::PrimitiveText;
                            data.text = Some(PrimitiveText {
                                content: text.clone(),
                                ..PrimitiveText::default()
                            });
                        }
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                }
            }
            self.sync_drawing_back_to_group();
            eprintln!("[ChartApp] prim_settings text_content auto-committed: {}", text);
        } else if field == "stroke_width_value" || field == "stroke_width" {
            if let Ok(width) = text.trim().parse::<f64>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(mut data) = window.drawing_manager.get_data_at(idx) {
                            data.width = width.clamp(0.5, 20.0);
                            window.drawing_manager.set_data_at(idx, &data);
                        }
                    }
                }
            }
            self.sync_drawing_back_to_group();
        } else if field == "text_font_size" {
            if let Ok(font_size) = text.trim().parse::<f64>() {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.set_selected_text_font_size(font_size);
                }
            }
            eprintln!("[ChartApp] prim_settings text_font_size auto-committed: {}", text);
        } else if field.starts_with("tf_") && field.ends_with("_min") {
            if let Ok(val) = text.trim().parse::<u32>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(tf_idx) = field.strip_prefix("tf_")
                        .and_then(|s| s.strip_suffix("_min"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        self.apply_tf_min_value(idx, tf_idx, val);
                    }
                }
            }
        } else if field.starts_with("tf_") && field.ends_with("_max") {
            if let Ok(val) = text.trim().parse::<u32>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(tf_idx) = field.strip_prefix("tf_")
                        .and_then(|s| s.strip_suffix("_max"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        self.apply_tf_max_value(idx, tf_idx, val);
                    }
                }
            }
        } else if field.starts_with("level_") && field.ends_with("_value") {
            if let Ok(val) = text.trim().parse::<f64>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(level_idx) = field.strip_prefix("level_")
                        .and_then(|s| s.strip_suffix("_value"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let prims = window.drawing_manager.primitives_mut();
                            if idx < prims.len() {
                                if let Some(mut configs) = prims[idx].level_configs() {
                                    if level_idx < configs.len() {
                                        configs[level_idx].level = val;
                                        prims[idx].set_level_configs(configs);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if field.starts_with("coord_") && field.ends_with("_price") {
            if let Ok(price) = text.trim().parse::<f64>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(pt_idx) = field.strip_prefix("coord_")
                        .and_then(|s| s.strip_suffix("_price"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let prims = window.drawing_manager.primitives_mut();
                            if idx < prims.len() {
                                let mut pts = prims[idx].points().to_vec();
                                if pt_idx < pts.len() {
                                    pts[pt_idx].1 = price;
                                    prims[idx].set_points(&pts);
                                }
                            }
                        }
                    }
                }
            }
        } else if field.starts_with("coord_") && field.ends_with("_bar") {
            if let Ok(bar) = text.trim().parse::<f64>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(pt_idx) = field.strip_prefix("coord_")
                        .and_then(|s| s.strip_suffix("_bar"))
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let ts_ms = zengeld_chart::bar_f64_to_timestamp_ms(&window.bars, bar);
                            let prims = window.drawing_manager.primitives_mut();
                            if idx < prims.len() {
                                let mut pts = prims[idx].points().to_vec();
                                if pt_idx < pts.len() {
                                    pts[pt_idx].0 = ts_ms;
                                    prims[idx].set_points(&pts);
                                }
                            }
                        }
                    }
                }
            }
        } else if let Some(prop_id) = field.strip_prefix("text_prop:") {
            use zengeld_chart::drawing::primitives_v2::config::{PropertyValue, PropertyType};
            if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                let prop_type_opt = self.panel_app.panel_grid.active_window()
                    .and_then(|win| {
                        let prims = win.drawing_manager.primitives();
                        prims.get(idx).and_then(|p| {
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.apply_text_property(idx, prop_id, value);
                }
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            eprintln!("[ChartApp] prim_settings text_prop '{}' auto-committed: {}", prop_id, text);
        } else if let Some(prop_id) = field.strip_prefix("style_prop:") {
            use zengeld_chart::drawing::primitives_v2::config::PropertyValue;
            if let Ok(val) = text.trim().parse::<f64>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.apply_style_property(idx, prop_id, PropertyValue::Number(val));
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
        if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
            self.snapshot_primitive_settings_to_user_manager(idx);
        }
    }

    /// Handle a click on a specific primitive settings item.
    fn handle_prim_settings_item(&mut self, item_id: &str, x: f64, _y: f64) {
        use zengeld_chart::drawing::primitives_v2::{LineStyle, TextAlign};
        use zengeld_chart::drawing::primitives_v2::config::{PropertyValue, PropertyType};

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

        // ── Footer: OK / Cancel ───────────────────────────────────────────────
        if item_id == "ok" || item_id == "cancel" {
            self.panel_app.primitive_settings_state.close();
            eprintln!("[ChartApp] prim_settings closed via: {}", item_id);
            return;
        }

        // Get selected primitive index from settings state.
        let idx = match self.panel_app.primitive_settings_state.primitive_idx {
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
            let current_color: Option<String> = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx))
                .and_then(|data| match item_id {
                    "stroke_color" => Some(data.color.stroke.clone()),
                    "fill_color"   => data.color.fill.clone(),
                    "text_color"   => data.text.as_ref().and_then(|t| t.color.clone())
                                          .or_else(|| {
                                              // fallback to stroke color
                                              self.panel_app.panel_grid.active_window()
                                                  .and_then(|win| win.drawing_manager.get_data_at(idx))
                                                  .map(|d| d.color.stroke.clone())
                                          }),
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
            let data_opt = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx));
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
                eprintln!("[ChartApp] prim_settings line_style cycled to: {}", style_str);
            }
            self.snapshot_primitive_settings_to_user_manager(idx);
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
                let data_opt = self.panel_app.panel_grid.active_window()
                    .and_then(|win| win.drawing_manager.get_data_at(idx));
                if let Some(mut data) = data_opt {
                    data.style = style;
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings line_style set to: {}", style_name);
                }
            }
            self.panel_app.primitive_settings_state.open_line_style_dropdown = false;
            self.snapshot_primitive_settings_to_user_manager(idx);
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
                    let new_val = zengeld_chart::drawing::primitives_v2::config::PropertyValue::String(value.to_string());
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        if prefix.starts_with("style") {
                            w.drawing_manager.apply_style_property(idx, prop_id, new_val);
                        } else if prefix.starts_with("text") {
                            w.drawing_manager.apply_text_property(idx, prop_id, new_val);
                        } else {
                            w.drawing_manager.apply_level_property(idx, prop_id, new_val);
                        }
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
            use zengeld_chart::ui::modal_settings::TextEditingState;
            let current_width = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx))
                .map(|data| format!("{}", data.width as u32))
                .unwrap_or_default();
            let cursor = current_width.chars().count();
            self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                field_id: "stroke_width_value".to_string(),
                text: current_width,
                cursor,
                selection_start: None,
                blink_time: 0,
            });
            return;
        }

        // ── Text content — start inline text editing ──────────────────────────
        if item_id == "text_content" {
            use zengeld_chart::ui::modal_settings::TextEditingState;
            // Read the current text value from the primitive
            let current_text = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx))
                .and_then(|data| data.text.map(|t| t.content.clone()))
                .unwrap_or_default();
            let cursor = current_text.chars().count();
            self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                field_id: "text_content".to_string(),
                text: current_text,
                cursor,
                selection_start: None,
                blink_time: 0,
            });
            eprintln!("[ChartApp] prim_settings text_content editing started");
            return;
        }

        // ── Text font size — start inline text editing ────────────────────────
        if item_id == "text_font_size" {
            use zengeld_chart::ui::modal_settings::TextEditingState;
            let current_font_size = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx))
                .and_then(|data| data.text.map(|t| format!("{:.0}", t.font_size)))
                .unwrap_or_else(|| "14".to_string());
            let cursor = current_font_size.chars().count();
            self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                field_id: "text_font_size".to_string(),
                text: current_font_size,
                cursor,
                selection_start: Some(0),
                blink_time: 0,
            });
            eprintln!("[ChartApp] prim_settings text_font_size editing started");
            return;
        }

        // ── Text bold toggle ──────────────────────────────────────────────────
        if item_id == "text_bold" {
            let data_opt = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx));
            if let Some(mut data) = data_opt {
                if let Some(ref mut text) = data.text {
                    text.bold = !text.bold;
                    let new_bold = text.bold;
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings text_bold = {}", new_bold);
                }
            }
            return;
        }

        // ── Text italic toggle ────────────────────────────────────────────────
        if item_id == "text_italic" {
            let data_opt = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx));
            if let Some(mut data) = data_opt {
                if let Some(ref mut text) = data.text {
                    text.italic = !text.italic;
                    let new_italic = text.italic;
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.set_data_at(idx, &data);
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
                let data_opt = self.panel_app.panel_grid.active_window()
                    .and_then(|win| win.drawing_manager.get_data_at(idx));
                if let Some(mut data) = data_opt {
                    if data.text.is_none() {
                        data.text = Some(zengeld_chart::drawing::primitives_v2::PrimitiveText::default());
                    }
                    if let Some(ref mut text) = data.text {
                        text.v_align = v_align;
                        text.h_align = h_align;
                    }
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings text_pos: {}", item_id);
                }
            }
            return;
        }

        // ── Timeframe visibility toggle: tf_{i}_toggle ────────────────────────
        if item_id.starts_with("tf_") && item_id.ends_with("_toggle") {
            let data_opt = self.panel_app.panel_grid.active_window()
                .and_then(|win| win.drawing_manager.get_data_at(idx));
            if let Some(mut data) = data_opt {
                let mut tf_config = data.timeframe_visibility.clone()
                    .unwrap_or_else(zengeld_chart::drawing::TimeframeVisibilityConfig::all);
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
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] prim_settings tf_{}_toggle toggled", tf_idx);
                }
            }
            return;
        }

        // ── Timeframe min/max inputs — start inline text editing ─────────────
        if item_id.starts_with("tf_") && (item_id.ends_with("_min") || item_id.ends_with("_max")) {
            use zengeld_chart::ui::modal_settings::TextEditingState;
            use zengeld_chart::drawing::TimeframeVisibilityConfig;

            let is_min = item_id.ends_with("_min");
            let current_val: String = if let Some(tf_idx) = item_id.strip_prefix("tf_")
                .and_then(|s| if is_min { s.strip_suffix("_min") } else { s.strip_suffix("_max") })
                .and_then(|s| s.parse::<usize>().ok())
            {
                self.panel_app.panel_grid.active_window()
                    .and_then(|win| win.drawing_manager.get_data_at(idx))
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
            let cursor = current_val.chars().count();
            self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                field_id: item_id.to_string(),
                text: current_val,
                cursor,
                selection_start: Some(0),
                blink_time: 0,
            });
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
                    self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.drawing_manager.get_data_at(idx))
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
                    let window_ptr: *mut _ = self.panel_app.panel_grid.active_window_mut()
                        .map(|w| w as *mut _)
                        .unwrap_or(std::ptr::null_mut::<zengeld_chart::ChartWindow>());
                    if !window_ptr.is_null() {
                        let w = unsafe { &mut *window_ptr };
                        let prims = w.drawing_manager.primitives_mut();
                        if idx < prims.len() {
                            if let Some(mut configs) = prims[idx].level_configs() {
                                if level_idx < configs.len() {
                                    configs[level_idx].visible = !configs[level_idx].visible;
                                    let new_vis = configs[level_idx].visible;
                                    prims[idx].set_level_configs(configs);
                                    eprintln!("[ChartApp] prim_settings level_{}_visible = {}", level_idx, new_vis);
                                }
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
                    let window_ptr: *mut _ = self.panel_app.panel_grid.active_window_mut()
                        .map(|w| w as *mut _)
                        .unwrap_or(std::ptr::null_mut::<zengeld_chart::ChartWindow>());
                    if !window_ptr.is_null() {
                        let w = unsafe { &mut *window_ptr };
                        let prims = w.drawing_manager.primitives_mut();
                        if idx < prims.len() {
                            if let Some(mut configs) = prims[idx].level_configs() {
                                if level_idx < configs.len() {
                                    configs[level_idx].fill_enabled = !configs[level_idx].fill_enabled;
                                    let new_fill = configs[level_idx].fill_enabled;
                                    prims[idx].set_level_configs(configs);
                                    eprintln!("[ChartApp] prim_settings level_{}_fill = {}", level_idx, new_fill);
                                }
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
                    let current_color: Option<String> = self.panel_app.panel_grid.active_window()
                        .and_then(|win| {
                            let prims = win.drawing_manager.primitives();
                            if idx < prims.len() {
                                if let Some(configs) = prims[idx].level_configs() {
                                    if level_idx < configs.len() {
                                        return configs[level_idx].color.clone()
                                            .or_else(|| Some(prims[idx].data().color.stroke.clone()));
                                    }
                                }
                            }
                            None
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
            use zengeld_chart::ui::modal_settings::TextEditingState;
            let parts: Vec<&str> = item_id.split('_').collect();
            let current_val: String = if parts.len() >= 3 {
                if let Ok(level_idx) = parts[1].parse::<usize>() {
                    self.panel_app.panel_grid.active_window()
                        .and_then(|win| {
                            let prims = win.drawing_manager.primitives();
                            if idx < prims.len() {
                                if let Some(configs) = prims[idx].level_configs() {
                                    if level_idx < configs.len() {
                                        return Some(format!("{:.4}", configs[level_idx].level));
                                    }
                                }
                            }
                            None
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let cursor = current_val.chars().count();
            self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                field_id: item_id.to_string(),
                text: current_val,
                cursor,
                selection_start: Some(0),
                blink_time: 0,
            });
            eprintln!("[ChartApp] prim_settings {} editing started", item_id);
            return;
        }

        // ── Coordinate editing: coord_{idx}_{price|bar} ── start inline editing
        if item_id.starts_with("coord_") {
            use zengeld_chart::ui::modal_settings::TextEditingState;
            let is_price = item_id.ends_with("_price");
            let current_val: String = if let Some(pt_idx) = item_id.strip_prefix("coord_")
                .and_then(|s| if is_price { s.strip_suffix("_price") } else { s.strip_suffix("_bar") })
                .and_then(|s| s.parse::<usize>().ok())
            {
                self.panel_app.panel_grid.active_window()
                    .and_then(|win| {
                        let prims = win.drawing_manager.primitives();
                        if idx < prims.len() {
                            let pts = prims[idx].points();
                            if pt_idx < pts.len() {
                                if is_price {
                                    return Some(format!("{:.4}", pts[pt_idx].1));
                                } else {
                                    return Some(format!("{:.0}", pts[pt_idx].0));
                                }
                            }
                        }
                        None
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let cursor = current_val.chars().count();
            self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                field_id: item_id.to_string(),
                text: current_val,
                cursor,
                selection_start: Some(0),
                blink_time: 0,
            });
            eprintln!("[ChartApp] prim_settings {} editing started", item_id);
            return;
        }

        // ── Style properties: style_prop:{id} ────────────────────────────────
        if let Some(prop_id) = item_id.strip_prefix("style_prop:") {
            // Collect property info under immutable borrow
            let prop_info: Option<(PropertyType, PropertyValue)> = self.panel_app.panel_grid.active_window()
                .and_then(|win| {
                    let prims = win.drawing_manager.primitives();
                    if idx < prims.len() {
                        let style_props = prims[idx].style_properties();
                        style_props.iter().find(|p| p.id == prop_id)
                            .map(|p| (p.prop_type.clone(), p.value.clone()))
                    } else {
                        None
                    }
                });

            if let Some((prop_type, prop_value)) = prop_info {
                match prop_type {
                    PropertyType::Boolean => {
                        let current = prop_value.as_bool().unwrap_or(false);
                        let new_val = PropertyValue::Boolean(!current);
                        if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                            w.drawing_manager.apply_style_property(idx, prop_id, new_val);
                        }
                        self.sync_drawing_back_to_group();
                        self.autosave_snapshot();
                        eprintln!("[ChartApp] prim_settings style_prop '{}' toggled: {} -> {}", prop_id, current, !current);
                    }
                    PropertyType::Select { ref options } => {
                        let current = prop_value.as_string().unwrap_or("").to_string();
                        let current_idx = options.iter().position(|o| o.value == current).unwrap_or(0);
                        let next_idx = (current_idx + 1) % options.len().max(1);
                        if let Some(next_opt) = options.get(next_idx) {
                            let new_val = PropertyValue::String(next_opt.value.clone());
                            let next_value = next_opt.value.clone();
                            if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                                w.drawing_manager.apply_style_property(idx, prop_id, new_val);
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
                        use zengeld_chart::ui::modal_settings::TextEditingState;
                        let current_val = match prop_value {
                            PropertyValue::Number(f) => format!("{}", f),
                            PropertyValue::Integer(i) => format!("{}", i),
                            _ => String::new(),
                        };
                        let cursor = current_val.chars().count();
                        self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                            field_id: item_id.to_string(),
                            text: current_val,
                            cursor,
                            selection_start: Some(0),
                            blink_time: 0,
                        });
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
            let prop_info: Option<(PropertyType, PropertyValue)> = self.panel_app.panel_grid.active_window()
                .and_then(|win| {
                    let prims = win.drawing_manager.primitives();
                    if idx < prims.len() {
                        let level_props = prims[idx].level_properties();
                        level_props.iter().find(|p| p.id == prop_id)
                            .map(|p| (p.prop_type.clone(), p.value.clone()))
                    } else {
                        None
                    }
                });

            if let Some((prop_type, prop_value)) = prop_info {
                match prop_type {
                    PropertyType::Boolean => {
                        let current = prop_value.as_bool().unwrap_or(false);
                        let new_val = PropertyValue::Boolean(!current);
                        if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                            w.drawing_manager.apply_level_property(idx, prop_id, new_val);
                        }
                        eprintln!("[ChartApp] prim_settings level_prop '{}' toggled: {} -> {}", prop_id, current, !current);
                    }
                    PropertyType::Select { ref options } => {
                        let current = prop_value.as_string().unwrap_or("").to_string();
                        let current_idx = options.iter().position(|o| o.value == current).unwrap_or(0);
                        let next_idx = (current_idx + 1) % options.len().max(1);
                        if let Some(next_opt) = options.get(next_idx) {
                            let new_val = PropertyValue::String(next_opt.value.clone());
                            let next_value = next_opt.value.clone();
                            if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                                w.drawing_manager.apply_level_property(idx, prop_id, new_val);
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
            let prop_info: Option<(PropertyType, PropertyValue)> = self.panel_app.panel_grid.active_window()
                .and_then(|win| {
                    let prims = win.drawing_manager.primitives();
                    if idx < prims.len() {
                        prims[idx].text_properties().and_then(|text_props| {
                            text_props.iter().find(|p| p.id == prop_id)
                                .map(|p| (p.prop_type.clone(), p.value.clone()))
                        })
                    } else {
                        None
                    }
                });

            if let Some((prop_type, prop_value)) = prop_info {
                match prop_type {
                    PropertyType::Boolean => {
                        let current = prop_value.as_bool().unwrap_or(false);
                        let new_val = PropertyValue::Boolean(!current);
                        if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                            w.drawing_manager.apply_text_property(idx, prop_id, new_val);
                        }
                        eprintln!("[ChartApp] prim_settings text_prop '{}' toggled: {} -> {}", prop_id, current, !current);
                    }
                    PropertyType::Select { ref options } => {
                        let current = prop_value.as_string().unwrap_or("").to_string();
                        let current_idx = options.iter().position(|o| o.value == current).unwrap_or(0);
                        let next_idx = (current_idx + 1) % options.len().max(1);
                        if let Some(next_opt) = options.get(next_idx) {
                            let new_val = PropertyValue::String(next_opt.value.clone());
                            let next_value = next_opt.value.clone();
                            if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                                w.drawing_manager.apply_text_property(idx, prop_id, new_val);
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
                        use zengeld_chart::ui::modal_settings::TextEditingState;
                        let current_val = match &prop_value {
                            PropertyValue::String(s) => s.clone(),
                            PropertyValue::Number(f) => format!("{}", f),
                            PropertyValue::Integer(i) => format!("{}", i),
                            _ => String::new(),
                        };
                        let cursor = current_val.chars().count();
                        self.panel_app.primitive_settings_state.editing_text = Some(TextEditingState {
                            field_id: item_id.to_string(),
                            text: current_val,
                            cursor,
                            selection_start: Some(0),
                            blink_time: 0,
                        });
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
            if let Some(tmpl) = self.panel_app.template_manager.get_primitive_template(tmpl_id).cloned() {
                self.panel_app.primitive_settings_state.applied_template_id = Some(tmpl.id.clone());
                self.panel_app.primitive_settings_state.template_dropdown_open = false;
                // Apply the template to the primitive
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(mut data) = window.drawing_manager.get_data_at(idx) {
                        tmpl.apply_to_primitive_data(&mut data);
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                }
                self.sync_drawing_back_to_group();
                self.autosave_snapshot();
                self.snapshot_primitive_settings_to_user_manager(idx);
                eprintln!("[ChartApp] prim_settings template applied: {}", tmpl.name);
            }
            return;
        }

        // ── Template "Save as..." (from dropdown) ────────────────────────────
        if item_id == "template_save_as" {
            self.panel_app.primitive_settings_state.template_dropdown_open = false;
            self.panel_app.primitive_settings_state.save_template_mode = true;
            let prefix = "Мой шаблон ";
            let max_n = self.panel_app.template_manager.primitive_templates
                .iter()
                .filter_map(|t| t.name.strip_prefix(prefix))
                .filter_map(|s| s.parse::<u32>().ok())
                .max()
                .unwrap_or(0);
            let default_name = format!("{}{}", prefix, max_n + 1);
            let default_cursor = default_name.chars().count();
            self.panel_app.primitive_settings_state.template_name_editing = Some(
                zengeld_chart::ui::modal_settings::TextEditingState {
                    field_id: "template_name".to_string(),
                    text: default_name,
                    cursor: default_cursor,
                    selection_start: None,
                    blink_time: 0,
                }
            );
            eprintln!("[ChartApp] prim_settings template save-as opened");
            return;
        }

        // ── Template "По умолчанию" (from dropdown) ───────────────────────────
        if item_id == "template_default" {
            self.panel_app.primitive_settings_state.template_dropdown_open = false;
            self.panel_app.primitive_settings_state.applied_template_id = None;
            // Reset primitive to factory defaults
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                if let Some(data) = window.drawing_manager.get_data_at(idx) {
                    let type_id = data.type_id.clone();
                    let default_tmpl = zengeld_chart::templates::PrimitiveTemplate::new_with_defaults(&type_id, "__default__");
                    let mut data_copy = data;
                    default_tmpl.apply_to_primitive_data(&mut data_copy);
                    window.drawing_manager.set_data_at(idx, &data_copy);
                }
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            self.snapshot_primitive_settings_to_user_manager(idx);
            eprintln!("[ChartApp] prim_settings template reset to defaults");
            return;
        }

        eprintln!("[ChartApp] prim_settings item not handled: {}", item_id);
    }

    // =========================================================================
    // Template name overlay modal click handlers
    // =========================================================================

    /// Handle clicks in the template name modal for primitive settings ("prim_tmpl:*").
    pub(super) fn handle_prim_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {
                // No-op — click inside the modal is absorbed.
            }
            "close" | "cancel" => {
                self.panel_app.primitive_settings_state.save_template_mode = false;
                self.panel_app.primitive_settings_state.template_name_editing = None;
                eprintln!("[ChartApp] prim_tmpl modal cancelled");
            }
            "save" => {
                let name = self.panel_app.primitive_settings_state.template_name_editing
                    .as_ref().map(|e| e.text.clone()).unwrap_or_default();
                let idx = match self.panel_app.primitive_settings_state.primitive_idx {
                    Some(i) => i,
                    None => {
                        self.panel_app.primitive_settings_state.save_template_mode = false;
                        self.panel_app.primitive_settings_state.template_name_editing = None;
                        return;
                    }
                };
                if !name.is_empty() {
                    if let Some(data) = self.panel_app.panel_grid.active_window()
                        .and_then(|win| win.drawing_manager.get_data_at(idx))
                    {
                        let tmpl = zengeld_chart::templates::PrimitiveTemplate::from_primitive_data(&data, &name);
                        eprintln!("[ChartApp] prim_tmpl saved: {}", name);
                        self.template_actions.push(crate::TemplateAction::AddPrimitive(tmpl));
                    }
                }
                self.panel_app.primitive_settings_state.save_template_mode = false;
                self.panel_app.primitive_settings_state.template_name_editing = None;
            }
            _ => {
                eprintln!("[ChartApp] prim_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks in the template name modal for indicator settings ("ind_tmpl:*").
    pub(super) fn handle_ind_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {}
            "close" | "cancel" => {
                self.panel_app.indicator_settings_state.save_template_mode = false;
                self.panel_app.indicator_settings_state.template_name_editing = None;
                eprintln!("[ChartApp] ind_tmpl modal cancelled");
            }
            "save" => {
                let name = self.panel_app.indicator_settings_state.template_name_editing
                    .as_ref().map(|e| e.text.clone()).unwrap_or_default();
                if !name.is_empty() {
                    if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                        let type_id = self.indicator_manager.get_instance(ind_id)
                            .map(|inst| inst.type_id.to_string())
                            .unwrap_or_default();
                        let params = self.indicator_manager.get_instance(ind_id)
                            .map(|inst| {
                                inst.params.iter().map(|(k, v): (&String, &zengeld_terminal_indicators::IndicatorParamValue)| {
                                    let json_val = match v {
                                        zengeld_terminal_indicators::IndicatorParamValue::Int(i) => serde_json::json!(i),
                                        zengeld_terminal_indicators::IndicatorParamValue::Float(f) => serde_json::json!(f),
                                        zengeld_terminal_indicators::IndicatorParamValue::String(s) => serde_json::json!(s),
                                        zengeld_terminal_indicators::IndicatorParamValue::Bool(b) => serde_json::json!(b),
                                        _ => serde_json::Value::Null,
                                    };
                                    (k.clone(), json_val)
                                }).collect::<std::collections::HashMap<_, _>>()
                            })
                            .unwrap_or_default();
                        let tmpl = zengeld_chart::templates::IndicatorTemplate::new(
                            &type_id, &name, true, params, std::collections::HashMap::new(),
                        );
                        eprintln!("[ChartApp] ind_tmpl saved: {}", name);
                        self.template_actions.push(crate::TemplateAction::AddIndicator(tmpl));
                    }
                }
                self.panel_app.indicator_settings_state.save_template_mode = false;
                self.panel_app.indicator_settings_state.template_name_editing = None;
            }
            _ => {
                eprintln!("[ChartApp] ind_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks in the template name modal for compare settings ("cmp_tmpl:*").
    pub(super) fn handle_cmp_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {}
            "close" | "cancel" => {
                self.panel_app.compare_settings_state.save_template_mode = false;
                self.panel_app.compare_settings_state.template_name_editing = None;
                eprintln!("[ChartApp] cmp_tmpl modal cancelled");
            }
            "save" => {
                let name = self.panel_app.compare_settings_state.template_name_editing
                    .as_ref().map(|e| e.text.clone()).unwrap_or_default();
                if !name.is_empty() {
                    let color = self.panel_app.compare_settings_state.cached_color.clone();
                    let line_width = self.panel_app.compare_settings_state.cached_line_width;
                    let line_style = self.panel_app.compare_settings_state.cached_line_style.clone();
                    let tmpl = zengeld_chart::templates::CompareTemplate::new(
                        &name, &color, line_width, &line_style,
                    );
                    eprintln!("[ChartApp] cmp_tmpl saved: {}", name);
                    self.template_actions.push(crate::TemplateAction::AddCompare(tmpl));
                }
                self.panel_app.compare_settings_state.save_template_mode = false;
                self.panel_app.compare_settings_state.template_name_editing = None;
            }
            _ => {
                eprintln!("[ChartApp] cmp_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks in the template name modal for chart settings ("chart_tmpl:*").
    pub(super) fn handle_chart_tmpl_modal_click(&mut self, rest: &str) {
        match rest {
            "modal_bg" | "input" => {
                // No-op — click inside the modal is absorbed.
            }
            "close" | "cancel" => {
                self.panel_app.chart_settings_state.save_template_mode = false;
                self.panel_app.chart_settings_state.template_name_editing = None;
                eprintln!("[ChartApp] chart_tmpl modal cancelled");
            }
            "save" => {
                let name = self.panel_app.chart_settings_state.template_name_editing
                    .as_ref().map(|e| e.text.clone()).unwrap_or_default();
                if !name.is_empty() {
                    let data = self.build_chart_settings_data();
                    let tmpl = zengeld_chart::templates::ChartTemplate::new(&name, &data);
                    eprintln!("[ChartApp] chart_tmpl saved: {}", name);
                    self.template_actions.push(crate::TemplateAction::AddChart(tmpl));
                }
                self.panel_app.chart_settings_state.save_template_mode = false;
                self.panel_app.chart_settings_state.template_name_editing = None;
            }
            _ => {
                eprintln!("[ChartApp] chart_tmpl: unhandled: {}", rest);
            }
        }
    }

    /// Handle clicks on widgets registered with the "alert_set:" prefix.
    pub(super) fn handle_alert_settings_click(&mut self, rest: &str) {
        match rest {
            "close" | "cancel" => {
                self.panel_app.alert_settings_state.close();
            }
            "modal_bg" | "header" => {
                // No-op (catch-all, drag handled separately)
            }
            "save" => {
                let state = &self.panel_app.alert_settings_state;
                let source = state.source.clone();
                let name = state.name.clone();
                let price = state.price;
                let price2 = state.price2;
                let percentage = state.percentage;
                let condition = state.condition;
                let trigger_mode = state.build_trigger_mode();
                let transports = state.build_transports();

                let alert_id = if let Some(existing_id) = state.editing_alert_id {
                    self.alert_manager.update_full(
                        existing_id, source, &name, price, price2, percentage,
                        condition, trigger_mode, transports,
                    );
                    existing_id
                } else {
                    let id = self.alert_manager.create(source, &name, price, condition);
                    // Set extra fields on the newly created alert
                    if let Some(alert) = self.alert_manager.get_mut(id) {
                        alert.price2 = price2;
                        alert.percentage = percentage;
                        alert.trigger_mode = trigger_mode;
                        alert.transports = transports;
                        // Stamp scoping fields from the active window so this alert
                        // is correctly filtered to its symbol:exchange context.
                        if let Some(window) = self.panel_app.panel_grid.active_window() {
                            alert.exchange = window.exchange.clone();
                            alert.timeframe = window.timeframe.name.clone();
                            alert.window_id_hint = Some(window.id.0);
                            alert.group_id = window.group_id.map(|g| g.0);
                            // Drawing/Indicator alerts need symbol stamped explicitly —
                            // Price alerts carry it inside AlertSource::Price { symbol }.
                            if !matches!(alert.source, alerts::AlertSource::Price { .. }) {
                                alert.set_symbol(window.symbol.clone());
                            }
                        }
                    }
                    id
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
                        use zengeld_chart::ui::modal_settings::AlertCondition;
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
                use zengeld_chart::ui::modal_settings::AlertSettingsTab;
                let tab = match rest.strip_prefix("tab:").unwrap_or("") {
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
                    if t == zengeld_chart::ui::modal_settings::AlertSettingsTab::AlertsList {
                        self.panel_app.alert_settings_state.all_alerts =
                            self.alert_manager.items().to_vec();
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
                            0 => Some(alerts::AlertTriggerMode::OneShot),
                            1 => Some(alerts::AlertTriggerMode::EveryTime),
                            2 => Some(alerts::AlertTriggerMode::OncePerBar),
                            3 => Some(alerts::AlertTriggerMode::TimesN(
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
                // Sync token input buffer when enabling
                if s.notification_settings.telegram.enabled {
                    s.tg_bot_token_input = s.notification_settings.telegram.bot_token.clone();
                }
            }
            "notif:tg_token" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.tg_token_focused = true;
            }
            "notif:tg_screenshot" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.telegram.send_screenshots =
                    !s.notification_settings.telegram.send_screenshots;
                s.notification_settings_dirty = true;
            }
            "notif:tg_test" => {
                let s = &mut self.panel_app.alert_settings_state;
                // Sync token buffer back to settings before testing
                s.notification_settings.telegram.bot_token = s.tg_bot_token_input.clone();
                s.tg_test_pending = true;
                s.notification_settings_dirty = true;
                s.tg_status_message = "Sending test...".to_string();
                eprintln!("[AlertSettings] Telegram test requested");
            }
            "notif:tg_detect" => {
                let s = &mut self.panel_app.alert_settings_state;
                s.notification_settings.telegram.bot_token = s.tg_bot_token_input.clone();
                s.tg_detect_pending = true;
                s.notification_settings_dirty = true;
                s.tg_status_message = "Detecting users...".to_string();
                eprintln!("[AlertSettings] Telegram detect users requested");
            }
            _ if rest.starts_with("notif:tg_sub_toggle:") => {
                if let Ok(idx) = rest.strip_prefix("notif:tg_sub_toggle:").unwrap_or("").parse::<usize>() {
                    let s = &mut self.panel_app.alert_settings_state;
                    if let Some(sub) = s.notification_settings.telegram.subscribers.get_mut(idx) {
                        sub.active = !sub.active;
                        s.notification_settings_dirty = true;
                    }
                }
            }
            _ if rest.starts_with("notif:tg_sub_remove:") => {
                if let Ok(idx) = rest.strip_prefix("notif:tg_sub_remove:").unwrap_or("").parse::<usize>() {
                    let s = &mut self.panel_app.alert_settings_state;
                    if idx < s.notification_settings.telegram.subscribers.len() {
                        s.notification_settings.telegram.subscribers.remove(idx);
                        s.notification_settings_dirty = true;
                    }
                }
            }
            _ if rest.starts_with("notif:tg_add_detected:") => {
                if let Ok(idx) = rest.strip_prefix("notif:tg_add_detected:").unwrap_or("").parse::<usize>() {
                    let s = &mut self.panel_app.alert_settings_state;
                    if let Some((cid, name, uname)) = s.tg_detected_users.get(idx).cloned() {
                        // Only add if not already present
                        if !s.notification_settings.telegram.subscribers.iter().any(|sub| sub.chat_id == cid) {
                            s.notification_settings.telegram.subscribers.push(
                                alert_delivery::TelegramSubscriber {
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
                use zengeld_chart::ui::modal_settings::AlertListFilter;
                let filter = match rest.strip_prefix("filter:").unwrap_or("") {
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
                        if let Some(alert) = self.alert_manager.get(id) {
                            self.panel_app.alert_settings_state.open_edit(alert);
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
                        self.alert_manager.remove(id);
                        // Refresh the list
                        self.panel_app.alert_settings_state.all_alerts =
                            self.alert_manager.items().to_vec();
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                }
            }
            // --- Alerts list: pause/resume ---
            _ if rest.starts_with("list_pause:") => {
                if let Some(id_str) = rest.strip_prefix("list_pause:") {
                    if let Ok(id) = id_str.parse::<u64>() {
                        if let Some(alert) = self.alert_manager.get_mut(id) {
                            alert.status = match alert.status {
                                alerts::AlertStatus::Active => alerts::AlertStatus::Paused,
                                alerts::AlertStatus::Paused => alerts::AlertStatus::Active,
                                other => other,
                            };
                        }
                        self.panel_app.alert_settings_state.all_alerts =
                            self.alert_manager.items().to_vec();
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
        use zengeld_chart::ui::modal_settings::IndicatorSettingsTab;

        // Auto-commit: if a param input is being edited and the click lands on a
        // DIFFERENT widget inside the same modal, commit the value (same as Enter).
        if self.panel_app.indicator_settings_state.editing_text_state.is_some() {
            let editing_field = self.panel_app.indicator_settings_state.editing_text_state
                .as_ref()
                .map(|e| e.field_id.clone())
                .unwrap_or_default();
            // The editing field_id is "indicator_param:<name>", the click item_id is
            // "input:<name>", so reconstruct the active_field_id for comparison.
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
                // For "input:<name>" items, editing field_id is "indicator_param:<name>".
                // For "tf_<i>_min" / "tf_<i>_max" items, editing field_id equals item_id directly.
                let already_editing = {
                    let editing_field = self.panel_app.indicator_settings_state.editing_text_state
                        .as_ref()
                        .map(|e| e.field_id.as_str())
                        .unwrap_or("");
                    if let Some(input_name) = item_id.strip_prefix("input:") {
                        // "input:<name>" maps to "indicator_param:<name>"
                        let active_field_id = format!("indicator_param:{}", input_name);
                        editing_field == active_field_id
                    } else if (item_id.starts_with("tf_") && item_id.ends_with("_min"))
                        || (item_id.starts_with("tf_") && item_id.ends_with("_max"))
                    {
                        // tf_ min/max items: field_id stored as item_id (e.g. "tf_1_min")
                        editing_field == item_id
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
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                        if let Some(ref mut edit) = self.panel_app.indicator_settings_state.editing_text_state {
                            edit.cursor = new_cursor;
                            edit.selection_start = None;
                            edit.reset_blink(0);
                        }
                        return;
                    }
                }
                self.handle_ind_settings_item(item_id, x, y);
            }
            _ if rest.starts_with("footer:") => {
                let btn_id = &rest["footer:".len()..];
                match btn_id {
                    "ok" | "cancel" => {
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
                        let max_n = self.panel_app.template_manager.indicator_templates
                            .iter()
                            .filter_map(|t| t.name.strip_prefix(prefix))
                            .filter_map(|s| s.parse::<u32>().ok())
                            .max()
                            .unwrap_or(0);
                        let default_name = format!("{}{}", prefix, max_n + 1);
                        let default_cursor = default_name.chars().count();
                        self.panel_app.indicator_settings_state.template_name_editing = Some(
                            zengeld_chart::ui::modal_settings::TextEditingState {
                                field_id: "template_name".to_string(),
                                text: default_name,
                                cursor: default_cursor,
                                selection_start: None,
                                blink_time: 0,
                            }
                        );
                        eprintln!("[ChartApp] ind_settings template save-as opened");
                    }
                    "template_default" => {
                        self.panel_app.indicator_settings_state.template_dropdown_open = false;
                        self.panel_app.indicator_settings_state.applied_template_id = None;
                        if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                            if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                                inst.params.clear();
                                inst.outputs.clear();  // Reset output styles to defaults
                            }
                            // Recalculate
                            let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                            let active_cid = self.panel_app.panel_grid.active_chart_id().map(|c| c.0).unwrap_or(0);
                            if let Some(bars) = bars_opt {
                                self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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
                        if let Some(tmpl) = self.panel_app.template_manager.get_indicator_template(tmpl_id).cloned() {
                            self.panel_app.indicator_settings_state.applied_template_id = Some(tmpl.id.clone());
                            self.panel_app.indicator_settings_state.template_dropdown_open = false;
                            // Apply params from template to the indicator instance
                            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                                use zengeld_terminal_indicators::IndicatorParamValue as IndValue;
                                if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                                    for (k, v) in &tmpl.params {
                                        let pv = match v {
                                            serde_json::Value::Number(n) if n.is_f64() => IndValue::Float(n.as_f64().unwrap_or(0.0)),
                                            serde_json::Value::Number(n) => IndValue::Int(n.as_i64().unwrap_or(0) as i32),
                                            serde_json::Value::String(s) => IndValue::String(s.clone()),
                                            serde_json::Value::Bool(b) => IndValue::Bool(*b),
                                            _ => continue,
                                        };
                                        inst.set_param(k, pv);
                                    }
                                }
                                let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                                let active_cid = self.panel_app.panel_grid.active_chart_id().map(|c| c.0).unwrap_or(0);
                                if let Some(bars) = bars_opt {
                                    self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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
                    let new_val = self.indicator_manager
                        .get_instance(ind_id)
                        .map(|inst| !inst.signals_enabled);
                    if let Some(enabled) = new_val {
                        if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                            inst.signals_enabled = enabled;
                        }
                        let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                        let active_cid = self.panel_app.panel_grid.active_chart_id().map(|c| c.0).unwrap_or(0);
                        if let Some(bars) = bars_opt {
                            self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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
    fn commit_ind_editing_text(&mut self) {
        let (text, field) = match self.panel_app.indicator_settings_state.editing_text_state.take() {
            Some(e) => (e.text, e.field_id),
            None => return,
        };
        if let Some(param_name) = field.strip_prefix("indicator_param:") {
            use zengeld_terminal_indicators::IndicatorParamValue as IndValue;
            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                    let value = if let Ok(f) = text.trim().parse::<f64>() {
                        IndValue::Float(f)
                    } else if let Ok(i) = text.trim().parse::<i32>() {
                        IndValue::Int(i)
                    } else {
                        IndValue::String(text.trim().to_string())
                    };
                    inst.set_param(param_name, value);
                }
                let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                let active_cid = self.panel_app.panel_grid.active_chart_id().map(|c| c.0).unwrap_or(0);
                if let Some(bars) = bars_opt {
                    self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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

    /// Handle a click on a specific indicator settings item.
    fn handle_ind_settings_item(&mut self, item_id: &str, _x: f64, _y: f64) {
        use zengeld_terminal_indicators::IndicatorParamValue as IndValue;

        // Close template dropdown on any content item click
        self.panel_app.indicator_settings_state.template_dropdown_open = false;

        // ── Color swatch ─────────────────────────────────────────────────────
        if let Some(output_name) = item_id.strip_prefix("color:") {
            let screen_w = self.width as f64;
            let screen_h = self.height as f64;
            let current_color: Option<String> = if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                self.indicator_manager
                    .get_instance(ind_id)
                    .and_then(|inst| inst.outputs.get(output_name))
                    .and_then(|o| o.color.clone())
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
                    self.indicator_manager.get_instance(ind_id).and_then(|inst| {
                        let type_id = inst.type_id.clone();
                        let current_val = inst.params.get(param_name)
                            .and_then(|v| v.as_string())
                            .map(|s| s.to_string());
                        self.indicator_manager.get_definition(&type_id).and_then(|def| {
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
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        inst.set_param(param_name, IndValue::String(next_val.clone()));
                    }
                    // Recalculate.
                    let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                    let active_cid = self.panel_app.panel_grid.active_chart_id().map(|c| c.0).unwrap_or(0);
                    if let Some(bars) = bars_opt {
                        self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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
            let param_name = parts.next().unwrap_or("").to_string();
            let value = parts.next().unwrap_or("").to_string();

            self.panel_app.indicator_settings_state.open_param_dropdown = None;

            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                    inst.set_param(&param_name, IndValue::String(value.clone()));
                }
                let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                let active_cid = self.panel_app.panel_grid.active_chart_id().map(|c| c.0).unwrap_or(0);
                if let Some(bars) = bars_opt {
                    self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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
                let current_bool: Option<bool> = self.indicator_manager
                    .get_instance(ind_id)
                    .and_then(|inst| inst.params.get(param_name))
                    .and_then(|v| v.as_bool());

                if let Some(cur) = current_bool {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        inst.set_param(param_name, IndValue::Bool(!cur));
                    }
                    let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                    let active_cid = self.panel_app.panel_grid.active_chart_id().map(|c| c.0).unwrap_or(0);
                    if let Some(bars) = bars_opt {
                        self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        let mut tf_config = inst.timeframe_visibility.clone()
                            .unwrap_or_else(zengeld_chart::drawing::TimeframeVisibilityConfig::all);
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
                        inst.timeframe_visibility = Some(tf_config);
                        eprintln!("[ChartApp] ind_settings tf_{}_toggle toggled", tf_idx);
                    }
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                }
            }
            return;
        }

        // ── Timeframe min/max inputs — start inline text editing ─────────────
        if item_id.starts_with("tf_") && (item_id.ends_with("_min") || item_id.ends_with("_max")) {
            use zengeld_chart::ui::modal_settings::TextEditingState;
            use zengeld_chart::drawing::TimeframeVisibilityConfig;

            let is_min = item_id.ends_with("_min");
            let current_val: String = if let Some(tf_idx) = item_id.strip_prefix("tf_")
                .and_then(|s| if is_min { s.strip_suffix("_min") } else { s.strip_suffix("_max") })
                .and_then(|s| s.parse::<usize>().ok())
            {
                self.panel_app.indicator_settings_state.indicator_id
                    .and_then(|ind_id| self.indicator_manager.get_instance(ind_id))
                    .map(|inst| {
                        let tf = inst.timeframe_visibility.clone()
                            .unwrap_or_else(TimeframeVisibilityConfig::all);
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
            let cursor = current_val.chars().count();
            self.panel_app.indicator_settings_state.editing_text_state = Some(TextEditingState {
                field_id: item_id.to_string(),
                text: current_val,
                cursor,
                selection_start: Some(0),
                blink_time: 0,
            });
            eprintln!("[ChartApp] ind_settings {} editing started", item_id);
            return;
        }

        // ── Text input — start editing the parameter value ───────────────────
        if let Some(param_name_str) = item_id.strip_prefix("input:") {
            let param_name = param_name_str.to_string();
            let current_value: String = self.panel_app.indicator_settings_state.indicator_id
                .and_then(|ind_id| self.indicator_manager.get_instance(ind_id))
                .and_then(|inst| inst.params.get(&param_name))
                .map(|v| v.to_display_string())
                .unwrap_or_default();
            let cursor = current_value.chars().count();
            let field_id = format!("indicator_param:{}", param_name);
            self.panel_app.indicator_settings_state.editing_text_state = Some(
                zengeld_chart::ui::modal_settings::TextEditingState {
                    field_id,
                    text: current_value,
                    cursor,
                    selection_start: None,
                    blink_time: 0,
                }
            );
            eprintln!("[ChartApp] ind_settings text input editing started: {}", param_name);
            return;
        }

        // ── Signal display config buttons ─────────────────────────────────────
        if item_id.starts_with("ind_set:") {
            use zengeld_chart::indicator_source::SignalShape;
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
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        inst.signal_display.shape = shape;
                        eprintln!("[ChartApp] ind_settings signal_shape = {:?}", shape);
                    }
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_bullish_color" {
                    let screen_w = self.width as f64;
                    let screen_h = self.height as f64;
                    let current_color = self.indicator_manager
                        .get_instance(ind_id)
                        .map(|inst| inst.signal_display.bullish_color.clone());
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
                    let current_color = self.indicator_manager
                        .get_instance(ind_id)
                        .map(|inst| inst.signal_display.bearish_color.clone());
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
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        inst.signal_display.size = (inst.signal_display.size + 2.0).min(24.0);
                        eprintln!("[ChartApp] ind_settings signal_size = {}", inst.signal_display.size);
                    }
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_size_dec" {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        inst.signal_display.size = (inst.signal_display.size - 2.0).max(8.0);
                        eprintln!("[ChartApp] ind_settings signal_size = {}", inst.signal_display.size);
                    }
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_offset_inc" {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        inst.signal_display.offset = (inst.signal_display.offset + 2.0).min(16.0);
                        eprintln!("[ChartApp] ind_settings signal_offset = {}", inst.signal_display.offset);
                    }
                    self.autosave_snapshot();
                    self.snapshot_indicator_settings_to_user_manager();
                } else if item_id == "ind_set:signal_offset_dec" {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                        inst.signal_display.offset = (inst.signal_display.offset - 2.0).max(0.0);
                        eprintln!("[ChartApp] ind_settings signal_offset = {}", inst.signal_display.offset);
                    }
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

}
