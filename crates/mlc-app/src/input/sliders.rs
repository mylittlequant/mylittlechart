//! Slider value appliers — translate slider drag/scroll deltas into state mutations.

use crate::ChartApp;
use mlc_app_layout::ui::modal_settings::DualSliderHandle;
use mlc_core::drawing::TimeframeVisibilityConfig;

impl ChartApp {

    // -------------------------------------------------------------------------
    // Slider value application
    // -------------------------------------------------------------------------

    /// Apply a slider value by routing `field_id` to the correct state mutation.
    ///
    /// Covers chart-relevant sliders only:
    /// - `appearance:style_*`   — theme style params (opacity, blur)
    /// - `scales:*`             — price/time scale dimensions, crosshair width
    /// - `stroke_width`         — primitive stroke width
    /// - `style_prop:*`         — primitive numeric style properties
    /// - `text_prop:*`          — primitive numeric text properties
    /// - `level_prop:*`         — primitive numeric level properties
    pub(crate) fn apply_slider_value(&mut self, field_id: &str, value: f64) {

        // Appearance / style params (glass opacity, blur radius)
        if let Some(param_id) = field_id.strip_prefix("appearance:style_") {
            // strip "appearance:style_"
            let params = &mut self.panel_app.theme_manager.current_mut().style_params;
            match param_id {
                "toolbar_opacity"         => params.toolbar_bg_opacity = value as f32,
                "modal_opacity"           => params.modal_bg_opacity = value as f32,
                "sidebar_opacity"         => params.sidebar_bg_opacity = value as f32,
                "menu_opacity"            => params.menu_bg_opacity = value as f32,
                "scale_opacity"           => params.scale_bg_opacity = value as f32,
                "hover_opacity"           => params.hover_bg_opacity = value as f32,
                "crosshair_label_opacity" => params.crosshair_label_bg_opacity = value as f32,
                "blur_radius"             => params.blur_radius = value as f32,
                _ => log::debug!("[ChartApp] apply_slider_value: unknown style param: {}", param_id),
            }
            self.autosave_snapshot();
            return;
        }

        // Wheel / trackpad feel (chart settings, Scales tab -> Mouse & trackpad):
        // profile-wide multipliers.
        if let Some(which) = field_id.strip_prefix("input:") {
            self.set_input_speed(which, value);
            return;
        }

        // Shared candle-row alpha (`candle_opacity:{body,border,wick}`). The
        // body/border/wick sliders are gone; per-color opacity is the picker.
        // This route still writes both sides (border checkbox included).
        if let Some(row) = field_id.strip_prefix("candle_opacity:") {
            self.apply_candle_opacity(row, value);
            self.autosave_snapshot();
            return;
        }

        // Scales tab sliders
        if field_id.starts_with("scales:") {
            match field_id {
                "scales:price_width_slider" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.price_scale_width = value;
                    }
                }
                "scales:time_height_slider" => {
                    if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                        w.scale_settings.time_scale_height = value;
                    }
                }
                "scales:crosshair_line_width" => {
                    let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                    if let Some(pid) = pid {
                        let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                        panel.crosshair_options.vert_line.width = value;
                        panel.crosshair_options.horz_line.width = value;
                    }
                }
                _ => log::debug!("[ChartApp] apply_slider_value: unknown scales field: {}", field_id),
            }
            self.autosave_snapshot();
            return;
        }

        // Primitive settings sliders
        let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id else { return; };

        if field_id == "stroke_width" {
            let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            if let Some(bid) = active_bid {
                let data_opt = self.core.drawings().get_data(bid, prim_id)
                    .map(|mut d| { d.width = value; d });
                if let Some(data) = data_opt {
                    self.core.drawings_mut().set_data(bid, prim_id, &data);
                }
                // Global writer (item 3, 2026-08-09): the base width also
                // reaches every level's own `cfg.width` directly, exactly
                // the contract the palette already has for colour — see
                // `primitive_settings_intent::apply_global_width`'s own doc.
                let mut guard = self.core.drawings_mut();
                if let Some(mut configs) = guard.primitives(bid).iter().find(|p| p.data().id == prim_id).and_then(|p| p.level_configs()) {
                    mlc_app_layout::layout::modals::primitive_settings_intent::apply_global_width(
                        &mut configs, &self.panel_app.primitive_settings_state, value,
                    );
                    guard.set_level_configs(bid, prim_id, configs);
                }
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            self.snapshot_primitive_settings_to_user_manager(prim_id);
            return;
        }

        if let Some(prop_id) = field_id.strip_prefix("style_prop:") {
            use mlc_core::drawing::primitives_v2::config::PropertyValue;
            let new_val = PropertyValue::Number(value);
            let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            if let Some(bid) = active_bid {
                self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Style, prop_id, &new_val);
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            return;
        }

        if let Some(prop_id) = field_id.strip_prefix("text_prop:") {
            use mlc_core::drawing::primitives_v2::config::PropertyValue;
            let new_val = PropertyValue::Number(value);
            let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            if let Some(bid) = active_bid {
                self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Text, prop_id, &new_val);
            }
            self.autosave_snapshot();
            return;
        }

        if let Some(prop_id) = field_id.strip_prefix("level_prop:") {
            use mlc_core::drawing::primitives_v2::config::PropertyValue;
            let new_val = PropertyValue::Number(value);
            let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            if let Some(bid) = active_bid {
                self.core.drawings_mut().apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Level, prop_id, &new_val);
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            return;
        }

        // Уровни tab — the global "Заливка между" slider: writes the
        // PRIMITIVE's own `fill_opacity` style property directly (the same
        // single field `FibRetracement::render` — and every other
        // level-ladder primitive's own fill — actually reads for every
        // band), not a bulk per-level write. See
        // `mlc_app_layout::layout::modals::primitive_settings_levels::
        // draw_global_opacity_row`'s own doc for why this is the single
        // source of truth (2026-08-08 appearance-surface consolidation —
        // collapses Вид's old `fill_opacity` FILL row into this one).
        if field_id == "levels_global_opacity" {
            use mlc_core::drawing::primitives_v2::config::PropertyValue;
            let clamped_pct = value.clamp(0.0, 1.0) * 100.0;
            let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            if let Some(bid) = active_bid {
                self.core.drawings_mut().apply_property(
                    bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Style, "fill_opacity",
                    &PropertyValue::Number(clamped_pct),
                );
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            return;
        }

        // Уровни tab — one level's own width/fill-opacity, from its
        // inspector row.
        if let Some(key) = field_id.strip_prefix("level_v").and_then(|s| s.strip_suffix("_width")) {
            if let (Ok(level), true) = (key.parse::<f64>(), value > 0.0) {
                let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if let Some(bid) = active_bid {
                    let mut guard = self.core.drawings_mut();
                    if let Some(mut configs) = guard.primitives(bid).iter().find(|p| p.data().id == prim_id).and_then(|p| p.level_configs()) {
                        if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < 1e-9) {
                            cfg.width = Some(value);
                        }
                        guard.set_level_configs(bid, prim_id, configs);
                    }
                }
                self.sync_drawing_back_to_group();
                self.autosave_snapshot();
            }
            return;
        }
        if let Some(key) = field_id.strip_prefix("level_v").and_then(|s| s.strip_suffix("_fill_opacity")) {
            if let Ok(level) = key.parse::<f64>() {
                let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                if let Some(bid) = active_bid {
                    let mut guard = self.core.drawings_mut();
                    if let Some(mut configs) = guard.primitives(bid).iter().find(|p| p.data().id == prim_id).and_then(|p| p.level_configs()) {
                        if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < 1e-9) {
                            cfg.fill_opacity = value.clamp(0.0, 1.0);
                        }
                        guard.set_level_configs(bid, prim_id, configs);
                    }
                }
                self.sync_drawing_back_to_group();
                self.autosave_snapshot();
            }
            return;
        }

        log::warn!("[ChartApp] apply_slider_value: unhandled field_id: {}", field_id);
    }

    /// Apply a dual-handle (min/max range) slider value for `tf_*_slider` fields.
    pub(crate) fn apply_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
        let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id else { return; };

        let Some(data) = self.panel_app.panel_grid.active_container()
            .map(|w| w.current_tag_id)
            .and_then(|bid| self.core.drawings().get_data(bid, prim_id)) else { return; };

        if let Some(tf_idx) = field_id.strip_prefix("tf_")
            .and_then(|s| s.strip_suffix("_slider"))
            .and_then(|s| s.parse::<usize>().ok())
        {
            let mut new_data = data.clone();
            let mut tf_config = new_data.timeframe_visibility.clone()
                .unwrap_or_else(TimeframeVisibilityConfig::all);

            let (current_min, current_max) = match tf_idx {
                1 => tf_config.seconds.unwrap_or((1, 59)),
                2 => tf_config.minutes.unwrap_or((1, 59)),
                3 => tf_config.hours.unwrap_or((1, 24)),
                4 => tf_config.days.unwrap_or((1, 366)),
                5 => tf_config.weeks.unwrap_or((1, 52)),
                6 => tf_config.months.unwrap_or((1, 12)),
                _ => return,
            };

            let new_range = match handle {
                DualSliderHandle::Min => (value.min(current_max), current_max),
                DualSliderHandle::Max => (current_min, value.max(current_min)),
            };

            match tf_idx {
                1 => tf_config.seconds  = Some(new_range),
                2 => tf_config.minutes  = Some(new_range),
                3 => tf_config.hours    = Some(new_range),
                4 => tf_config.days     = Some(new_range),
                5 => tf_config.weeks    = Some(new_range),
                6 => tf_config.months   = Some(new_range),
                _ => {}
            }

            new_data.timeframe_visibility = Some(tf_config);
            let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            if let Some(bid) = active_bid {
                self.core.drawings_mut().set_data(bid, prim_id, &new_data);
            }
            self.sync_drawing_back_to_group();

            // Keep any active text-editing field in sync with the new range value.
            let edit_field = match handle {
                DualSliderHandle::Min => format!("tf_{}_min", tf_idx),
                DualSliderHandle::Max => format!("tf_{}_max", tf_idx),
            };
            let value_str = match handle {
                DualSliderHandle::Min => new_range.0.to_string(),
                DualSliderHandle::Max => new_range.1.to_string(),
            };
            if self.panel_app.primitive_settings_state.active_field_id.as_deref() == Some(edit_field.as_str()) {
                let prim_wid = uzor::WidgetId::from(crate::text_input::PRIMITIVE_SETTINGS_FIELD);
                let len = value_str.len();
                let mut coord = self.input_coordinator.borrow_mut();
                coord.text_fields_mut().set_text(&prim_wid, &value_str);
                if let Some(fs) = coord.text_fields_mut().field_state_mut(&prim_wid) {
                    fs.cursor = len;
                }
            }
        }
        self.autosave_snapshot();
    }

    // =========================================================================
    // DATA & CACHE slider commit helper
    // =========================================================================

    /// Commit a DATA & CACHE slider value to the user profile.
    ///
    /// `field_id` matches the IDs used in the Performance tab renderer:
    /// `"data_bg_bars"`, `"data_max_bars"`.
    pub(crate) fn apply_data_cache_slider_value(&mut self, field_id: &str, value: f64) {
        let mut user_mut = self.core.user_mut();
        let dl = &mut user_mut.profile_mut().domain.data_load;
        match field_id {
            "data_bg_bars" => {
                dl.background_bar_count = (value.round() as u32).clamp(300, 10000);
                let v = dl.background_bar_count;
                self.panel_app.user_settings_state.data_bg_bars = v;
                log::debug!("[ChartApp] data_load.background_bar_count = {}", v);
            }
            "data_max_bars" => {
                dl.max_loaded_bars = (value.round() as u32).min(50000);
                let v = dl.max_loaded_bars;
                self.panel_app.user_settings_state.data_max_bars = v;
                log::debug!("[ChartApp] data_load.max_loaded_bars = {}", v);
            }
            _ => log::debug!("[ChartApp] apply_data_cache_slider_value: unknown field: {}", field_id),
        }
        drop(user_mut);
        self.autosave_snapshot();
    }

    // =========================================================================
    // Indicator tf dual-slider helper
    // =========================================================================

    /// Apply a dual-handle (min/max range) slider value for `tf_*_slider` fields in
    /// the **indicator** settings modal. Writes directly to the `IndicatorInstance`'s
    /// `timeframe_visibility` field.
    pub(crate) fn apply_ind_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
        let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id else { return; };

        let Some(tf_idx) = field_id.strip_prefix("tf_")
            .and_then(|s| s.strip_suffix("_slider"))
            .and_then(|s| s.parse::<usize>().ok()) else { return; };

        let mut new_range = (0u32, 0u32);
        let mut out_of_range = false;
        let applied = self.core.indicators_mut().update_timeframe_visibility(ind_id, |tf_config| {
            let (current_min, current_max) = match tf_idx {
                1 => tf_config.seconds.unwrap_or((1, 59)),
                2 => tf_config.minutes.unwrap_or((1, 59)),
                3 => tf_config.hours.unwrap_or((1, 24)),
                4 => tf_config.days.unwrap_or((1, 366)),
                5 => tf_config.weeks.unwrap_or((1, 52)),
                6 => tf_config.months.unwrap_or((1, 12)),
                _ => { out_of_range = true; return; }
            };

            new_range = match handle {
                DualSliderHandle::Min => (value.min(current_max), current_max),
                DualSliderHandle::Max => (current_min, value.max(current_min)),
            };

            match tf_idx {
                1 => tf_config.seconds = Some(new_range),
                2 => tf_config.minutes = Some(new_range),
                3 => tf_config.hours   = Some(new_range),
                4 => tf_config.days    = Some(new_range),
                5 => tf_config.weeks   = Some(new_range),
                6 => tf_config.months  = Some(new_range),
                _ => {}
            }
        });
        if !applied || out_of_range { return; }

        // Keep any active text-editing field in sync with the new range value.
        let edit_field = match handle {
            DualSliderHandle::Min => format!("tf_{}_min", tf_idx),
            DualSliderHandle::Max => format!("tf_{}_max", tf_idx),
        };
        let value_str = match handle {
            DualSliderHandle::Min => new_range.0.to_string(),
            DualSliderHandle::Max => new_range.1.to_string(),
        };
        if self.panel_app.indicator_settings_state.active_field_id.as_deref() == Some(edit_field.as_str()) {
            let ind_wid = uzor::WidgetId::from(crate::text_input::INDICATOR_SETTINGS_FIELD);
            let len = value_str.len();
            let mut coord = self.input_coordinator.borrow_mut();
            coord.text_fields_mut().set_text(&ind_wid, &value_str);
            if let Some(fs) = coord.text_fields_mut().field_state_mut(&ind_wid) {
                fs.cursor = len;
            }
        }

        log::debug!("[ChartApp] ind_settings tf_{}_slider {:?}: {}", tf_idx, handle, value);
        self.autosave_snapshot();
    }

    /// Apply a new minimum value for a timeframe range on the current indicator instance.
    pub(crate) fn apply_ind_tf_min_value(&mut self, tf_idx: usize, val: u32) {
        let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id else { return; };
        self.core.indicators_mut().update_timeframe_visibility(ind_id, |tf| {
            match tf_idx {
                1 => { if let Some((_, max)) = tf.seconds { tf.seconds = Some((val.min(max), max)); } }
                2 => { if let Some((_, max)) = tf.minutes { tf.minutes = Some((val.min(max), max)); } }
                3 => { if let Some((_, max)) = tf.hours   { tf.hours   = Some((val.min(max), max)); } }
                4 => { if let Some((_, max)) = tf.days    { tf.days    = Some((val.min(max), max)); } }
                5 => { if let Some((_, max)) = tf.weeks   { tf.weeks   = Some((val.min(max), max)); } }
                6 => { if let Some((_, max)) = tf.months  { tf.months  = Some((val.min(max), max)); } }
                _ => {}
            }
        });
        log::debug!("[ChartApp] ind_settings tf_{}_min committed: {}", tf_idx, val);
        self.autosave_snapshot();
    }

    /// Apply a new maximum value for a timeframe range on the current indicator instance.
    pub(crate) fn apply_ind_tf_max_value(&mut self, tf_idx: usize, val: u32) {
        let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id else { return; };
        self.core.indicators_mut().update_timeframe_visibility(ind_id, |tf| {
            match tf_idx {
                1 => { if let Some((min, _)) = tf.seconds { tf.seconds = Some((min, val.max(min))); } }
                2 => { if let Some((min, _)) = tf.minutes { tf.minutes = Some((min, val.max(min))); } }
                3 => { if let Some((min, _)) = tf.hours   { tf.hours   = Some((min, val.max(min))); } }
                4 => { if let Some((min, _)) = tf.days    { tf.days    = Some((min, val.max(min))); } }
                5 => { if let Some((min, _)) = tf.weeks   { tf.weeks   = Some((min, val.max(min))); } }
                6 => { if let Some((min, _)) = tf.months  { tf.months  = Some((min, val.max(min))); } }
                _ => {}
            }
        });
        log::debug!("[ChartApp] ind_settings tf_{}_max committed: {}", tf_idx, val);
        self.autosave_snapshot();
    }

    // =========================================================================
    // Compare tf dual-slider helper
    // =========================================================================

    /// Apply a dual-handle (min/max range) slider value for `tf_*_slider` fields in
    /// the **compare** settings modal. Writes to the compare series `timeframe_visibility`.
    pub(crate) fn apply_cmp_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
        use mlc_core::drawing::TimeframeVisibilityConfig;

        let Some(tf_idx) = field_id.strip_prefix("tf_")
            .and_then(|s| s.strip_suffix("_slider"))
            .and_then(|s| s.parse::<usize>().ok()) else { return; };

        let series_idx = self.panel_app.compare_settings_state.series_index;

        let mut tf_config = self.panel_app.compare_settings_state
            .cached_timeframe_visibility.clone()
            .unwrap_or_else(TimeframeVisibilityConfig::all);

        let (current_min, current_max) = match tf_idx {
            1 => tf_config.seconds.unwrap_or((1, 59)),
            2 => tf_config.minutes.unwrap_or((1, 59)),
            3 => tf_config.hours.unwrap_or((1, 24)),
            4 => tf_config.days.unwrap_or((1, 366)),
            5 => tf_config.weeks.unwrap_or((1, 52)),
            6 => tf_config.months.unwrap_or((1, 12)),
            _ => return,
        };

        let new_range = match handle {
            DualSliderHandle::Min => (value.min(current_max), current_max),
            DualSliderHandle::Max => (current_min, value.max(current_min)),
        };

        match tf_idx {
            1 => tf_config.seconds = Some(new_range),
            2 => tf_config.minutes = Some(new_range),
            3 => tf_config.hours   = Some(new_range),
            4 => tf_config.days    = Some(new_range),
            5 => tf_config.weeks   = Some(new_range),
            6 => tf_config.months  = Some(new_range),
            _ => {}
        }

        self.panel_app.compare_settings_state.cached_timeframe_visibility = Some(tf_config.clone());

        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.compare_overlay.set_series_timeframe_visibility(series_idx, tf_config);
        }

        // Keep any active text-editing field in sync with the new range value.
        let edit_field = match handle {
            DualSliderHandle::Min => format!("tf_{}_min", tf_idx),
            DualSliderHandle::Max => format!("tf_{}_max", tf_idx),
        };
        let value_str = match handle {
            DualSliderHandle::Min => new_range.0.to_string(),
            DualSliderHandle::Max => new_range.1.to_string(),
        };
        if self.panel_app.compare_settings_state.active_field_id.as_deref() == Some(edit_field.as_str()) {
            let cmp_wid = uzor::WidgetId::from(crate::text_input::COMPARE_SETTINGS_FIELD);
            let len = value_str.len();
            let mut coord = self.input_coordinator.borrow_mut();
            coord.text_fields_mut().set_text(&cmp_wid, &value_str);
            if let Some(fs) = coord.text_fields_mut().field_state_mut(&cmp_wid) {
                fs.cursor = len;
            }
        }

        log::debug!("[ChartApp] cmp_settings tf_{}_slider {:?}: {}", tf_idx, handle, value);
        self.autosave_snapshot();
    }

}

