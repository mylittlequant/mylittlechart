//! Color picker click/drag handlers and color appliers for the 5 picker sources
//! (primitive, indicator, chart settings, compare settings, panel).

use crate::{ChartApp, sglog};
use mlc_core::ThemeSettingsPanel;
use uzor::WidgetId;

impl ChartApp {
    /// Route a click on a color picker widget to the appropriate hit test and handler.
    pub(super) fn handle_color_picker_click(&mut self, widget_id: &str, x: f64, y: f64, source: &str) {
        use mlc_app_layout::ui::widgets::color_picker::{
            color_picker_l1_hit_test, color_picker_l2_hit_test,
            ColorPickerL1HitResult, ColorPickerL2HitResult,
        };

        // Background click — close picker.
        if widget_id.ends_with(":bg") {
            match source {
                "primitive" => self.panel_app.primitive_settings_state.close_color_picker(),
                "indicator" => self.panel_app.indicator_settings_state.close_color_picker(),
                "chart"     => self.panel_app.chart_settings_state.close_color_picker(),
                "panel"     => self.panel_app.close_panel_color_tag_picker(),
                "compare"   => self.panel_app.compare_settings_state.close_color_picker(),
                _ => {}
            }
            return;
        }

        // Extract the hit result from last frame into an owned enum value.
        // We scope the immutable borrow of self.frame_result so the mutable
        // borrow in handle_l1_hit/handle_l2_hit is allowed to follow.
        enum HitResultOwned {
            L1(ColorPickerL1HitResult),
            L2(ColorPickerL2HitResult),
            None,
        }

        let hit_owned: HitResultOwned = {
            let cp_opt = self.frame_result.as_ref()
                .and_then(|fr| fr.color_picker.as_ref());

            match cp_opt {
                None => HitResultOwned::None,
                Some(cp) => {
                    if let Some(ref l1) = cp.l1_result {
                        HitResultOwned::L1(color_picker_l1_hit_test(l1, x, y))
                    } else if let Some(ref l2) = cp.l2_result {
                        HitResultOwned::L2(color_picker_l2_hit_test(l2, x, y))
                    } else {
                        HitResultOwned::None
                    }
                }
            }
        }; // immutable borrow of self ends here

        match hit_owned {
            HitResultOwned::L1(hit) => self.handle_l1_hit(hit, source, x, y),
            HitResultOwned::L2(hit) => self.handle_l2_hit(hit, source, x, y),
            HitResultOwned::None => {}
        }
    }

