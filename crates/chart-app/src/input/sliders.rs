//! Slider value appliers — translate slider drag/scroll deltas into state mutations.

use crate::ChartApp;
use zengeld_chart::ui::modal_settings::DualSliderHandle;
use zengeld_chart::drawing::TimeframeVisibilityConfig;

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
                _ => eprintln!("[ChartApp] apply_slider_value: unknown style param: {}", param_id),
            }
            self.autosave_snapshot();
            return;
        }

        // Scales tab sliders
        if field_id.starts_with("scales:") {
            let Some(window) = self.panel_app.panel_grid.active_window_mut() else { return; };
            match field_id {
                "scales:price_width_slider" => {
                    window.scale_settings.price_scale_width = value;
                }
                "scales:time_height_slider" => {
                    window.scale_settings.time_scale_height = value;
                }
                "scales:crosshair_line_width" => {
                    window.crosshair_options.vert_line.width = value;
                    window.crosshair_options.horz_line.width = value;
                }
                _ => eprintln!("[ChartApp] apply_slider_value: unknown scales field: {}", field_id),
            }
            self.autosave_snapshot();
            return;
        }

        // Primitive settings sliders
        let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx else { return; };

        if field_id == "stroke_width" {
            let data_opt = self.panel_app.panel_grid.active_window()
                .and_then(|w| w.drawing_manager.get_data_at(idx))
                .map(|mut d| { d.width = value; d });
            if let Some(data) = data_opt {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.set_data_at(idx, &data);
                }
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            self.snapshot_primitive_settings_to_user_manager(idx);
            return;
        }

        if let Some(prop_id) = field_id.strip_prefix("style_prop:") {
            use zengeld_chart::drawing::primitives_v2::config::PropertyValue;
            let new_val = PropertyValue::Number(value);
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.apply_style_property(idx, prop_id, new_val);
            }
            self.sync_drawing_back_to_group();
            self.autosave_snapshot();
            return;
        }

        if let Some(prop_id) = field_id.strip_prefix("text_prop:") {
            use zengeld_chart::drawing::primitives_v2::config::PropertyValue;
            let new_val = PropertyValue::Number(value);
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.apply_text_property(idx, prop_id, new_val);
            }
            self.autosave_snapshot();
            return;
        }

        eprintln!("[ChartApp] apply_slider_value: unhandled field_id: {}", field_id);
    }

    /// Apply a dual-handle (min/max range) slider value for `tf_*_slider` fields.
    pub(crate) fn apply_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
        let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx else { return; };

        let Some(data) = self.panel_app.panel_grid.active_window()
            .and_then(|w| w.drawing_manager.get_data_at(idx)) else { return; };

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
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.set_data_at(idx, &new_data);
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
            if let Some(ref mut edit) = self.panel_app.primitive_settings_state.editing_text {
                if edit.field_id == edit_field {
                    edit.text = value_str;
                    edit.cursor = edit.text.len();
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
    /// `"data_bg_bars"`, `"data_max_bars"`, `"data_store_size_mb"`, `"data_cleanup_days"`.
    pub(crate) fn apply_data_cache_slider_value(&mut self, field_id: &str, value: f64) {
        let dl = &mut self.panel_app.user_manager.profile.data_load;
        match field_id {
            "data_bg_bars" => {
                dl.background_bar_count = (value.round() as u32).clamp(300, 10000);
                let v = dl.background_bar_count;
                self.panel_app.user_settings_state.data_bg_bars = v;
                eprintln!("[ChartApp] data_load.background_bar_count = {}", v);
            }
            "data_max_bars" => {
                dl.max_loaded_bars = (value.round() as u32).min(50000);
                let v = dl.max_loaded_bars;
                self.panel_app.user_settings_state.data_max_bars = v;
                eprintln!("[ChartApp] data_load.max_loaded_bars = {}", v);
            }
            "data_store_size_mb" => {
                dl.max_store_size_mb = (value.round() as u32).clamp(50, 5000);
                let v = dl.max_store_size_mb;
                self.panel_app.user_settings_state.data_store_size_mb = v;
                eprintln!("[ChartApp] data_load.max_store_size_mb = {}", v);
            }
            "data_cleanup_days" => {
                dl.store_cleanup_days = (value.round() as u32).clamp(1, 365);
                let v = dl.store_cleanup_days;
                self.panel_app.user_settings_state.data_cleanup_days = v;
                eprintln!("[ChartApp] data_load.store_cleanup_days = {}", v);
            }
            _ => eprintln!("[ChartApp] apply_data_cache_slider_value: unknown field: {}", field_id),
        }
        self.autosave_snapshot();
    }

    // =========================================================================
    // Indicator tf dual-slider helper
    // =========================================================================

    /// Apply a dual-handle (min/max range) slider value for `tf_*_slider` fields in
    /// the **indicator** settings modal. Writes directly to the `IndicatorInstance`'s
    /// `timeframe_visibility` field.
    pub(crate) fn apply_ind_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
        use zengeld_chart::drawing::TimeframeVisibilityConfig;

        let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id else { return; };

        let Some(tf_idx) = field_id.strip_prefix("tf_")
            .and_then(|s| s.strip_suffix("_slider"))
            .and_then(|s| s.parse::<usize>().ok()) else { return; };

        let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) else { return; };

        let mut tf_config = inst.timeframe_visibility.clone()
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

        inst.timeframe_visibility = Some(tf_config);

        // Keep any active text-editing field in sync with the new range value.
        let edit_field = match handle {
            DualSliderHandle::Min => format!("tf_{}_min", tf_idx),
            DualSliderHandle::Max => format!("tf_{}_max", tf_idx),
        };
        let value_str = match handle {
            DualSliderHandle::Min => new_range.0.to_string(),
            DualSliderHandle::Max => new_range.1.to_string(),
        };
        if let Some(ref mut edit) = self.panel_app.indicator_settings_state.editing_text_state {
            if edit.field_id == edit_field {
                edit.text = value_str;
                edit.cursor = edit.text.len();
            }
        }

        eprintln!("[ChartApp] ind_settings tf_{}_slider {:?}: {}", tf_idx, handle, value);
        self.autosave_snapshot();
    }

    /// Apply a new minimum value for a timeframe range on the current indicator instance.
    pub(crate) fn apply_ind_tf_min_value(&mut self, tf_idx: usize, val: u32) {
        use zengeld_chart::drawing::TimeframeVisibilityConfig;
        let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id else { return; };
        let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) else { return; };
        let mut tf = inst.timeframe_visibility.clone()
            .unwrap_or_else(TimeframeVisibilityConfig::all);
        match tf_idx {
            1 => { if let Some((_, max)) = tf.seconds { tf.seconds = Some((val.min(max), max)); } }
            2 => { if let Some((_, max)) = tf.minutes { tf.minutes = Some((val.min(max), max)); } }
            3 => { if let Some((_, max)) = tf.hours   { tf.hours   = Some((val.min(max), max)); } }
            4 => { if let Some((_, max)) = tf.days    { tf.days    = Some((val.min(max), max)); } }
            5 => { if let Some((_, max)) = tf.weeks   { tf.weeks   = Some((val.min(max), max)); } }
            6 => { if let Some((_, max)) = tf.months  { tf.months  = Some((val.min(max), max)); } }
            _ => {}
        }
        inst.timeframe_visibility = Some(tf);
        eprintln!("[ChartApp] ind_settings tf_{}_min committed: {}", tf_idx, val);
        self.autosave_snapshot();
    }

    /// Apply a new maximum value for a timeframe range on the current indicator instance.
    pub(crate) fn apply_ind_tf_max_value(&mut self, tf_idx: usize, val: u32) {
        use zengeld_chart::drawing::TimeframeVisibilityConfig;
        let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id else { return; };
        let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) else { return; };
        let mut tf = inst.timeframe_visibility.clone()
            .unwrap_or_else(TimeframeVisibilityConfig::all);
        match tf_idx {
            1 => { if let Some((min, _)) = tf.seconds { tf.seconds = Some((min, val.max(min))); } }
            2 => { if let Some((min, _)) = tf.minutes { tf.minutes = Some((min, val.max(min))); } }
            3 => { if let Some((min, _)) = tf.hours   { tf.hours   = Some((min, val.max(min))); } }
            4 => { if let Some((min, _)) = tf.days    { tf.days    = Some((min, val.max(min))); } }
            5 => { if let Some((min, _)) = tf.weeks   { tf.weeks   = Some((min, val.max(min))); } }
            6 => { if let Some((min, _)) = tf.months  { tf.months  = Some((min, val.max(min))); } }
            _ => {}
        }
        inst.timeframe_visibility = Some(tf);
        eprintln!("[ChartApp] ind_settings tf_{}_max committed: {}", tf_idx, val);
        self.autosave_snapshot();
    }

    // =========================================================================
    // Compare tf dual-slider helper
    // =========================================================================

    /// Apply a dual-handle (min/max range) slider value for `tf_*_slider` fields in
    /// the **compare** settings modal. Writes to the compare series `timeframe_visibility`.
    pub(crate) fn apply_cmp_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
        use zengeld_chart::drawing::TimeframeVisibilityConfig;

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

        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
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
        if let Some(ref mut edit) = self.panel_app.compare_settings_state.editing_text {
            if edit.field_id == edit_field {
                edit.text = value_str;
                edit.cursor = edit.text.len();
            }
        }

        eprintln!("[ChartApp] cmp_settings tf_{}_slider {:?}: {}", tf_idx, handle, value);
        self.autosave_snapshot();
    }

    /// Apply a new minimum value for the timeframe range at `tf_idx` on primitive `prim_idx`.
    pub(crate) fn apply_tf_min_value(&mut self, prim_idx: usize, tf_idx: usize, val: u32) {
        use zengeld_chart::drawing::TimeframeVisibilityConfig;
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            if let Some(mut data) = window.drawing_manager.get_data_at(prim_idx) {
                let mut tf = data.timeframe_visibility.clone()
                    .unwrap_or_else(TimeframeVisibilityConfig::all);
                match tf_idx {
                    1 => { if let Some((_, max)) = tf.seconds { tf.seconds = Some((val.min(max), max)); } }
                    2 => { if let Some((_, max)) = tf.minutes { tf.minutes = Some((val.min(max), max)); } }
                    3 => { if let Some((_, max)) = tf.hours   { tf.hours   = Some((val.min(max), max)); } }
                    4 => { if let Some((_, max)) = tf.days    { tf.days    = Some((val.min(max), max)); } }
                    5 => { if let Some((_, max)) = tf.weeks   { tf.weeks   = Some((val.min(max), max)); } }
                    6 => { if let Some((_, max)) = tf.months  { tf.months  = Some((val.min(max), max)); } }
                    _ => {}
                }
                data.timeframe_visibility = Some(tf);
                window.drawing_manager.set_data_at(prim_idx, &data);
                eprintln!("[ChartApp] prim_settings tf_{}_min committed: {}", tf_idx, val);
            }
        }
    }

    /// Apply a new maximum value for the timeframe range at `tf_idx` on primitive `prim_idx`.
    pub(crate) fn apply_tf_max_value(&mut self, prim_idx: usize, tf_idx: usize, val: u32) {
        use zengeld_chart::drawing::TimeframeVisibilityConfig;
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            if let Some(mut data) = window.drawing_manager.get_data_at(prim_idx) {
                let mut tf = data.timeframe_visibility.clone()
                    .unwrap_or_else(TimeframeVisibilityConfig::all);
                match tf_idx {
                    1 => { if let Some((min, _)) = tf.seconds { tf.seconds = Some((min, val.max(min))); } }
                    2 => { if let Some((min, _)) = tf.minutes { tf.minutes = Some((min, val.max(min))); } }
                    3 => { if let Some((min, _)) = tf.hours   { tf.hours   = Some((min, val.max(min))); } }
                    4 => { if let Some((min, _)) = tf.days    { tf.days    = Some((min, val.max(min))); } }
                    5 => { if let Some((min, _)) = tf.weeks   { tf.weeks   = Some((min, val.max(min))); } }
                    6 => { if let Some((min, _)) = tf.months  { tf.months  = Some((min, val.max(min))); } }
                    _ => {}
                }
                data.timeframe_visibility = Some(tf);
                window.drawing_manager.set_data_at(prim_idx, &data);
                eprintln!("[ChartApp] prim_settings tf_{}_max committed: {}", tf_idx, val);
            }
        }
    }

}