impl ChartApp {
    /// Set the profile's `scroll_speed` or `zoom_speed` (`which`), clamped to
    /// the slider's range, and mark the profile for saving.
    pub(crate) fn set_input_speed(&mut self, which: &str, value: f64) {
        use mlc_app_layout::layout::modals::chart_settings::{INPUT_SPEED_MAX, INPUT_SPEED_MIN};
        let value = value.clamp(INPUT_SPEED_MIN, INPUT_SPEED_MAX);
        {
            let mut user = self.core.user_mut();
            let ui = &mut user.profile_mut().ui;
            match which {
                "scroll_speed" => ui.scroll_speed = value,
                "zoom_speed" => ui.zoom_speed = value,
                other => {
                    log::debug!("[ChartApp] set_input_speed: unknown setting: {}", other);
                    return;
                }
            }
        }
        self.profile_dirty = true;
    }

    /// Mirror the profile's wheel / trackpad settings into the chart settings
    /// modal's state, which is what its Scales tab draws.
    pub(crate) fn sync_input_prefs_to_settings_ui(&mut self) {
        let (scroll_speed, zoom_speed, precise) = {
            let user = self.core.user();
            let ui = &user.profile().ui;
            (ui.scroll_speed, ui.zoom_speed, ui.precise_trackpad)
        };
        let state = &mut self.panel_app.chart_settings_state;
        state.input_scroll_speed = scroll_speed;
        state.input_zoom_speed = zoom_speed;
        state.input_precise_trackpad = precise;
    }