    /// Handle an L1 (quick palette) hit result.
    fn handle_l1_hit(&mut self, hit: mlc_app_layout::ui::widgets::color_picker::ColorPickerL1HitResult, source: &str, _x: f64, _y: f64) {
        use mlc_app_layout::ui::widgets::color_picker::ColorPickerL1HitResult;

        match hit {
            ColorPickerL1HitResult::Color(color) => {
                // Apply to state picker.
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.select_color(&color);
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                        self.panel_app.primitive_settings_state.close_color_picker();
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.select_color(&color);
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field {
                            self.apply_indicator_color(f, &final_color);
                        }
                        self.panel_app.indicator_settings_state.close_color_picker();
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.select_color(&color);
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field {
                            self.apply_chart_settings_color(f, &final_color);
                        }
                        self.panel_app.chart_settings_state.close_color_picker();
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.select_color(&color);
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                        self.panel_app.compare_settings_state.close_color_picker();
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.select_color(&color);
                        let final_color = self.panel_app.panel_color_picker.get_final_color();
                        if self.panel_app.sync_color_grid.adding_custom_color {
                            // Adding custom color to grid — push the color and reopen grid.
                            let rgba = super::hex_str_to_rgba(&final_color);
                            self.panel_app.sync_color_grid.custom_colors.push(rgba);
                            self.panel_app.sync_color_grid.adding_custom_color = false;
                            // Grid is still open — just close the color picker.
                            self.panel_app.close_panel_color_tag_picker();
                        } else {
                            // Normal flow — assign color to leaf.
                            if let Some(leaf_id) = self.panel_app.panel_color_picker_leaf {
                                let rgba = super::hex_str_to_rgba(&final_color);
                                self.panel_app.leaf_color_tags.insert(leaf_id, rgba);
                                self.sidebar_data_dirty = true;
                            }
                            self.panel_app.close_panel_color_tag_picker();
                        }
                    }
                    _ => {}
                }
            }
            ColorPickerL1HitResult::PlusButton => {
                let picker = match source {
                    "primitive" => Some(&mut self.panel_app.primitive_settings_state.color_picker),
                    "indicator" => Some(&mut self.panel_app.indicator_settings_state.color_picker),
                    "chart"     => Some(&mut self.panel_app.chart_settings_state.color_picker),
                    "compare"   => Some(&mut self.panel_app.compare_settings_state.color_picker),
                    "panel"     => Some(&mut self.panel_app.panel_color_picker),
                    _           => None,
                };
                if let Some(picker) = picker {
                    picker.open_l2();
                    // Immediately activate hex editing so on_drag_start works
                    // without requiring an extra click to warm up geometry.
                    picker.hex_editing = true;
                    let hex = picker.hex_input.clone();
                    let hex_id = WidgetId::from(crate::text_input::HEX_COLOR);
                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&hex_id, &hex);
                    self.input_coordinator.borrow_mut().text_fields_mut().begin_edit(&hex_id);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::HEX_COLOR);
                }
            }
            ColorPickerL1HitResult::OpacitySlider(opacity) => {
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_indicator_color(f, &final_color); }
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_chart_settings_color(f, &final_color); }
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.set_opacity(opacity);
                    }
                    _ => {}
                }
            }
            ColorPickerL1HitResult::OpacityToggle => {
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_indicator_color(f, &final_color); }
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_chart_settings_color(f, &final_color); }
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.toggle_opacity();
                    }
                    _ => {}
                }
            }
            ColorPickerL1HitResult::Inside => {} // absorb
            ColorPickerL1HitResult::Outside => {
                match source {
                    "primitive" => self.panel_app.primitive_settings_state.close_color_picker(),
                    "indicator" => self.panel_app.indicator_settings_state.close_color_picker(),
                    "chart"     => self.panel_app.chart_settings_state.close_color_picker(),
                    "compare"   => self.panel_app.compare_settings_state.close_color_picker(),
                    "panel"     => {
                        self.panel_app.sync_color_grid.adding_custom_color = false;
                        self.panel_app.close_panel_color_tag_picker();
                    }
                    _ => {}
                }
            }
        }
    }

    /// Handle an L2 (HSV full picker) hit result.
    fn handle_l2_hit(&mut self, hit: mlc_app_layout::ui::widgets::color_picker::ColorPickerL2HitResult, source: &str, x: f64, y: f64) {
        use mlc_app_layout::ui::widgets::color_picker::ColorPickerL2HitResult;

        match hit {
            ColorPickerL2HitResult::SVSquare(s, v) => {
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.set_sv(s, v);
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.set_sv(s, v);
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_indicator_color(f, &final_color); }
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.set_sv(s, v);
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_chart_settings_color(f, &final_color); }
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.set_sv(s, v);
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.set_sv(s, v);
                    }
                    _ => {}
                }
            }
            ColorPickerL2HitResult::HueBar(h) => {
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.set_hue(h);
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.set_hue(h);
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_indicator_color(f, &final_color); }
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.set_hue(h);
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_chart_settings_color(f, &final_color); }
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.set_hue(h);
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.set_hue(h);
                    }
                    _ => {}
                }
            }
            ColorPickerL2HitResult::OpacitySlider(opacity) => {
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_indicator_color(f, &final_color); }
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_chart_settings_color(f, &final_color); }
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.set_opacity(opacity);
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.set_opacity(opacity);
                    }
                    _ => {}
                }
            }
            ColorPickerL2HitResult::OpacityToggle => {
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_indicator_color(f, &final_color); }
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_chart_settings_color(f, &final_color); }
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.toggle_opacity();
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.toggle_opacity();
                    }
                    _ => {}
                }
            }
            ColorPickerL2HitResult::AddButton => {
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.add_to_custom();
                        let final_color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                        self.apply_primitive_color(&final_color);
                        self.panel_app.primitive_settings_state.close_color_picker();
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.add_to_custom();
                        let final_color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_indicator_color(f, &final_color); }
                        self.panel_app.indicator_settings_state.close_color_picker();
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.add_to_custom();
                        let final_color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                        let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                        if let Some(ref f) = field { self.apply_chart_settings_color(f, &final_color); }
                        self.panel_app.chart_settings_state.close_color_picker();
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.add_to_custom();
                        let final_color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                        self.apply_compare_color(&final_color);
                        self.panel_app.compare_settings_state.close_color_picker();
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.add_to_custom();
                        let final_color = self.panel_app.panel_color_picker.get_final_color();
                        if self.panel_app.sync_color_grid.adding_custom_color {
                            // Adding custom color to grid — push the color and reopen grid.
                            let rgba = super::hex_str_to_rgba(&final_color);
                            self.panel_app.sync_color_grid.custom_colors.push(rgba);
                            self.panel_app.sync_color_grid.adding_custom_color = false;
                            // Grid is still open — just close the color picker.
                            self.panel_app.close_panel_color_tag_picker();
                        } else {
                            // Normal flow — assign color to leaf.
                            if let Some(leaf_id) = self.panel_app.panel_color_picker_leaf {
                                let rgba = super::hex_str_to_rgba(&final_color);
                                self.panel_app.leaf_color_tags.insert(leaf_id, rgba);
                                self.sidebar_data_dirty = true;
                            }
                            self.panel_app.close_panel_color_tag_picker();
                        }
                    }
                    _ => {}
                }
            }
            ColorPickerL2HitResult::BackButton => {
                match source {
                    "primitive" => self.panel_app.primitive_settings_state.color_picker.back_to_l1(),
                    "indicator" => self.panel_app.indicator_settings_state.color_picker.back_to_l1(),
                    "chart"     => self.panel_app.chart_settings_state.color_picker.back_to_l1(),
                    "compare"   => self.panel_app.compare_settings_state.color_picker.back_to_l1(),
                    "panel"     => self.panel_app.panel_color_picker.back_to_l1(),
                    _ => {}
                }
            }
            ColorPickerL2HitResult::HexInput => {
                // hex_editing is already true (set when L2 opens).
                // Reposition cursor at click x via text-field store.
                self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
            }
            ColorPickerL2HitResult::Inside => {} // absorb
            ColorPickerL2HitResult::Outside => {
                match source {
                    "primitive" => self.panel_app.primitive_settings_state.close_color_picker(),
                    "indicator" => self.panel_app.indicator_settings_state.close_color_picker(),
                    "chart"     => self.panel_app.chart_settings_state.close_color_picker(),
                    "compare"   => self.panel_app.compare_settings_state.close_color_picker(),
                    "panel"     => {
                        self.panel_app.sync_color_grid.adding_custom_color = false;
                        self.panel_app.close_panel_color_tag_picker();
                    }
                    _ => {}
                }
            }
        }
    }

    // =========================================================================
    // Sync Color Grid Handler
    // =========================================================================

    /// Handle clicks on widgets registered with the "sync_color_grid:" prefix.
    pub(super) fn handle_sync_color_grid_click(&mut self, widget_id: &str, x: f64, y: f64) {
        sglog!(
            "[sync_color_grid_click] ENTER widget='{}' xy=({:.0},{:.0}) target_leaf={:?} target_panel={:?}",
            widget_id, x, y,
            self.panel_app.sync_color_grid.target_leaf,
            self.panel_app.sync_color_grid.target_panel
        );
        use mlc_app_layout::ui::sync_color_grid::{hit_test_sync_color_grid, SyncColorGridHitResult};

        // Backdrop click (full-screen transparent rect behind the popup) — close.
        if widget_id == "sync_color_grid:backdrop" {
            self.panel_app.sync_color_grid.close();
            return;
        }

        // Background click or unknown sub-id without a draw result — close.
        if widget_id == "sync_color_grid:bg" {
            // Hit-test to determine whether the click was truly outside.
            let draw_result = match self.frame_result.as_ref()
                .and_then(|fr| fr.sync_color_grid.as_ref())
            {
                Some(dr) => dr.clone(),
                None => {
                    self.panel_app.sync_color_grid.close();
                    return;
                }
            };

            let hit = hit_test_sync_color_grid(&draw_result, x, y);
            match hit {
                SyncColorGridHitResult::Outside => {
                    self.panel_app.sync_color_grid.close();
                }
                _ => {} // absorb click inside the popup
            }
            return;
        }

        // Remove action — full desync (purge cloned primitives/indicators + remove color tag)
        if widget_id == "sync_color_grid:remove" {
            if let Some(leaf_id) = self.panel_app.sync_color_grid.target_leaf {
                if self.panel_app.leaf_color_tags.contains_key(&leaf_id) {
                    self.perform_desync(leaf_id);
                    // Bug E fix: persist the untag operation immediately.
                    self.autosave_snapshot();
                }
            } else if let Some(panel_id) = self.panel_app.sync_color_grid.target_panel {
                self.perform_panel_desync(panel_id);
            }
            self.panel_app.sync_color_grid.close();
            return;
        }

        if widget_id == "sync_color_grid:add" {
            // Grid stays open while L1/L2 picker is shown on top.
            self.panel_app.sync_color_grid.adding_custom_color = true;

            if let Some(leaf_id) = self.panel_app.sync_color_grid.target_leaf {
                let (ww, wh) = (self.width as f64, self.height as f64);
                // Anchor L1 below the grid popup so it doesn't overlap.
                let (gx, gy) = self.panel_app.sync_color_grid.origin;
                let (_, gh) = self.panel_app.sync_color_grid.popup_size();
                self.panel_app.open_panel_color_tag_picker(
                    leaf_id,
                    [gx, gy + gh, 0.0, 0.0],
                    ww, wh,
                    None,
                );
            }
            return;
        }

        // Swatch click: "sync_color_grid:swatch:{idx}"
        if let Some(rest) = widget_id.strip_prefix("sync_color_grid:swatch:") {
            if let Ok(idx) = rest.parse::<usize>() {
                let colors = self.panel_app.sync_color_grid.all_colors();
                if let Some(&new_color) = colors.get(idx) {
                    if let Some(leaf_id) = self.panel_app.sync_color_grid.target_leaf {
                        let old_color = self.panel_app.leaf_color_tags.get(&leaf_id).copied();
                        let same_color = old_color.is_some_and(|oc|
                            (oc[0] - new_color[0]).abs() < 0.01
                            && (oc[1] - new_color[1]).abs() < 0.01
                            && (oc[2] - new_color[2]).abs() < 0.01
                        );
                        sglog!(
                            "[swatch-leaf] leaf={:?} idx={} new=[{:.2},{:.2},{:.2}] old={:?} same={} -> path={}",
                            leaf_id, idx, new_color[0], new_color[1], new_color[2],
                            old_color, same_color,
                            if same_color { "skip" }
                            else if old_color.is_some() { "retag(desync+join)" }
                            else { "tag-from-untagged(disconnect+join)" }
                        );
                        if !same_color {
                            let is_retag = old_color.is_some();
                            if is_retag {
                                // RE-TAG (was already in a color tag): detach
                                // without restoring the home stash into
                                // drawing_manager / indicators.  The previous
                                // tag's primitives + indicators stay with it;
                                // the new tag is a fresh empty bucket (or, if
                                // the color is already used by another chart,
                                // a standard peer-join into that group).
                                self.disconnect_from_current_group(leaf_id);
                            } else {
                                // Bug D fix: window was in an invisible auto group — clean it up
                                // before joining a color tag, so the old auto group doesn't
                                // retain this chart_id as a stale member.
                                self.disconnect_from_current_group(leaf_id);
                            }
                            // Assign new color.
                            self.panel_app.leaf_color_tags.insert(leaf_id, new_color);
                            sglog!(
                                "[swatch-leaf] assigned color to leaf {:?}, calling sync_join_color_group (is_retag={})",
                                leaf_id, is_retag
                            );
                            // Sync with new group — clone primitives/indicators from peers.
                            if is_retag {
                                self.sync_join_color_group_retag(leaf_id, new_color);
                            } else {
                                self.sync_join_color_group(leaf_id, new_color);
                            }
                            self.sidebar_data_dirty = true;
                            // Persist tag assignment immediately.
                            self.autosave_snapshot();
                        }
                    } else if let Some(panel_id) = self.panel_app.sync_color_grid.target_panel {
                        use mlc_core::ids::SyncMemberId;
                        let member = SyncMemberId::Panel(panel_id);
                        let old_color = self.group_for_member(member)
                            .and_then(|gid| self.core.tags().get(gid).and_then(|t| (!t.auto_created).then_some(t.color)));
                        let same_color = old_color.is_some_and(|oc|
                            (oc[0] - new_color[0]).abs() < 0.01
                            && (oc[1] - new_color[1]).abs() < 0.01
                            && (oc[2] - new_color[2]).abs() < 0.01
                        );
                        if !same_color {
                            self.disconnect_member(member);
                            let existing_tag = self.core.tags().find_tag_by_color(new_color);
                            let is_new_tag = existing_tag.is_none();
                            let target_gid = match existing_tag {
                                Some(gid) => gid,
                                None => self.create_tagged_bubble(
                                    new_color,
                                    String::new(),
                                    mlc_core::state::Timeframe::m1(),
                                ),
                            };
                            self.connect_member(member, target_gid);
                            self.core.tags_mut().unmark_synced(member);
                            log::debug!(
                                "[ChartApp] Panel {} → color group [{:.2},{:.2},{:.2}]",
                                panel_id, new_color[0], new_color[1], new_color[2]
                            );

                            // TradingContainer: color-sync groups are now the ONLY
                            // symbol-follow mechanism (Auto/Pinned source badge
                            // removed). Joining a group must rebind immediately —
                            // not wait for the next active-chart-change /
                            // propagate_symbol tick to reach this group.
                            let tc_pid = mlc_core::state::PanelId(panel_id);
                            let tc_current: Option<(String, String, String)> = self.panels_store
                                .trading_container
                                .get(&tc_pid)
                                .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone()));
                            if let Some((cur_symbol, cur_exchange, cur_account_type)) = tc_current {
                                if is_new_tag && !cur_symbol.is_empty() {
                                    // Brand-new tag: seed it from the joining container's
                                    // own current instrument instead of leaving it blank —
                                    // mirrors the chart-side new-group seed in
                                    // `sync_join_color_group_inner`.
                                    self.core.bubbles_mut().set_instrument(
                                        target_gid, cur_symbol, cur_exchange, cur_account_type,
                                    );
                                }
                                let resolved = self.core.bubbles().instrument(target_gid)
                                    .map(|instrument| mlc_app_panels::trading::ResolvedSymbol {
                                        symbol: instrument.address.symbol,
                                        exchange: instrument.address.exchange,
                                        account_type: instrument.address.account_type.as_label().to_string(),
                                    });
                                if let Some(resolved) = resolved {
                                    if !resolved.symbol.is_empty() && !resolved.exchange.is_empty() {
                                        self.rebind_trading_container(tc_pid, resolved);
                                    }
                                }
                            }

                            self.sidebar_data_dirty = true;
                            self.autosave_snapshot();
                        }
                    }
                }
                self.panel_app.sync_color_grid.close();
            }
            return;
        }

        // Unknown sub-id — absorb
        let _ = (x, y);
    }

    /// Apply the current color picker color to the selected primitive by field name.
    pub(super) fn apply_primitive_color(&mut self, color: &str) {
        match self.panel_app.primitive_settings_state.primitive_id {
            Some(prim_id) => {
                let color = color.to_string();
                self.edit_primitive_recorded(prim_id, |app| app.apply_primitive_color_inner(&color))
            }
            None => self.apply_primitive_color_inner(color),
        }
    }

    fn apply_primitive_color_inner(&mut self, color: &str) {
        let field = match self.panel_app.primitive_settings_state.color_picker_field.clone() {
            Some(f) => f,
            None => return,
        };
        let prim_id = match self.panel_app.primitive_settings_state.primitive_id {
            Some(i) => i,
            None => return,
        };
        let active_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        if let Some(bid) = active_bid {
            let data_opt = self.core.drawings().get_data(bid, prim_id);
            if let Some(mut data) = data_opt {
                let style_prop_change = if field.starts_with("style_prop:") {
                    let prop_id = field["style_prop:".len()..].to_string();
                    let value = mlc_core::drawing::primitives_v2::config::PropertyValue::Color(color.to_string());
                    Some((prop_id, value))
                } else {
                    None
                };
                // Level colour swatch, `PropertyKind::Level` twin of
                // `style_prop_change` above — no `level_properties()`
                // override declares a `Color` property today, but a future
                // one that does must not silently drop the picked colour.
                let level_prop_change = if field.starts_with("level_prop:") {
                    let prop_id = field["level_prop:".len()..].to_string();
                    let value = mlc_core::drawing::primitives_v2::config::PropertyValue::Color(color.to_string());
                    Some((prop_id, value))
                } else {
                    None
                };
                // Level colour swatch: `level_v{key}_color` (the level's own
                // line) / `level_v{key}_fill_color` (its band, from the row
                // inspector). `{key}` is the level's VALUE, not a row index
                // (see `vkey()` in `primitive_settings_levels.rs`), so this
                // resolves by matching `FibLevelConfig::level`, same as
                // `mlc-app-layout/.../primitive_settings_intent.rs`'s own
                // `set_level_color_by_value` (the `mlc-ui-lab` host's
                // equivalent of this block — mlc-app can't call it directly:
                // it operates on `&mut dyn Primitive`, mlc-app only ever
                // gets a `Vec<FibLevelConfig>` out of the drawings facade).
                // `_fill_color` is checked BEFORE the plain `_color` suffix:
                // `"..._fill_color".strip_suffix("_color")` also succeeds
                // (leaving an unparseable `"..._fill"` key), so the longer,
                // fill-specific suffix must be tried first — same trap
                // already fixed in `mlc-app/src/input/settings.rs`'s own
                // `level_v*` swatch-click dispatch.
                let level_color_change: Option<(f64, bool)> = field.strip_prefix("level_v").and_then(|rest| {
                    if let Some(key) = rest.strip_suffix("_fill_color") {
                        key.parse::<f64>().ok().map(|level| (level, true))
                    } else if let Some(key) = rest.strip_suffix("_color") {
                        key.parse::<f64>().ok().map(|level| (level, false))
                    } else {
                        None
                    }
                });
                match field.as_str() {
                    "stroke_color" => {
                        data.color.stroke = color.to_string();
                    }
                    "fill_color" => {
                        data.color.fill = Some(color.to_string());
                    }
                    "text_color" => {
                        if let Some(ref mut text) = data.text {
                            text.color = Some(color.to_string());
                        }
                    }
                    _ => {}
                }
                let mut guard = self.core.drawings_mut();
                if let Some((prop_id, value)) = style_prop_change {
                    guard.apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Style, &prop_id, &value);
                }
                if let Some((prop_id, value)) = level_prop_change {
                    guard.apply_property(bid, prim_id, mlc_core::drawing::primitives_v2::PropertyKind::Level, &prop_id, &value);
                }
                if let Some((level, is_fill)) = level_color_change {
                    let configs = guard.primitives(bid).iter().find(|p| p.data().id == prim_id).and_then(|p| p.level_configs());
                    if let Some(mut configs) = configs {
                        if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < 1e-9) {
                            if is_fill {
                                cfg.fill_color = Some(color.to_string());
                                cfg.fill_enabled = true;
                            } else {
                                cfg.color = Some(color.to_string());
                            }
                        }
                        guard.set_level_configs(bid, prim_id, configs);
                    }
                }
                guard.set_data(bid, prim_id, &data);
            }
        }
        self.sync_drawing_back_to_group();
        log::debug!("[ChartApp] applied primitive color: {} = {}", field, color);
        self.autosave_snapshot();
        if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
            self.snapshot_primitive_settings_to_user_manager(prim_id);
        }
    }

    /// Apply the color picker color to the active indicator output.
    pub(super) fn apply_indicator_color(&mut self, output_name: &str, color: &str) {
        if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
            match output_name {
                "signal_bullish_color" => {
                    self.core.indicators_mut().set_signal_bullish_color(ind_id, color);
                    log::debug!("[ChartApp] applied indicator signal_bullish_color: {} = {}", ind_id, color);
                }
                "signal_bearish_color" => {
                    self.core.indicators_mut().set_signal_bearish_color(ind_id, color);
                    log::debug!("[ChartApp] applied indicator signal_bearish_color: {} = {}", ind_id, color);
                }
                _ => {
                    if self.core.indicators_mut().set_output_color(ind_id, output_name, color) {
                        log::debug!("[ChartApp] applied indicator color: {} output '{}' = {}", ind_id, output_name, color);
                    }
                }
            }
        }
        self.autosave_snapshot();
        self.sidebar_data_dirty = true;
        self.snapshot_indicator_settings_to_user_manager();
    }

    /// Apply the color picker color to chart settings fields.
    pub(super) fn apply_chart_settings_color(&mut self, field: &str, color: &str) {
        match field {
            "crosshair_line_color" => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    panel.crosshair_options.vert_line.color = color.to_string();
                    panel.crosshair_options.horz_line.color = color.to_string();
                    log::debug!("[ChartApp] applied crosshair_line_color: {}", color);
                }
            }
            "body_up_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_up_body = color.to_string();
                log::debug!("[ChartApp] applied body_up_color: {}", color);
            }
            "body_down_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_down_body = color.to_string();
                log::debug!("[ChartApp] applied body_down_color: {}", color);
            }
            "border_up_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_up_border = Some(color.to_string());
                log::debug!("[ChartApp] applied border_up_color: {}", color);
            }
            "border_down_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_down_border = Some(color.to_string());
                log::debug!("[ChartApp] applied border_down_color: {}", color);
            }
            "wick_up_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_up_wick = color.to_string();
                log::debug!("[ChartApp] applied wick_up_color: {}", color);
            }
            "wick_down_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_down_wick = color.to_string();
                log::debug!("[ChartApp] applied wick_down_color: {}", color);
            }
            "footprint_buy_color" => {
                self.panel_app.theme_manager.current_mut().series.footprint_buy = color.to_string();
                log::debug!("[ChartApp] applied footprint_buy_color: {}", color);
            }
            "footprint_sell_color" => {
                self.panel_app.theme_manager.current_mut().series.footprint_sell = color.to_string();
                log::debug!("[ChartApp] applied footprint_sell_color: {}", color);
            }
            "renko_up_color" => {
                self.panel_app.theme_manager.current_mut().series.renko_up = color.to_string();
                log::debug!("[ChartApp] applied renko_up_color: {}", color);
            }
            "renko_down_color" => {
                self.panel_app.theme_manager.current_mut().series.renko_down = color.to_string();
                log::debug!("[ChartApp] applied renko_down_color: {}", color);
            }
            "pnf_x_color" => {
                self.panel_app.theme_manager.current_mut().series.pnf_x = color.to_string();
                log::debug!("[ChartApp] applied pnf_x_color: {}", color);
            }
            "pnf_o_color" => {
                self.panel_app.theme_manager.current_mut().series.pnf_o = color.to_string();
                log::debug!("[ChartApp] applied pnf_o_color: {}", color);
            }
            "tlb_up_color" => {
                self.panel_app.theme_manager.current_mut().series.tlb_up = color.to_string();
                log::debug!("[ChartApp] applied tlb_up_color: {}", color);
            }
            "tlb_down_color" => {
                self.panel_app.theme_manager.current_mut().series.tlb_down = color.to_string();
                log::debug!("[ChartApp] applied tlb_down_color: {}", color);
            }
            "kagi_yang_color" => {
                self.panel_app.theme_manager.current_mut().series.kagi_yang = color.to_string();
                log::debug!("[ChartApp] applied kagi_yang_color: {}", color);
            }
            "kagi_yin_color" => {
                self.panel_app.theme_manager.current_mut().series.kagi_yin = color.to_string();
                log::debug!("[ChartApp] applied kagi_yin_color: {}", color);
            }
            "hollow_up_color" => {
                self.panel_app.theme_manager.current_mut().series.hollow_up = color.to_string();
                log::debug!("[ChartApp] applied hollow_up_color: {}", color);
            }
            "hollow_down_color" => {
                self.panel_app.theme_manager.current_mut().series.hollow_down = color.to_string();
                log::debug!("[ChartApp] applied hollow_down_color: {}", color);
            }
            "line_color" => {
                self.panel_app.theme_manager.current_mut().series.line_color = color.to_string();
                log::debug!("[ChartApp] applied line_color: {}", color);
            }
            "step_line_color" => {
                self.panel_app.theme_manager.current_mut().series.step_line_color = color.to_string();
                log::debug!("[ChartApp] applied step_line_color: {}", color);
            }
            "area_line_color" => {
                self.panel_app.theme_manager.current_mut().series.area_line = color.to_string();
                log::debug!("[ChartApp] applied area_line_color: {}", color);
            }
            "area_top_color" => {
                self.panel_app.theme_manager.current_mut().series.area_top = color.to_string();
                log::debug!("[ChartApp] applied area_top_color: {}", color);
            }
            "area_bottom_color" => {
                self.panel_app.theme_manager.current_mut().series.area_bottom = color.to_string();
                log::debug!("[ChartApp] applied area_bottom_color: {}", color);
            }
            "baseline_top_line_color" => {
                self.panel_app.theme_manager.current_mut().series.baseline_top_line = color.to_string();
                log::debug!("[ChartApp] applied baseline_top_line_color: {}", color);
            }
            "baseline_top_fill_color" => {
                self.panel_app.theme_manager.current_mut().series.baseline_top_fill = color.to_string();
                log::debug!("[ChartApp] applied baseline_top_fill_color: {}", color);
            }
            "baseline_bottom_line_color" => {
                self.panel_app.theme_manager.current_mut().series.baseline_bottom_line = color.to_string();
                log::debug!("[ChartApp] applied baseline_bottom_line_color: {}", color);
            }
            "baseline_bottom_fill_color" => {
                self.panel_app.theme_manager.current_mut().series.baseline_bottom_fill = color.to_string();
                log::debug!("[ChartApp] applied baseline_bottom_fill_color: {}", color);
            }
            "bars_up_color" => {
                self.panel_app.theme_manager.current_mut().series.bar_up = color.to_string();
                log::debug!("[ChartApp] applied bars_up_color: {}", color);
            }
            "bars_down_color" => {
                self.panel_app.theme_manager.current_mut().series.bar_down = color.to_string();
                log::debug!("[ChartApp] applied bars_down_color: {}", color);
            }
            "line_markers_color" => {
                self.panel_app.theme_manager.current_mut().series.line_markers_color = color.to_string();
                log::debug!("[ChartApp] applied line_markers_color: {}", color);
            }
            "watermark_color" => {
                let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
                if let Some(pid) = pid {
                    let panel = crate::chart_panel_bridge::ensure_chart_panel_state(&mut self.panels_store, pid);
                    if let Some(ref mut watermark) = panel.watermark {
                        if let Some(line) = watermark.lines.first_mut() {
                            line.color = color.to_string();
                            log::debug!("[ChartApp] applied watermark_color: {}", color);
                        }
                    }
                }
            }
            other => {
                // Try to apply as a theme color via ThemeSettingsPanel
                // (for appearance tab fields that reference theme colors)
                if ThemeSettingsPanel::set_color_by_id(
                    &mut self.panel_app.theme_manager,
                    other,
                    color,
                ) {
                    log::debug!("[ChartApp] applied appearance color '{}' = {}", other, color);
                } else {
                    log::debug!("[ChartApp] apply_chart_settings_color: unknown field '{}' = {}", other, color);
                }
            }
        }
        self.autosave_snapshot();
        self.snapshot_chart_settings_to_user_manager();
    }

    /// Apply a color change to the compare series currently being edited.
    ///
    /// Every colour path in the picker funnels here, so this is where the
    /// edit becomes undoable. A no-op repaint (the picker re-applying the
    /// colour it already has) records nothing — otherwise moving the cursor
    /// over a swatch would fill the stack with commands that change nothing.
    pub(super) fn apply_compare_color(&mut self, color: &str) {
        let idx = self.panel_app.compare_settings_state.series_index;
        self.panel_app.compare_settings_state.cached_color = color.to_string();
        self.edit_compare_style(idx, |ov| ov.set_series_color_by_index(idx, color));
        log::debug!("[ChartApp] cmp_settings color applied: idx={} color={}", idx, color);
        self.snapshot_compare_settings_to_user_manager();
    }

    /// Apply the current mouse position to the active color picker L2 drag.
    ///
    /// Called from `on_drag_start` (initial value) and `on_drag_move`.
    pub(super) fn apply_color_picker_drag(&mut self, x: f64, y: f64) {
        // Clone the drag state so we can borrow self mutably below.
        let drag = match self.color_picker_drag.clone() {
            Some(d) => d,
            None => return,
        };

        match drag.area {
            crate::ColorPickerDragArea::SVSquare => {
                let (sx, sy, sw, sh) = drag.sv_rect;
                if sw > 0.0 && sh > 0.0 {
                    let s = ((x - sx) / sw).clamp(0.0, 1.0);
                    let v = 1.0 - ((y - sy) / sh).clamp(0.0, 1.0);
                    match drag.source.as_str() {
                        "primitive" => {
                            self.panel_app.primitive_settings_state.color_picker.set_sv(s, v);
                            let color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                            self.apply_primitive_color(&color);
                        }
                        "indicator" => {
                            self.panel_app.indicator_settings_state.color_picker.set_sv(s, v);
                            let color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                            let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                            if let Some(ref f) = field { self.apply_indicator_color(f, &color); }
                        }
                        "chart" => {
                            self.panel_app.chart_settings_state.color_picker.set_sv(s, v);
                            let color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                            let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                            if let Some(ref f) = field { self.apply_chart_settings_color(f, &color); }
                        }
                        _ => {}
                    }
                }
            }
            crate::ColorPickerDragArea::HueBar => {
                let (_hx, hy, _hw, hh) = drag.hue_rect;
                if hh > 0.0 {
                    let hue = ((y - hy) / hh).clamp(0.0, 1.0) * 360.0;
                    match drag.source.as_str() {
                        "primitive" => {
                            self.panel_app.primitive_settings_state.color_picker.set_hue(hue);
                            let color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                            self.apply_primitive_color(&color);
                        }
                        "indicator" => {
                            self.panel_app.indicator_settings_state.color_picker.set_hue(hue);
                            let color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                            let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                            if let Some(ref f) = field { self.apply_indicator_color(f, &color); }
                        }
                        "chart" => {
                            self.panel_app.chart_settings_state.color_picker.set_hue(hue);
                            let color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                            let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                            if let Some(ref f) = field { self.apply_chart_settings_color(f, &color); }
                        }
                        _ => {}
                    }
                }
            }
            crate::ColorPickerDragArea::OpacitySlider => {
                let (ox, _oy, ow, _oh) = drag.opacity_rect;
                if ow > 0.0 {
                    let opacity = ((x - ox) / ow).clamp(0.0, 1.0);
                    match drag.source.as_str() {
                        "primitive" => {
                            self.panel_app.primitive_settings_state.color_picker.set_opacity(opacity);
                            let color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                            self.apply_primitive_color(&color);
                        }
                        "indicator" => {
                            self.panel_app.indicator_settings_state.color_picker.set_opacity(opacity);
                            let color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                            let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                            if let Some(ref f) = field { self.apply_indicator_color(f, &color); }
                        }
                        "chart" => {
                            self.panel_app.chart_settings_state.color_picker.set_opacity(opacity);
                            let color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                            let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                            if let Some(ref f) = field { self.apply_chart_settings_color(f, &color); }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