    /// Flip the profile's precise-trackpad switch.
    pub(crate) fn toggle_precise_trackpad(&mut self) {
        {
            let mut user = self.core.user_mut();
            let ui = &mut user.profile_mut().ui;
            ui.precise_trackpad = !ui.precise_trackpad;
        }
        self.profile_dirty = true;
    }

    /// Set the alpha of a candle style row's up and down colors (`body`,
    /// `border` or `wick`) to `alpha`, keeping each color's RGB exactly.
    pub(crate) fn apply_candle_opacity(&mut self, row: &str, alpha: f64) {
        let series = &mut self.panel_app.theme_manager.current_mut().series;
        match row {
            "body" => {
                series.candle_up_body = mlc_core::with_alpha(&series.candle_up_body, alpha);
                series.candle_down_body = mlc_core::with_alpha(&series.candle_down_body, alpha);
            }
            "wick" => {
                series.candle_up_wick = mlc_core::with_alpha(&series.candle_up_wick, alpha);
                series.candle_down_wick = mlc_core::with_alpha(&series.candle_down_wick, alpha);
            }
            "border" => {
                // Borders are optional: an unset one follows its body color,
                // so the row starts from that color.
                let up = series.candle_up_border.clone().unwrap_or_else(|| series.candle_up_body.clone());
                let down = series.candle_down_border.clone().unwrap_or_else(|| series.candle_down_body.clone());
                series.candle_up_border = Some(mlc_core::with_alpha(&up, alpha));
                series.candle_down_border = Some(mlc_core::with_alpha(&down, alpha));
            }
            other => {
                log::debug!("[ChartApp] apply_candle_opacity: unknown row: {}", other);
                return;
            }
        }
        if row == "border" {
            let on = alpha > 0.001;
            let state = &mut self.panel_app.chart_settings_state;
            state.instrument_border_enabled = on;
            if on {
                state.remembered_border_alpha_up = Some(alpha);
                state.remembered_border_alpha_down = Some(alpha);
            }
        }
        self.drain_candle_appearance();
    }

    /// Copy the live candle colors into the user profile and mark it dirty.
    /// Same writer as [`Self::set_input_speed`]: the shell saves `profile.json`.
    pub(crate) fn drain_candle_appearance(&mut self) {
        let series = self.panel_app.theme_manager.current().series.clone();
        {
            let mut user = self.core.user_mut();
            let ui = &mut user.profile_mut().ui;
            ui.candle_up_body = Some(series.candle_up_body);
            ui.candle_down_body = Some(series.candle_down_body);
            ui.candle_up_wick = Some(series.candle_up_wick);
            ui.candle_down_wick = Some(series.candle_down_wick);
            ui.candle_up_border = series.candle_up_border;
            ui.candle_down_border = series.candle_down_border;
        }
        self.profile_dirty = true;
    }

    /// Write the shared custom palette into the profile and every picker.
    pub(crate) fn publish_custom_palette(&mut self, colors: Vec<String>) {
        {
            let mut user = self.core.user_mut();
            user.profile_mut().ui.custom_colors = colors.clone();
        }
        self.sync_custom_palette(&colors);
        self.profile_dirty = true;
    }

    fn sync_custom_palette(&mut self, colors: &[String]) {
        let owned = colors.to_vec();
        self.panel_app.chart_settings_state.color_picker.custom_colors = owned.clone();
        self.panel_app.primitive_settings_state.color_picker.custom_colors = owned.clone();
        self.panel_app.indicator_settings_state.color_picker.custom_colors = owned.clone();
        self.panel_app.compare_settings_state.color_picker.custom_colors = owned.clone();
        self.panel_app.panel_color_picker.custom_colors = owned;
    }

    /// Apply the factory theme named `name`, then the profile's candle
    /// colors on top. An empty name leaves the current theme in place.
    /// Missing profile fields leave the factory color.
    pub fn set_chart_theme(&mut self, name: &str) {
        if !name.is_empty() {
            self.panel_app.theme_manager.set_preset(name);
        }
        self.apply_saved_candle_appearance();
    }

    /// Overlay profile candle colors and the custom palette onto the live
    /// theme. Does not mark the profile dirty.
    pub fn apply_saved_candle_appearance(&mut self) {
        let (up_body, down_body, up_wick, down_wick, up_border, down_border, colors) = {
            let user = self.core.user();
            let ui = &user.profile().ui;
            (
                ui.candle_up_body.clone(),
                ui.candle_down_body.clone(),
                ui.candle_up_wick.clone(),
                ui.candle_down_wick.clone(),
                ui.candle_up_border.clone(),
                ui.candle_down_border.clone(),
                ui.custom_colors.clone(),
            )
        };
        {
            let series = &mut self.panel_app.theme_manager.current_mut().series;
            if let Some(color) = up_body {
                series.candle_up_body = color;
            }
            if let Some(color) = down_body {
                series.candle_down_body = color;
            }
            if let Some(color) = up_wick {
                series.candle_up_wick = color;
            }
            if let Some(color) = down_wick {
                series.candle_down_wick = color;
            }
            if let Some(color) = up_border {
                series.candle_up_border = Some(color);
            }
            if let Some(color) = down_border {
                series.candle_down_border = Some(color);
            }
        }
        let both_off = {
            let series = &self.panel_app.theme_manager.current().series;
            match (&series.candle_up_border, &series.candle_down_border) {
                (Some(up), Some(down)) => {
                    mlc_core::color_alpha(up) <= 0.001 && mlc_core::color_alpha(down) <= 0.001
                }
                _ => false,
            }
        };
        if both_off {
            self.panel_app.chart_settings_state.instrument_border_enabled = false;
        }
        self.sync_custom_palette(&colors);
    }

    /// Uncheck stores each side's non-zero alpha and writes alpha 0 (RGB kept).
    /// Recheck restores that alpha. Body alpha is only the fallback when
    /// nothing non-zero was remembered.
    pub(crate) fn toggle_instrument_border(&mut self) {
        let enabling = !self.panel_app.chart_settings_state.instrument_border_enabled;
        let remembered_up = self.panel_app.chart_settings_state.remembered_border_alpha_up;
        let remembered_down = self.panel_app.chart_settings_state.remembered_border_alpha_down;
        let (up_color, down_color, remember_up, remember_down) = {
            let series = &self.panel_app.theme_manager.current().series;
            let up_src = series
                .candle_up_border
                .clone()
                .unwrap_or_else(|| series.candle_up_body.clone());
            let down_src = series
                .candle_down_border
                .clone()
                .unwrap_or_else(|| series.candle_down_body.clone());
            if enabling {
                let up_alpha = remembered_up
                    .filter(|alpha| *alpha > 0.001)
                    .unwrap_or_else(|| mlc_core::color_alpha(&series.candle_up_body));
                let down_alpha = remembered_down
                    .filter(|alpha| *alpha > 0.001)
                    .unwrap_or_else(|| mlc_core::color_alpha(&series.candle_down_body));
                (
                    mlc_core::with_alpha(&up_src, up_alpha),
                    mlc_core::with_alpha(&down_src, down_alpha),
                    Some(up_alpha),
                    Some(down_alpha),
                )
            } else {
                let up_alpha = mlc_core::color_alpha(&up_src);
                let down_alpha = mlc_core::color_alpha(&down_src);
                (
                    mlc_core::with_alpha(&up_src, 0.0),
                    mlc_core::with_alpha(&down_src, 0.0),
                    if up_alpha > 0.001 { Some(up_alpha) } else { remembered_up },
                    if down_alpha > 0.001 { Some(down_alpha) } else { remembered_down },
                )
            }
        };
        {
            let series = &mut self.panel_app.theme_manager.current_mut().series;
            series.candle_up_border = Some(up_color);
            series.candle_down_border = Some(down_color);
        }
        let state = &mut self.panel_app.chart_settings_state;
        state.instrument_border_enabled = enabling;
        state.remembered_border_alpha_up = remember_up;
        state.remembered_border_alpha_down = remember_down;
        self.drain_candle_appearance();
    }

    /// Checkbox follows the border colors just written. A non-zero alpha is
    /// remembered for that side only.
    pub(crate) fn sync_border_checkbox_from_series(&mut self) {
        let (up_alpha, down_alpha) = {
            let series = &self.panel_app.theme_manager.current().series;
            let up_alpha = series.candle_up_border.as_deref().map(mlc_core::color_alpha).unwrap_or(1.0);
            let down_alpha = series.candle_down_border.as_deref().map(mlc_core::color_alpha).unwrap_or(1.0);
            (up_alpha, down_alpha)
        };
        let state = &mut self.panel_app.chart_settings_state;
        state.instrument_border_enabled = up_alpha > 0.001 || down_alpha > 0.001;
        if up_alpha > 0.001 {
            state.remembered_border_alpha_up = Some(up_alpha);
        }
        if down_alpha > 0.001 {
            state.remembered_border_alpha_down = Some(down_alpha);
        }
    }

    /// One precision for the settings dropdown and the price-scale menu.
    /// Writes `scale_settings` and the scale the formatter reads.
    pub fn set_user_price_precision(&mut self, precision: Option<usize>) {
        let precision = match precision {
            Some(n) if n <= 8 => Some(n),
            Some(_) => return,
            None => None,
        };
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.scale_settings.user_precision = precision;
            window.set_price_precision(precision);
        }
    }
}

#[cfg(test)]
mod candle_opacity_tests {
    use std::sync::Arc;

    use mlc_backend_data_bridge::DataBridge;

    use crate::ChartApp;

    async fn app() -> ChartApp {
        let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
        let core = mlc_backend::BackendCore::new(Arc::new(bridge), None);
        ChartApp::new_empty(core)
    }

    /// Tester report: changing the candle opacity reset the color to green.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_opacity_slider_changes_only_alpha_and_keeps_the_users_grey() {
        let mut app = app().await;
        {
            let series = &mut app.panel_app.theme_manager.current_mut().series;
            series.candle_up_body = "#808080".to_string();
            series.candle_down_body = "#404040".to_string();
            series.candle_up_wick = "#909090".to_string();
        }
        app.apply_slider_value("candle_opacity:body", 0.4);
        let series = &app.panel_app.theme_manager.current().series;
        assert_eq!(series.candle_up_body, "rgba(128,128,128,0.4)", "up stays grey, only alpha changes");
        assert_eq!(series.candle_down_body, "rgba(64,64,64,0.4)", "down keeps its own rgb");
        assert_eq!(series.candle_up_wick, "#909090", "other rows are untouched");

        app.apply_slider_value("candle_opacity:body", 0.7);
        assert_eq!(app.panel_app.theme_manager.current().series.candle_up_body, "rgba(128,128,128,0.7)");
        app.apply_slider_value("candle_opacity:body", 1.0);
        assert_eq!(app.panel_app.theme_manager.current().series.candle_up_body, "#808080");
        assert!(app.profile_dirty, "a candle edit drains into the profile");
        assert_eq!(
            app.core.user().profile().ui.candle_up_body.as_deref(),
            Some("#808080"),
            "the profile keeps the user's grey"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_speed_sliders_write_the_profile_within_their_range() {
        let mut app = app().await;
        assert_eq!(app.core.user().profile().ui.scroll_speed, 1.0, "default");
        app.apply_slider_value("input:scroll_speed", 2.0);
        app.apply_slider_value("input:zoom_speed", 0.5);
        assert_eq!(app.core.user().profile().ui.scroll_speed, 2.0);
        assert_eq!(app.core.user().profile().ui.zoom_speed, 0.5);
        app.apply_slider_value("input:zoom_speed", 99.0);
        assert_eq!(app.core.user().profile().ui.zoom_speed, 3.0, "clamped to 3x");
        app.apply_slider_value("input:zoom_speed", 0.0);
        assert_eq!(app.core.user().profile().ui.zoom_speed, 0.25, "clamped to 0.25x");

        app.sync_input_prefs_to_settings_ui();
        assert_eq!(app.panel_app.chart_settings_state.input_scroll_speed, 2.0, "the modal shows the profile");
        assert!(app.panel_app.chart_settings_state.input_precise_trackpad);
        app.toggle_precise_trackpad();
        app.sync_input_prefs_to_settings_ui();
        assert!(!app.panel_app.chart_settings_state.input_precise_trackpad);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scroll_actions_carry_the_kind_and_the_users_speeds() {
        use crate::ScrollKind;
        use mlc_core::ChartInputAction;
        let mut app = app().await;
        app.apply_slider_value("input:scroll_speed", 2.0);
        app.apply_slider_value("input:zoom_speed", 1.5);

        let fields = |a: ChartInputAction| match a {
            ChartInputAction::ScrollScaled { continuous, zoom_gain, pan_gain, .. } => (continuous, zoom_gain, pan_gain),
            other => panic!("unexpected action {other:?}"),
        };
        assert_eq!(fields(app.chart_scroll_action(0.0, 0.0, 1.0, 2.0, ScrollKind::Discrete)), (false, 1.5, 2.0));
        assert_eq!(
            fields(app.chart_scroll_action(0.0, 0.0, 1.0, 2.0, ScrollKind::Continuous { pinch: false })),
            (true, 1.5, 2.0),
            "a trackpad scroll is continuous"
        );
        let (continuous, pinch_gain, _) =
            fields(app.chart_scroll_action(0.0, 0.0, 0.0, 2.0, ScrollKind::Continuous { pinch: true }));
        assert!(continuous && pinch_gain > 1.5, "a pinch carries extra gain: {pinch_gain}");

        // With Precise trackpad off every event is one fixed step again.
        app.toggle_precise_trackpad();
        assert_eq!(
            fields(app.chart_scroll_action(0.0, 0.0, 1.0, 2.0, ScrollKind::Continuous { pinch: false })),
            (false, 1.5, 2.0)
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wick_and_border_rows_get_the_same_treatment() {
        let mut app = app().await;
        {
            let series = &mut app.panel_app.theme_manager.current_mut().series;
            series.candle_up_wick = "#123456".to_string();
            series.candle_down_wick = "#654321".to_string();
            series.candle_up_border = Some("#abcdef".to_string());
            series.candle_down_border = None;
            series.candle_down_body = "#404040".to_string();
        }
        app.apply_slider_value("candle_opacity:wick", 0.5);
        app.apply_slider_value("candle_opacity:border", 0.25);
        let series = &app.panel_app.theme_manager.current().series;
        assert_eq!(series.candle_up_wick, "rgba(18,52,86,0.5)");
        assert_eq!(series.candle_down_wick, "rgba(101,67,33,0.5)");
        assert_eq!(series.candle_up_border.as_deref(), Some("rgba(171,205,239,0.25)"));
        assert_eq!(series.candle_down_border.as_deref(), Some("rgba(64,64,64,0.25)"), "an unset border follows its body");
        {
            let user = app.core.user();
            let ui = &user.profile().ui;
            assert_eq!(ui.candle_up_border.as_deref(), Some("rgba(171,205,239,0.25)"));
            assert_eq!(ui.candle_down_border.as_deref(), Some("rgba(64,64,64,0.25)"));
        }
        assert!(app.panel_app.chart_settings_state.instrument_border_enabled);
        let snapshot = app.build_chart_settings_data();
        assert_eq!(snapshot.instrument.border_up_color, "rgba(171,205,239,0.25)");
        assert_ne!(snapshot.instrument.border_up_color, snapshot.instrument.body_up_color);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unchecking_the_border_zeros_alpha_and_recheck_restores_it() {
        let mut app = app().await;
        {
            let series = &mut app.panel_app.theme_manager.current_mut().series;
            series.candle_up_body = "#112233".to_string();
            series.candle_down_body = "#445566".to_string();
            series.candle_up_border = Some("#abcdef".to_string());
            series.candle_down_border = Some("rgba(10,20,30,0.4)".to_string());
        }
        app.toggle_instrument_border();
        let series = &app.panel_app.theme_manager.current().series;
        assert_eq!(series.candle_up_border.as_deref(), Some("rgba(171,205,239,0)"));
        assert_eq!(series.candle_down_border.as_deref(), Some("rgba(10,20,30,0)"));
        assert_eq!(series.candle_up_body, "#112233", "body rgb stays put");
        assert!(!app.panel_app.chart_settings_state.instrument_border_enabled);
        assert_eq!(
            app.core.user().profile().ui.candle_up_border.as_deref(),
            Some("rgba(171,205,239,0)")
        );

        // Body alpha changes must not become the restore value.
        app.panel_app.theme_manager.current_mut().series.candle_up_body = "rgba(17,34,51,0.2)".to_string();
        app.toggle_instrument_border();
        let series = &app.panel_app.theme_manager.current().series;
        assert_eq!(series.candle_up_border.as_deref(), Some("#abcdef"));
        assert_eq!(series.candle_down_border.as_deref(), Some("rgba(10,20,30,0.4)"));
        assert!(app.panel_app.chart_settings_state.instrument_border_enabled);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn recheck_without_a_remembered_alpha_uses_the_body_alpha_once() {
        let mut app = app().await;
        {
            let series = &mut app.panel_app.theme_manager.current_mut().series;
            series.candle_up_body = "rgba(9,9,9,0.3)".to_string();
            series.candle_down_body = "rgba(8,8,8,0.6)".to_string();
            series.candle_up_border = Some("rgba(1,2,3,0)".to_string());
            series.candle_down_border = Some("rgba(4,5,6,0)".to_string());
        }
        app.toggle_instrument_border();
        assert!(app.panel_app.chart_settings_state.remembered_border_alpha_up.is_none());
        app.toggle_instrument_border();
        assert_eq!(
            app.panel_app.theme_manager.current().series.candle_up_border.as_deref(),
            Some("rgba(1,2,3,0.3)"),
            "with nothing remembered, the body alpha is the restore value"
        );
        app.panel_app.theme_manager.current_mut().series.candle_up_body = "#ffffff".to_string();
        assert_eq!(
            app.panel_app.theme_manager.current().series.candle_up_border.as_deref(),
            Some("rgba(1,2,3,0.3)"),
            "body alpha is not a live binding after the restore"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn saved_candle_colors_override_the_factory_theme() {
        let mut app = app().await;
        {
            let mut user = app.core.user_mut();
            let ui = &mut user.profile_mut().ui;
            ui.candle_up_body = Some("#010203".to_string());
            ui.candle_up_border = Some("rgba(1,2,3,0)".to_string());
            ui.candle_down_border = Some("rgba(4,5,6,0)".to_string());
            ui.custom_colors = vec!["#aabbcc".to_string()];
        }
        app.profile_dirty = false;
        app.set_chart_theme("light");
        assert_eq!(app.panel_app.theme_manager.current().series.candle_up_body, "#010203");
        assert!(!app.panel_app.chart_settings_state.instrument_border_enabled);
        assert_eq!(
            app.panel_app.chart_settings_state.color_picker.custom_colors,
            vec!["#aabbcc".to_string()]
        );
        assert!(!app.profile_dirty, "loading appearance does not rewrite the profile");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn price_precision_setter_writes_the_scale_and_the_settings() {
        let mut app = app().await;
        app.set_user_price_precision(Some(4));
        let window = app.panel_app.panel_grid.active_container().expect("a chart window");
        assert_eq!(window.scale_settings.user_precision, Some(4));
        assert_eq!(window.price_scale().user_precision, Some(4));
        app.set_user_price_precision(None);
        let window = app.panel_app.panel_grid.active_container().expect("a chart window");
        assert_eq!(window.scale_settings.user_precision, None);
        assert_eq!(window.price_scale().user_precision, None);
        app.set_user_price_precision(Some(9));
        let window = app.panel_app.panel_grid.active_container().expect("a chart window");
        assert_eq!(window.price_scale().user_precision, None, "above 8 is not a precision");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_open_dropdown_swallows_the_click_under_it() {
        let mut app = app().await;
        app.panel_app.chart_settings_state.is_open = true;
        app.panel_app.chart_settings_state.active_dropdown = Some("precision".to_string());
        assert!(!app.exclusive_overlay_swallows_click(Some("chart_settings:item:dropdown_option:precision:4")));
        assert_eq!(
            app.panel_app.chart_settings_state.active_dropdown.as_deref(),
            Some("precision")
        );
        assert!(app.exclusive_overlay_swallows_click(Some("chart_settings:footer:ok")));
        assert!(app.panel_app.chart_settings_state.active_dropdown.is_none());
        assert!(app.panel_app.chart_settings_state.is_open, "the parent modal stays");

        app.panel_app.chart_settings_state.color_picker.open_l1(0.0, 0.0, Some("#808080"));
        assert!(app.exclusive_overlay_swallows_click(Some("chart_settings:footer:ok")));
        assert!(!app.panel_app.chart_settings_state.color_picker.is_open());
        assert!(app.panel_app.chart_settings_state.is_open);
    }
}
