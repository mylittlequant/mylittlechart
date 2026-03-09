//! Input handling for ChartApp.
//!
//! This module provides all user-interaction methods for `ChartApp`:
//! click, drag, hover, scroll, and keyboard escape.
//!
//! ## Design notes
//!
//! - `on_click` first checks the `input_coordinator` (which owns modal and
//!   toolbar widget registrations) via `process_click()`, then falls through
//!   to chart-canvas hit testing.
//! - Drag, scroll, and hover are forwarded to `DefaultChartInputHandler.process_action()`
//!   and the resulting `ChartOutputAction` values are applied via `process_output_actions`.
//! - There are no dependencies on the terminal crate; all types come from
//!   `zengeld-chart` and `uzor-core`.

// =============================================================================
// KeyPress — named key events forwarded from the platform runner
// =============================================================================

/// Named key events that cannot be represented as a `char`.
///
/// The platform runner maps winit `NamedKey` variants to these and calls
/// `ChartApp::on_key_press`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPress {
    /// Delete character at cursor (forward delete)
    Delete,
    /// Move cursor one character to the left
    ArrowLeft,
    /// Move cursor one character to the right
    ArrowRight,
    /// Move cursor to start of text
    Home,
    /// Move cursor to end of text
    End,
    /// Select all text (Ctrl+A)
    SelectAll,
    /// Extend selection one character to the left (Shift+Left)
    ShiftLeft,
    /// Extend selection one character to the right (Shift+Right)
    ShiftRight,
    /// Extend selection to start of text (Shift+Home)
    ShiftHome,
    /// Extend selection to end of text (Shift+End)
    ShiftEnd,
    /// Copy selected text to clipboard (Ctrl+C) — text returned via `on_copy_selection`
    Copy,
    /// Paste text from clipboard (Ctrl+V) — text supplied via `on_paste_text`
    Paste(String),
    /// Undo last action (Ctrl+Z)
    Undo,
    /// Redo last undone action (Ctrl+Y / Ctrl+Shift+Z)
    Redo,
}

use crate::ChartApp;
use zengeld_chart::{
    ChartInputAction,
    ChartOutputAction,
    ChartHitTester,
    ExtendedFrameLayout, ExtendedLayoutHitTester,
    LayoutRect,
    ScaleCornerButton,
    ChartPanelLayout,
    CursorStyle,
    input::DragMode,
    input::MouseButton,
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
    ScaleMode,
};
use zengeld_chart::ui::context_menu::{
    ContextMenuTarget, ContextMenuItemState,
    build_primitive_context_menu,
};
use zengeld_chart::ui::modal_state::{OpenModal, IndicatorCategoryFilter};
use zengeld_chart::ui::modal_settings::DualSliderHandle;
use zengeld_chart::drawing::TimeframeVisibilityConfig;

// =============================================================================
// ChartApp input methods
// =============================================================================

impl ChartApp {
    // -------------------------------------------------------------------------
    // Click
    // -------------------------------------------------------------------------

    /// Handle a left-click at screen coordinates `(x, y)`.
    ///
    /// Dispatch order:
    /// 1. `input_coordinator.process_click()` — modals, toolbars
    /// 2. Chart-canvas hit testing (crosshair, primitives, zoom)
    pub fn on_click(&mut self, x: f64, y: f64) {
        // 1. Check the input coordinator (modals, toolbars, dropdowns).
        // This MUST come before the drawing tool guard so toolbar clicks still work.
        // Drop the RefMut borrow before calling dispatch_panel_click (which needs &mut self).
        let clicked_widget_id = self.input_coordinator.borrow_mut().process_click(x, y)
            .map(|w| w.0.clone());
        if let Some(id) = clicked_widget_id {
            eprintln!("[ChartApp] click dispatched to: {}", id);
            self.dispatch_panel_click(&id, x, y);
            return;
        }

        // 2. Click outside any registered widget — check for modal backdrop.
        //    Use layered close: only close the topmost modal layer, not all at once.
        let in_modal = self.input_coordinator.borrow_mut().is_point_in_modal_layer(x, y);
        if in_modal {
            self.close_topmost_modal_layer();
            return;
        }

        // 3. Click on canvas — close any open dropdown and context menu first.
        self.panel_app.toolbar_state.open_dropdown_id = None;
        self.panel_app.toolbar_state.hovered_dropdown_item = None;
        // Close inline dropdowns on canvas click
        self.panel_app.toolbar_state.open_inline_style_dropdown = false;
        self.panel_app.toolbar_state.open_inline_width_dropdown = false;
        self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
        self.panel_app.context_menu_state.close();
        self.modal_state.close();
        self.sidebar_state.watchlist_config_dropdown_open = false;
        // Close watchlist color picker when clicking outside any registered widget.
        self.sidebar_state.watchlist_color_picker_open = None;

        // 4. Split panel routing — route click to the correct leaf.
        if self.panel_app.panel_grid.is_split() {
            use zengeld_chart::state::ChartInputTarget;
            let target = self.panel_app.panel_grid.resolve_input(
                x, y, self.content_rect.x, self.content_rect.y,
            );
            match target {
                ChartInputTarget::Separator { .. } => {
                    // Separator click (not a drag) — no action needed.
                    return;
                }
                ChartInputTarget::ScaleCorner { leaf_id, button } => {
                    self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    // Handle the scale corner click on the correct leaf's window.
                    use zengeld_chart::ScaleCornerButton;
                    match button {
                        ScaleCornerButton::AutoManual => {
                            use zengeld_chart::ScaleMode;
                            let current_mode = self.panel_app.panel_grid
                                .window_for_leaf(leaf_id)
                                .map(|w| w.price_scale.scale_mode)
                                .unwrap_or(ScaleMode::Auto);
                            let next_mode = current_mode.next();
                            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                                window.price_scale.scale_mode = next_mode;
                                if next_mode.is_follow() {
                                    let count = window.bars.len();
                                    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
                                    let right_margin = 2.0_f64;
                                    window.viewport.view_start = (count as f64 + right_margin - visible_f).max(0.0);
                                }
                                if next_mode.is_auto_y() {
                                    window.calc_auto_scale();
                                }
                            }
                        }
                        ScaleCornerButton::Mode => {
                            self.process_output_actions(vec![zengeld_chart::ChartOutputAction::TogglePriceScaleMode]);
                        }
                        ScaleCornerButton::None => {}
                    }
                    return;
                }
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id } => {
                    self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    // Fall through to handle_canvas_click using the now-active leaf.
                }
                ChartInputTarget::None => return,
            }
        }

        // When a click-based drawing tool is active, canvas clicks come from
        // on_drag_start (mouse press). Ignore the release-path canvas click.
        if self.has_click_drawing_tool() {
            return;
        }

        self.handle_canvas_click(x, y);
    }

    /// Handle a right-click at screen coordinates `(x, y)`.
    ///
    /// Opens a context menu: primitive menu if clicking on a primitive,
    /// chart background menu otherwise.
    pub fn on_right_click(&mut self, x: f64, y: f64) {
        // Suppress context menu when any modal is open — right-clicks inside
        // modals must not bleed through to the chart background.
        if self.modal_state.is_open()
            || self.watchlist_modal.is_open()
            || self.wl_group_name_input.is_open()
            || self.panel_app.primitive_settings_state.is_open()
            || self.panel_app.indicator_settings_state.is_open()
            || self.panel_app.alert_settings_state.is_open()
            || self.panel_app.compare_settings_state.is_open()
            || self.panel_app.chart_settings_state.is_open
            || self.panel_app.preset_name_input.is_open
            || self.panel_app.chart_browser.is_open
            || self.panel_app.overlay_settings_state.is_open
            || self.panel_app.tags_tabs_state.is_open
        {
            return;
        }

        // Close any existing context menu / dropdown first.
        self.panel_app.context_menu_state.close();
        self.panel_app.toolbar_state.open_dropdown_id = None;

        let w = self.width as f64;
        let h = self.height as f64;

        // Right-click on color-tag square: no context menu (the popup has a Remove button).

        // Hit-test primitives — check if click landed on a drawing primitive.
        // Check main chart first, then sub-panes.
        let primitive_hit = {
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;

            let main_hit = if local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height
            {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    window.drawing_manager.hit_test(
                        local_x,
                        local_y,
                        &window.viewport,
                        &window.price_scale,
                    )
                } else {
                    None
                }
            } else {
                None
            };

            // If main chart has no hit, check sub-panes.
            let result = if main_hit.is_some() {
                main_hit
            } else {
                let mut sub_hit = None;
                for pane_layout in extended.sub_panes.iter() {
                    let content = pane_layout.content;
                    let plx = x - content.x;
                    let ply = y - content.y;
                    if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                        continue;
                    }
                    let instance_id = pane_layout.instance_id;
                    let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            w.sub_panes.iter()
                                .find(|sp| sp.instance_id == instance_id)
                                .map(|sp| (sp.price_min, sp.price_max))
                        })
                        .unwrap_or((0.0, 100.0));
                    let sub_price_scale = zengeld_chart::PriceScale::new(price_min, price_max);
                    let sub_viewport = self.panel_app.panel_grid.active_window()
                        .map(|w| {
                            let mut vp = w.viewport.clone();
                            vp.chart_height = content.height;
                            vp
                        });
                    if let (Some(sub_viewport), Some(window)) = (sub_viewport, self.panel_app.panel_grid.active_window()) {
                        if let Some(prim_idx) = window.drawing_manager.hit_test_in_pane(
                            plx, ply, instance_id, &sub_viewport, &sub_price_scale,
                        ) {
                            // Set pane context so context menu actions know which pane is active.
                            if let Some(win) = self.panel_app.panel_grid.active_window_mut() {
                                win.drawing_manager.set_current_pane(Some(instance_id));
                            }
                            sub_hit = Some(prim_idx);
                            break;
                        }
                    }
                }
                sub_hit
            };
            result
        };

        if let Some(prim_idx) = primitive_hit {
            // Right-clicked on a primitive — look up info via primitive_list.
            let (display_name, is_locked, is_visible) = self.panel_app.panel_grid
                .active_window()
                .and_then(|win| {
                    win.drawing_manager.primitive_list()
                        .into_iter()
                        .find(|item| item.index == prim_idx)
                        .map(|item| (item.display_name, item.locked, item.visible))
                })
                .unwrap_or_else(|| ("Primitive".to_string(), false, true));

            let items = build_primitive_context_menu(&display_name, is_locked, is_visible);

            // Select the primitive so it highlights.
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.select_by_index(prim_idx);
            }

            self.panel_app.context_menu_state.open_smart(
                x, y,
                ContextMenuTarget::Primitive(prim_idx),
                items,
                w, h,
            );
            eprintln!("[ChartApp] Context menu opened for primitive #{}", prim_idx);
        } else {
            // Check if right-click landed on an overlay indicator line.
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;

            let indicator_hit = if local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height
            {
                self.panel_app.panel_grid.active_window()
                    .and_then(|window| {
                        self.indicator_manager.hit_test_overlay(
                            local_x,
                            local_y,
                            &window.symbol,
                            &window.viewport,
                            &window.price_scale,
                            chart_rect.height,
                            8.0,
                        )
                    })
            } else {
                None
            };

            // If no main-chart indicator hit, try sub-panes.
            let indicator_hit = if indicator_hit.is_none() {
                let mut sub_hit = None;
                for sp in &extended.sub_panes {
                    let pane_rect = sp.content;
                    let plx = x - pane_rect.x;
                    let ply = y - pane_rect.y;
                    if plx < 0.0 || plx > pane_rect.width || ply < 0.0 || ply > pane_rect.height {
                        continue;
                    }
                    let instance_id = sp.instance_id;
                    let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            w.sub_panes.iter()
                                .find(|p| p.instance_id == instance_id)
                                .map(|p| (p.price_min, p.price_max))
                        })
                        .unwrap_or((0.0, 100.0));
                    let hit = self.panel_app.panel_grid.active_window()
                        .map(|window| {
                            self.indicator_manager.hit_test_sub_pane(
                                instance_id,
                                plx, ply,
                                &window.viewport,
                                price_min, price_max,
                                pane_rect.height,
                                8.0,
                            )
                        })
                        .unwrap_or(false);
                    if hit {
                        sub_hit = Some(instance_id);
                        break;
                    }
                }
                sub_hit
            } else {
                indicator_hit
            };

            if let Some(ind_id) = indicator_hit {
                // Right-clicked on an indicator line — select it and open its settings.
                self.selected_indicator_id = Some(ind_id);
                self.panel_app.indicator_settings_state.open(ind_id);
                eprintln!("[ChartApp] Indicator right-clicked: id={}, settings opened", ind_id);
            } else {
                // Right-clicked on empty chart background — build chart background menu.
                let items = build_chart_background_menu();
                self.panel_app.context_menu_state.open_smart(
                    x, y,
                    ContextMenuTarget::ChartBackground,
                    items,
                    w, h,
                );
                eprintln!("[ChartApp] Context menu opened for chart background");
            }
        }
    }

    /// Handle a double-click at screen coordinates `(x, y)`.
    ///
    /// Resets the price scale on double-click over the price scale area,
    /// resets the time scale on double-click over the time scale area.
    /// For sub-pane price scales the sub-pane auto-scale is restored directly,
    /// since `drag_mode` is `None` during a double-click and the generic
    /// `ResetPriceScale` path in `process_output_actions` cannot determine
    /// which sub-pane to target.
    ///
    /// Toolbar widgets (toolbars, dropdowns) are checked first: if the
    /// double-click lands on one, it is forwarded to `dispatch_panel_click`
    /// so that double-click-sensitive buttons (e.g. the magnet button) can
    /// compare against the timestamp saved during the first click.
    pub fn on_double_click(&mut self, x: f64, y: f64) {
        // Check whether the double-click landed on a toolbar or dropdown widget.
        // The platform fires on_click (first click) then on_double_click (second
        // click) for a double-click sequence.  Without this check the second
        // click never reaches handle_toolbar_click_with_chart, so the
        // last_magnet_click_time comparison never fires.
        let dbl_clicked_id = self.input_coordinator.borrow_mut().process_click(x, y)
            .map(|w| w.0.clone());
        if let Some(id) = dbl_clicked_id {
            let is_toolbar = id.starts_with("toolbar:")
                || id.starts_with("dtb:")
                || id.starts_with("csb:")
                || id.starts_with("btb:")
                || id.starts_with("rtb:")
                || id.starts_with("dropdown:");
            if is_toolbar {
                eprintln!("[ChartApp] double-click on toolbar widget: {}", id);
                self.dispatch_panel_click(&id, x, y);
                return;
            }
        }

        // Double-click on watchlist column header area → reset separators to equal widths.
        if self.sidebar_state.right_panel == sidebar_content::state::RightSidebarPanel::Watchlist {
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                let sr = &sidebar_result.content_rect;
                let header_y = sr.y + 12.0; // content_padding from render.rs
                let header_h = 22.0;        // header_row_h from render.rs
                if x >= sr.x && x <= sr.x + sr.width && y >= header_y && y <= header_y + header_h {
                    self.watchlist_actions.push(crate::WatchlistAction::ResetSeparatorOffsets);
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    return;
                }
            }
        }

        let extended = self.build_extended_layout();
        let hit_tester = ExtendedLayoutHitTester::new(&extended);

        // Capture hit result before passing hit_tester into process_action.
        let hit = hit_tester.hit_test(x, y);

        let actions = self.input_handler.process_action(
            ChartInputAction::DoubleClick { x, y },
            &hit_tester,
        );
        self.process_output_actions(actions);

        // For sub-pane price scales, process_output_actions cannot route
        // correctly (drag_mode is None during double-click), so handle it here.
        if let zengeld_chart::engine::input::HitResult::SubPanePriceScale { pane_index } = hit {
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                if let Some(sub_pane) = window.sub_panes.get_mut(pane_index) {
                    sub_pane.auto_scale = true;
                }
                window.update_sub_pane_ranges();
            }
        }
    }

    // -------------------------------------------------------------------------
    // Drag
    // -------------------------------------------------------------------------

    /// Handle drag start at `(x, y)`.
    pub fn on_drag_start(&mut self, x: f64, y: f64) {
        // Track whether this drag started on a UI element (for crosshair suppression).
        self.ui_drag_active = self.input_coordinator.borrow_mut().is_over_ui();

        // Check if drag starts on the sidebar separator — if so, begin sidebar resize.
        // This must be checked BEFORE the modal guard so the separator is reachable
        // even when a sidebar panel is open (which registers the sidebar as a UI widget).
        let on_sidebar_separator = self.input_coordinator.borrow_mut().hovered_widget()
            .map(|h| h.0 == "right_sidebar_separator")
            .unwrap_or(false);
        if on_sidebar_separator {
            self.sidebar_separator_drag_active = true;
            return;
        }

        // Check if drag starts on a watchlist column separator — begin separator drag.
        // Must be checked before the watchlist row drag so separators win the hit-test.
        if self.sidebar_state.right_panel == sidebar_content::state::RightSidebarPanel::Watchlist {
            // Widget ids use 1-based indexing; strip prefix and subtract 1 for 0-based sep index.
            let on_sep = self.input_coordinator.borrow_mut().hovered_widget()
                .and_then(|h| h.0.strip_prefix("watchlist_sep_").and_then(|s| s.parse::<usize>().ok()))
                .map(|one_based| one_based.saturating_sub(1));
            if let Some(sep_idx) = on_sep {
                // Compute the current separator absolute X offset from area_left.
                let item_padding = 8.0_f64;
                let scrollbar_width = 8.0_f64;
                let content_width = self.sidebar_state.right_sidebar_width - scrollbar_width;
                let usable_w = content_width - item_padding * 2.0;
                let area_left = item_padding; // relative to sidebar left edge

                // Determine current separator offset from area_left.
                let sep_offset_at_start = {
                    let col_cfg = self.sidebar_state.watchlist_manager
                        .active_list()
                        .map(|l| &l.column_config);
                    let n_seps = col_cfg.map(|c| {
                        let mut n = 0usize; // symbol always present
                        if c.show_exchange   { n += 1; }
                        if c.show_last_price { n += 1; }
                        if c.show_change_pct { n += 1; }
                        if c.show_change_abs { n += 1; }
                        if c.show_high_low   { n += 2; }
                        if c.show_volume     { n += 1; }
                        n
                    }).unwrap_or(0);

                    let use_custom = col_cfg
                        .and_then(|c| c.separator_offsets.as_ref())
                        .map(|o| o.len() == n_seps)
                        .unwrap_or(false);

                    if use_custom {
                        col_cfg
                            .and_then(|c| c.separator_offsets.as_ref())
                            .and_then(|o| o.get(sep_idx).copied())
                            .unwrap_or(0.0)
                    } else {
                        // Default: all columns equal width.
                        let n_cols = n_seps + 1;
                        let equal_col_w = usable_w / n_cols as f64;
                        (sep_idx + 1) as f64 * equal_col_w
                    }
                };

                // Store: (0-based sep index, screen X at drag start, sep offset from area_left at drag start).
                // We also store area_left as an absolute screen X for math convenience.
                let sidebar_left_x = self.right_toolbar_left_x - self.sidebar_state.right_sidebar_width;
                let area_left_abs = sidebar_left_x + area_left;
                // Encode area_left_abs in the third slot; we compute sep offset on the fly from (x - area_left_abs).
                let _ = area_left_abs; // will use sep_offset_at_start directly
                self.sidebar_state.watchlist_sep_drag = Some((sep_idx, x, sep_offset_at_start));
                self.ui_drag_active = true;
                eprintln!("[Sidebar] Watchlist sep drag started: sep={}, x={:.0}, offset={:.0}", sep_idx, x, sep_offset_at_start);
                return;
            }
        }

        // Check if drag starts on a watchlist row — begin drag-to-reorder.
        // Must be checked before the modal guard so it works while the sidebar is open.
        if self.sidebar_state.right_panel == sidebar_content::state::RightSidebarPanel::Watchlist {
            let on_watchlist_row = self.input_coordinator.borrow_mut().hovered_widget()
                .and_then(|h| h.0.strip_prefix("watchlist_").and_then(|s| s.parse::<usize>().ok()));
            if let Some(idx) = on_watchlist_row {
                // Verify this is a plain index (not "watchlist_delete_N" etc.).
                let hovered_id = self.input_coordinator.borrow_mut().hovered_widget()
                    .map(|h| h.0.clone())
                    .unwrap_or_default();
                if hovered_id == format!("watchlist_{}", idx) {
                    self.sidebar_state.watchlist_drag_index = Some(idx);
                    self.sidebar_state.watchlist_drag_y = y;
                    self.sidebar_state.watchlist_drop_index = Some(idx);
                    self.ui_drag_active = true;
                    eprintln!("[Sidebar] Watchlist drag started: row {}", idx);
                    return;
                }
            }
        }

        // Check if drag starts inside the right sidebar content area — begin
        // drag-to-scroll.  This fires for any sidebar panel (Connectors,
        // Alerts, ObjectTree, Signals, Watchlist rows that didn't match above).
        // It must be checked BEFORE the modal guard so it works while the sidebar is open.
        if self.sidebar_state.is_right_open() && !self.ui_drag_active {
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                let sr = &sidebar_result.sidebar_rect;
                if x >= sr.x && x <= sr.x + sr.width && y >= sr.y && y <= sr.y + sr.height {
                    // Only start scroll-drag when not on the separator widget.
                    let on_sep = self.input_coordinator.borrow_mut().hovered_widget()
                        .map(|h| h.0 == "right_sidebar_separator")
                        .unwrap_or(false);
                    if !on_sep {
                        self.sidebar_state.sidebar_drag_active = true;
                        self.sidebar_state.sidebar_drag_last_y = y;
                        self.ui_drag_active = true;
                        // Do NOT return — let normal on_drag_start logic continue
                        // (e.g. the modal guard will still fire for clicks inside modals
                        // that happen to be positioned over the sidebar area).
                    }
                }
            }
        }

        // Check if drag starts on a modal title bar — if so, start modal drag
        // instead of blocking. This must be checked BEFORE the modal guard.
        if let Some(result) = &self.frame_result {
            // Primitive settings modal header
            if let Some(ref ps) = result.primitive_settings {
                if ps.header_rect.contains(x, y) && self.panel_app.primitive_settings_state.is_open() {
                    let modal_x = ps.header_rect.x;
                    let modal_y = ps.header_rect.y;
                    self.panel_app.primitive_settings_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] prim_settings modal drag started");
                    return;
                }
            }
            // Chart settings modal header
            if let Some(ref cs) = result.chart_settings {
                if cs.header_rect.contains(x, y) && self.panel_app.chart_settings_state.is_open {
                    let modal_x = cs.header_rect.x;
                    let modal_y = cs.header_rect.y;
                    self.panel_app.chart_settings_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] chart_settings modal drag started");
                    return;
                }
            }
            // User settings modal header
            if let Some(ref us) = result.user_settings {
                if us.header_rect.contains(x, y) && self.panel_app.user_settings_state.is_open {
                    let modal_x = us.header_rect.x;
                    let modal_y = us.header_rect.y;
                    self.panel_app.user_settings_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] user_settings modal drag started");
                    return;
                }
            }
            // Overlay settings modal header
            if let Some(ref os) = result.overlay_settings {
                if os.header_rect.contains(x, y) && self.panel_app.overlay_settings_state.is_open {
                    let modal_x = os.header_rect.x;
                    let modal_y = os.header_rect.y;
                    self.panel_app.overlay_settings_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] overlay_settings modal drag started");
                    return;
                }
            }
            // Tags & Tabs modal header
            if let Some(ref tt) = result.tags_tabs {
                if tt.header_rect.contains(x, y) && self.panel_app.tags_tabs_state.is_open {
                    let modal_x = tt.header_rect.x;
                    let modal_y = tt.header_rect.y;
                    self.panel_app.tags_tabs_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] tags_tabs modal drag started");
                    return;
                }
            }
            // Indicator settings modal header
            if let Some(ref is) = result.indicator_settings {
                if is.header_rect.contains(x, y) && self.panel_app.indicator_settings_state.is_open() {
                    let modal_x = is.header_rect.x;
                    let modal_y = is.header_rect.y;
                    self.panel_app.indicator_settings_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] ind_settings modal drag started");
                    return;
                }
            }
            // Alert settings modal header
            if let Some(ref as_result) = result.alert_settings {
                if as_result.header_rect.contains(x, y) && self.panel_app.alert_settings_state.is_open() {
                    let modal_x = as_result.header_rect.x;
                    let modal_y = as_result.header_rect.y;
                    self.panel_app.alert_settings_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] alert_settings modal drag started");
                    return;
                }
            }
            // Compare settings modal header
            if let Some(ref cs_result) = result.compare_settings {
                if cs_result.header_rect.contains(x, y) && self.panel_app.compare_settings_state.is_open() {
                    let modal_x = cs_result.header_rect.x;
                    let modal_y = cs_result.header_rect.y;
                    self.panel_app.compare_settings_state.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] compare_settings modal drag started");
                    return;
                }
            }
            // Compare settings slider drag start
            if self.panel_app.compare_settings_state.is_open() {
                if let Some(ref cs_result) = result.compare_settings {
                    // Dual-handle tf_*_slider tracks (Visibility tab) — check before line_width
                    let tf_slider_tracks = cs_result.tf_slider_tracks.clone();
                    let mut tf_drag_started = false;
                    for track in &tf_slider_tracks {
                        let handle_r = 6.0;
                        let hit = x >= track.track_x - handle_r
                            && x <= track.track_x + track.track_width + handle_r
                            && y >= track.track_y
                            && y <= track.track_y + track.track_height;
                        if hit {
                            let field_id = track.field_id.clone();
                            let track_x = track.track_x;
                            let track_width = track.track_width;
                            let min_val = track.min_val;
                            let max_val = track.max_val;

                            // Determine which handle (Min/Max) is closer to click position.
                            let t = ((x - track_x) / track_width).clamp(0.0, 1.0);
                            let tf_config = self.panel_app.compare_settings_state
                                .cached_timeframe_visibility.clone()
                                .unwrap_or_else(TimeframeVisibilityConfig::all);
                            let handle = if let Some(tf_idx) = field_id.strip_prefix("tf_")
                                .and_then(|s| s.strip_suffix("_slider"))
                                .and_then(|s| s.parse::<usize>().ok())
                            {
                                let (cur_min, cur_max): (u32, u32) = match tf_idx {
                                    1 => tf_config.seconds.unwrap_or((1, 59)),
                                    2 => tf_config.minutes.unwrap_or((1, 59)),
                                    3 => tf_config.hours.unwrap_or((1, 24)),
                                    4 => tf_config.days.unwrap_or((1, 366)),
                                    5 => tf_config.weeks.unwrap_or((1, 52)),
                                    6 => tf_config.months.unwrap_or((1, 12)),
                                    _ => {
                                        if t <= 0.5 { (min_val as u32, min_val as u32) }
                                        else { (max_val as u32, max_val as u32) }
                                    }
                                };
                                let min_pos = (cur_min as f64 - min_val) / (max_val - min_val);
                                let max_pos = (cur_max as f64 - min_val) / (max_val - min_val);
                                if (t - min_pos).abs() <= (t - max_pos).abs() {
                                    DualSliderHandle::Min
                                } else {
                                    DualSliderHandle::Max
                                }
                            } else {
                                if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                            };

                            self.panel_app.compare_settings_state.start_dual_slider_drag(
                                &field_id, track_x, track_width, min_val, max_val, handle, x,
                            );
                            eprintln!("[ChartApp] compare_settings tf slider drag started {:?}", handle);
                            tf_drag_started = true;
                            break;
                        }
                    }
                    if tf_drag_started {
                        return;
                    }

                    // Line-width single-handle slider (Style tab)
                    if let Some(ref track) = cs_result.line_width_slider {
                        let handle_r = 6.0;
                        // Check if drag starts within the actual slider track rect (both X and Y),
                        // not the entire modal rect, to avoid false positives elsewhere in the modal.
                        let hit = x >= track.track_x - handle_r
                            && x <= track.track_x + track.track_width + handle_r
                            && y >= track.track_y
                            && y <= track.track_y + track.track_height;
                        if hit {
                            let field_id = track.field_id.clone();
                            let track_x = track.track_x;
                            let track_width = track.track_width;
                            let min_val = track.min_val;
                            let max_val = track.max_val;
                            self.panel_app.compare_settings_state.start_slider_drag(
                                &field_id, track_x, track_width, min_val, max_val,
                            );
                            // Jump handle to click position immediately.
                            self.panel_app.compare_settings_state.update_slider_drag(x);
                            eprintln!("[ChartApp] compare_settings line_width slider drag started");
                            return;
                        }
                    }
                }
            }
            // Preset name input — text select drag on the input field
            if let Some(ref pni) = result.preset_name_input {
                if pni.input_rect.contains(x, y) && self.panel_app.preset_name_input.is_open {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                        &pni.char_x_positions,
                        x,
                    );
                    self.panel_app.preset_name_input.editing.cursor = new_cursor;
                    self.panel_app.preset_name_input.editing.selection_start = Some(new_cursor);
                    self.panel_app.preset_name_input.text_select_dragging = true;
                    eprintln!("[ChartApp] preset_name_input text select drag started at char {}", new_cursor);
                    return;
                }
            }
            // Preset name input modal header
            if let Some(ref pni) = result.preset_name_input {
                if pni.header_rect.contains(x, y) && self.panel_app.preset_name_input.is_open {
                    let modal_x = pni.modal_rect.x;
                    let modal_y = pni.modal_rect.y;
                    self.panel_app.preset_name_input.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] preset_name_input modal drag started");
                    return;
                }
            }
            // Chart browser — text select drag on the search input field
            if let Some(ref br) = result.chart_browser {
                if br.search_input_rect.contains(x, y) && self.panel_app.chart_browser.is_open {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                        &br.search_char_positions,
                        x,
                    );
                    self.panel_app.chart_browser.search_editing.cursor = new_cursor;
                    self.panel_app.chart_browser.search_editing.selection_start = Some(new_cursor);
                    self.panel_app.chart_browser.search_text_select_dragging = true;
                    eprintln!("[ChartApp] chart_browser search text select drag started at char {}", new_cursor);
                    return;
                }
            }
            // Chart browser modal header
            if let Some(ref br) = result.chart_browser {
                if br.header_rect.contains(x, y) && self.panel_app.chart_browser.is_open {
                    let modal_x = br.modal_rect.x;
                    let modal_y = br.modal_rect.y;
                    self.panel_app.chart_browser.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] chart_browser modal drag started");
                    return;
                }
            }
            // Primitive settings — text select drag on ANY active text input field
            if let Some(ref ps) = result.primitive_settings {
                if let Some(input_rect) = ps.active_input_rect {
                    if input_rect.contains(x, y)
                        && self.panel_app.primitive_settings_state.is_open()
                        && self.panel_app.primitive_settings_state.editing_text.is_some()
                    {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &ps.active_input_char_positions,
                            x,
                        );
                        if let Some(ref mut edit) = self.panel_app.primitive_settings_state.editing_text {
                            edit.cursor = new_cursor;
                            edit.selection_start = Some(new_cursor);
                        }
                        self.panel_app.primitive_settings_state.text_select_dragging = true;
                        return;
                    }
                }
            }
            // Indicator settings — text select drag on the active text input field
            if let Some(ref is) = result.indicator_settings {
                if let Some(input_rect) = is.active_input_rect {
                    if input_rect.contains(x, y)
                        && self.panel_app.indicator_settings_state.is_open()
                        && self.panel_app.indicator_settings_state.editing_text_state.is_some()
                    {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &is.active_input_char_positions,
                            x,
                        );
                        if let Some(ref mut edit) = self.panel_app.indicator_settings_state.editing_text_state {
                            edit.cursor = new_cursor;
                            edit.selection_start = Some(new_cursor);
                        }
                        self.panel_app.indicator_settings_state.text_select_dragging = true;
                        return;
                    }
                }
            }
            // Chart settings — text select drag on the active text input field
            if let Some(ref cs) = result.chart_settings {
                if let Some(input_rect) = cs.active_input_rect {
                    if input_rect.contains(x, y)
                        && self.panel_app.chart_settings_state.is_open
                        && self.panel_app.chart_settings_state.editing_text.is_some()
                    {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &cs.active_input_char_positions,
                            x,
                        );
                        if let Some(ref mut edit) = self.panel_app.chart_settings_state.editing_text {
                            edit.cursor = new_cursor;
                            edit.selection_start = Some(new_cursor);
                        }
                        self.panel_app.chart_settings_state.text_select_dragging = true;
                        return;
                    }
                }
            }
            // Compare settings — text select drag on the active tf_min/tf_max input field
            if let Some(ref cs_result) = result.compare_settings {
                if let Some(input_rect) = cs_result.tf_active_input_rect {
                    if input_rect.contains(x, y)
                        && self.panel_app.compare_settings_state.is_open()
                        && self.panel_app.compare_settings_state.editing_text.is_some()
                    {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &cs_result.tf_active_input_char_positions,
                            x,
                        );
                        if let Some(ref mut edit) = self.panel_app.compare_settings_state.editing_text {
                            edit.cursor = new_cursor;
                            edit.selection_start = Some(new_cursor);
                        }
                        self.panel_app.compare_settings_state.text_select_dragging = true;
                        return;
                    }
                }
            }
        }

        // Watchlist group name input — text select drag on the input field
        if self.wl_group_name_input.is_open() {
            if let Some(ref gni) = self.last_wl_group_name_result {
                if gni.input_rect.contains(x, y) {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                        &gni.char_x_positions,
                        x,
                    );
                    self.wl_group_name_input.editing.cursor = new_cursor;
                    self.wl_group_name_input.editing.selection_start = Some(new_cursor);
                    self.wl_group_name_input.text_select_dragging = true;
                    eprintln!("[ChartApp] wl_group_name_input text select drag started at char {}", new_cursor);
                    return;
                }
            }
        }

        // Watchlist group name input modal header drag (on top of watchlist modal).
        if self.wl_group_name_input.is_open() {
            if let Some(ref gni) = self.last_wl_group_name_result {
                if gni.header_rect.contains(x, y) {
                    let modal_x = gni.modal_rect.x;
                    let modal_y = gni.modal_rect.y;
                    self.wl_group_name_input.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] wl_group_name_input drag started");
                    return;
                }
            }
        }

        // Watchlist modal — text select drag on the search input field (Overview tab only).
        if self.watchlist_modal.is_open() {
            use zengeld_chart::ui::modal_settings::WatchlistModalTab;
            if self.watchlist_modal.active_tab == WatchlistModalTab::Overview {
                if let Some(ref wl) = self.last_watchlist_modal_result {
                    if wl.search_input_rect.contains(x, y) {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &wl.search_char_positions,
                            x,
                        );
                        self.watchlist_modal.search_editing.cursor = new_cursor;
                        self.watchlist_modal.search_editing.selection_start = Some(new_cursor);
                        self.watchlist_modal.search_text_select_dragging = true;
                        eprintln!("[ChartApp] watchlist_modal search text select drag started at char {}", new_cursor);
                        return;
                    }
                }
            }
        }

        // Watchlist modal header drag.
        if self.watchlist_modal.is_open() {
            if let Some(ref wl) = self.last_watchlist_modal_result {
                if wl.header_rect.contains(x, y) {
                    let modal_x = wl.modal_rect.x;
                    let modal_y = wl.modal_rect.y;
                    self.watchlist_modal.start_drag(x, y, modal_x, modal_y);
                    eprintln!("[ChartApp] watchlist_modal drag started");
                    return;
                }
                // Watchlist modal item row drag — begin drag-to-reorder.
                // Guard: if the pointer is over a delete button, don't start drag — let it be a click.
                // Check hovered widget AND check delete_btn_rects directly (hovered_widget can lag).
                let over_delete_hovered = self.input_coordinator.borrow_mut().hovered_widget()
                    .map(|h| h.0.starts_with("wl_modal:delete:"))
                    .unwrap_or(false);
                let over_delete_rect = wl.delete_btn_rects.iter()
                    .any(|(_sym, r)| r.contains(x, y));
                let over_delete = over_delete_hovered || over_delete_rect;
                // Guard: if the group name input modal is open on top of the watchlist
                // modal, don't start row drag — the click belongs to that modal.
                if !self.wl_group_name_input.is_open() && !over_delete && wl.list_viewport_rect.contains(x, y) {
                    // Find the row index (in the filtered/displayed list) that was
                    // clicked.  We match item_rects against (x, y) directly.
                    let mut found_idx: Option<usize> = None;
                    for (i, (_sym, item_rect)) in wl.item_rects.iter().enumerate() {
                        if item_rect.contains(x, y) {
                            found_idx = Some(i);
                            break;
                        }
                    }
                    if let Some(idx) = found_idx {
                        // Store as pending — only promote to actual drag_reorder
                        // in on_mouse_move once the pointer moves >= 5 px.
                        // This ensures short clicks reach on_click properly.
                        self.watchlist_modal.drag_reorder_pending = Some((idx, x, y));
                        eprintln!("[WatchlistModal] drag-reorder pending: row {}", idx);
                        // Do NOT return early — let the drag-start flow continue
                        // so other drag handlers (e.g. modal header) get checked.
                    }
                }
            }
        }

        // Search overlay modal (symbol / indicator / compare search) — drag-to-select in search input.
        if self.modal_state.current.is_search_overlay() {
            if let Some(ref smr) = self.search_modal_result {
                if smr.input_rect.contains(x, y) && self.modal_state.editing_text.is_some() {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                        &smr.search_char_positions,
                        x,
                    );
                    if let Some(ref mut edit) = self.modal_state.editing_text {
                        edit.cursor = new_cursor;
                        edit.selection_start = Some(new_cursor);
                    }
                    self.modal_state.text_select_dragging = true;
                    return;
                }
            }
        }
        // Search overlay modal (symbol / indicator / compare search) header drag.
        if self.modal_state.current.is_search_overlay() {
            if let Some(ref smr) = self.search_modal_result {
                if let Some(ref hdr) = smr.header_rect {
                    if hdr.contains(x, y) {
                        let modal_x = smr.modal_rect.x;
                        let modal_y = smr.modal_rect.y;
                        self.modal_state.start_drag(x, y, modal_x, modal_y);
                        eprintln!("[ChartApp] search_modal modal drag started");
                        return;
                    }
                }
            }
        }

        // === Floating inline bar drag start ===
        // Only drag by the name label — other items are clickable buttons.
        if !self.panel_app.toolbar_state.floating_inline_bar.dragging {
            if let Some(bar_rect) = self.last_inline_bar_rect {
                // Check if we're on the name label specifically
                let on_name_label = self.input_coordinator.borrow_mut().hovered_widget()
                    .map(|h| h.0 == "ilb:inline:name")
                    .unwrap_or(false);
                if on_name_label {
                    self.panel_app.toolbar_state.floating_inline_bar.start_drag(x, y, &bar_rect);
                    self.ui_drag_active = true;
                    eprintln!("[ChartApp] inline bar drag started (by label)");
                    return;
                }
            }
        }

        // === Scrollbar and Slider drag start — must come BEFORE the modal guard ===
        // These are legitimate drags that begin inside a modal area; the guard
        // would swallow them without these early-return checks.
        if let Some(result) = &self.frame_result {
            // Chart settings scrollbar / slider drag start
            if self.panel_app.chart_settings_state.is_open {
                if let Some(ref cs) = result.chart_settings {
                    if let Some(ref handle_rect) = cs.scrollbar_handle_rect {
                        // Inflate scrollbar handle ±5px horizontally for easier grab.
                        let hit = x >= handle_rect.x - 5.0 && x <= handle_rect.x + handle_rect.width + 5.0
                            && y >= handle_rect.y && y <= handle_rect.y + handle_rect.height;
                        if hit {
                            self.panel_app.chart_settings_state.scroll.start_drag(y);
                            eprintln!("[ChartApp] chart_settings scrollbar drag started");
                            return;
                        }
                    }
                    for track in &cs.slider_tracks {
                        if let Some((_, item_rect)) = cs.content_items.iter().find(|(id, _)| id == &track.field_id) {
                            // Inflate hit area ±2px horizontally for easier handle grab.
                            let hit = x >= item_rect.x - 2.0 && x <= item_rect.x + item_rect.width + 2.0
                                && y >= item_rect.y && y <= item_rect.y + item_rect.height;
                            if hit {
                                let field_id = track.field_id.clone();
                                let track_x = track.track_x;
                                let track_width = track.track_width;
                                let min_val = track.min_val;
                                let max_val = track.max_val;
                                self.panel_app.chart_settings_state.start_slider_drag_from_track(
                                    &field_id, track_x, track_width, min_val, max_val,
                                );
                                // Set initial floating value so the handle jumps to click position.
                                self.panel_app.chart_settings_state.update_slider_drag_float(x);
                                return;
                            }
                        }
                    }
                }
            }
            // Indicator settings scrollbar / slider drag start
            if self.panel_app.indicator_settings_state.is_open() {
                if let Some(ref is) = result.indicator_settings {
                    if let Some(ref handle_rect) = is.scrollbar_handle_rect {
                        // Inflate scrollbar handle ±5px horizontally for easier grab.
                        let hit = x >= handle_rect.x - 5.0 && x <= handle_rect.x + handle_rect.width + 5.0
                            && y >= handle_rect.y && y <= handle_rect.y + handle_rect.height;
                        if hit {
                            self.panel_app.indicator_settings_state.scroll.start_drag(y);
                            eprintln!("[ChartApp] ind_settings scrollbar drag started");
                            return;
                        }
                    }
                    for track in &is.slider_tracks {
                        if let Some((_, item_rect)) = is.content_items.iter().find(|(id, _)| id == &track.field_id) {
                            // Inflate hit area ±2px horizontally for easier handle grab.
                            let hit = x >= item_rect.x - 2.0 && x <= item_rect.x + item_rect.width + 2.0
                                && y >= item_rect.y && y <= item_rect.y + item_rect.height;
                            if hit {
                                let field_id = track.field_id.clone();
                                let track_x = track.track_x;
                                let track_width = track.track_width;
                                let min_val = track.min_val;
                                let max_val = track.max_val;
                                // For dual-handle sliders (tf_*_slider), determine which handle
                                // (min or max) is closest to the click position.
                                if field_id.starts_with("tf_") && field_id.ends_with("_slider") {
                                    // Get current min/max from indicator instance timeframe_visibility
                                    let t = ((x - track_x) / track_width).clamp(0.0, 1.0);
                                    let handle_and_vals = if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                                        if let Some(tf_idx) = field_id.strip_prefix("tf_")
                                            .and_then(|s| s.strip_suffix("_slider"))
                                            .and_then(|s| s.parse::<usize>().ok())
                                        {
                                            self.indicator_manager.get_instance(ind_id)
                                                .and_then(|inst| {
                                                    let tf_config = inst.timeframe_visibility.clone()
                                                        .unwrap_or_else(zengeld_chart::drawing::TimeframeVisibilityConfig::all);
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
                                                    Some((handle, cur_min as f64))
                                                })
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    };

                                    let handle = handle_and_vals.map(|(h, _)| h).unwrap_or_else(|| {
                                        // No current values available — pick handle by position:
                                        // left half → Min, right half → Max.
                                        let t = ((x - track_x) / track_width).clamp(0.0, 1.0);
                                        if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                                    });

                                    self.panel_app.indicator_settings_state.start_dual_slider_drag_from_track(
                                        &field_id, track_x, track_width, min_val, max_val, handle, x,
                                    );
                                } else {
                                    self.panel_app.indicator_settings_state.start_slider_drag_from_track(
                                        &field_id, track_x, track_width, min_val, max_val,
                                    );
                                    // Set initial floating value.
                                    self.panel_app.indicator_settings_state.update_slider_drag_float(x);
                                }
                                return;
                            }
                        }
                    }
                }
            }
            // Primitive settings slider drag start
            if self.panel_app.primitive_settings_state.is_open() {
                if let Some(ref ps) = result.primitive_settings {
                    for track in &ps.slider_tracks {
                        if let Some((_, item_rect)) = ps.content_items.iter().find(|(id, _)| id == &track.field_id) {
                            // Inflate hit area ±2px horizontally for easier handle grab.
                            let hit = x >= item_rect.x - 2.0 && x <= item_rect.x + item_rect.width + 2.0
                                && y >= item_rect.y && y <= item_rect.y + item_rect.height;
                            if hit {
                                let field_id = track.field_id.clone();
                                let track_x = track.track_x;
                                let track_width = track.track_width;
                                let min_val = track.min_val;
                                let max_val = track.max_val;
                                // For dual-handle sliders (tf_*_slider), determine which
                                // handle (min or max) is closest to the click position.
                                if field_id.starts_with("tf_") && field_id.ends_with("_slider") {
                                    let t = ((x - track_x) / track_width).clamp(0.0, 1.0);

                                    // Try to determine handle by proximity to current min/max positions.
                                    let handle = if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                                        if let Some(data) = self.panel_app.panel_grid.active_window()
                                            .and_then(|w| w.drawing_manager.get_data_at(idx))
                                        {
                                            if let Some(tf_idx) = field_id.strip_prefix("tf_")
                                                .and_then(|s| s.strip_suffix("_slider"))
                                                .and_then(|s| s.parse::<usize>().ok())
                                            {
                                                let tf_config = data.timeframe_visibility.clone()
                                                    .unwrap_or_else(TimeframeVisibilityConfig::all);

                                                let (current_min, current_max): (u32, u32) = match tf_idx {
                                                    1 => tf_config.seconds.unwrap_or((1, 59)),
                                                    2 => tf_config.minutes.unwrap_or((1, 59)),
                                                    3 => tf_config.hours.unwrap_or((1, 24)),
                                                    4 => tf_config.days.unwrap_or((1, 366)),
                                                    5 => tf_config.weeks.unwrap_or((1, 52)),
                                                    6 => tf_config.months.unwrap_or((1, 12)),
                                                    _ => {
                                                        // Unknown tf_idx — fallback to positional heuristic.
                                                        if t <= 0.5 { (min_val as u32, min_val as u32) }
                                                        else { (max_val as u32, max_val as u32) }
                                                    }
                                                };

                                                let min_pos = (current_min as f64 - min_val) / (max_val - min_val);
                                                let max_pos = (current_max as f64 - min_val) / (max_val - min_val);

                                                // If a min/max field is being edited, follow that handle.
                                                if let Some(ref edit) = self.panel_app.primitive_settings_state.editing_text {
                                                    if edit.field_id == format!("tf_{}_min", tf_idx) {
                                                        DualSliderHandle::Min
                                                    } else if edit.field_id == format!("tf_{}_max", tf_idx) {
                                                        DualSliderHandle::Max
                                                    } else if (t - min_pos).abs() <= (t - max_pos).abs() {
                                                        DualSliderHandle::Min
                                                    } else {
                                                        DualSliderHandle::Max
                                                    }
                                                } else if (t - min_pos).abs() <= (t - max_pos).abs() {
                                                    DualSliderHandle::Min
                                                } else {
                                                    DualSliderHandle::Max
                                                }
                                            } else {
                                                // Could not parse tf_idx — use positional fallback.
                                                if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                                            }
                                        } else {
                                            // No data — positional fallback.
                                            if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                                        }
                                    } else {
                                        // No primitive selected — positional fallback.
                                        if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                                    };

                                    self.panel_app.primitive_settings_state.start_dual_slider_drag_from_track(
                                        &field_id, track_x, track_width, min_val, max_val, handle, x,
                                    );
                                    return;
                                }
                                // Regular single-handle slider.
                                self.panel_app.primitive_settings_state.start_slider_drag_from_track(
                                    &field_id, track_x, track_width, min_val, max_val,
                                );
                                // Set initial floating value so handle jumps to click position.
                                self.panel_app.primitive_settings_state.update_slider_drag_float(x);
                                return;
                            }
                        }
                    }
                }
            }

            // Color picker drag start (SV square, hue bar, or opacity slider)
            if let Some(ref cp) = result.color_picker {
                let source = if self.panel_app.primitive_settings_state.is_color_picker_open() {
                    Some("primitive")
                } else if self.panel_app.indicator_settings_state.is_color_picker_open() {
                    Some("indicator")
                } else if self.panel_app.chart_settings_state.is_color_picker_open() {
                    Some("chart")
                } else {
                    None
                };

                if let (Some(src), Some(ref l2)) = (source, &cp.l2_result) {
                    let sv = &l2.sv_square_rect;
                    let hue = &l2.hue_bar_rect;
                    let opacity = &l2.opacity_slider_rect;
                    if sv.contains(x, y) {
                        self.color_picker_drag = Some(crate::ColorPickerDragState {
                            area: crate::ColorPickerDragArea::SVSquare,
                            source: src.to_string(),
                            sv_rect: (sv.x, sv.y, sv.width, sv.height),
                            hue_rect: (hue.x, hue.y, hue.width, hue.height),
                            opacity_rect: (opacity.x, opacity.y, opacity.width, opacity.height),
                        });
                        self.apply_color_picker_drag(x, y);
                        eprintln!("[ChartApp] color_picker SV drag started ({})", src);
                        return;
                    }
                    if hue.contains(x, y) {
                        self.color_picker_drag = Some(crate::ColorPickerDragState {
                            area: crate::ColorPickerDragArea::HueBar,
                            source: src.to_string(),
                            sv_rect: (sv.x, sv.y, sv.width, sv.height),
                            hue_rect: (hue.x, hue.y, hue.width, hue.height),
                            opacity_rect: (opacity.x, opacity.y, opacity.width, opacity.height),
                        });
                        self.apply_color_picker_drag(x, y);
                        eprintln!("[ChartApp] color_picker hue drag started ({})", src);
                        return;
                    }
                    if opacity.contains(x, y) {
                        self.color_picker_drag = Some(crate::ColorPickerDragState {
                            area: crate::ColorPickerDragArea::OpacitySlider,
                            source: src.to_string(),
                            sv_rect: (sv.x, sv.y, sv.width, sv.height),
                            hue_rect: (hue.x, hue.y, hue.width, hue.height),
                            opacity_rect: (opacity.x, opacity.y, opacity.width, opacity.height),
                        });
                        self.apply_color_picker_drag(x, y);
                        eprintln!("[ChartApp] color_picker L2 opacity drag started ({})", src);
                        return;
                    }
                }

                if let (Some(src), Some(ref l1)) = (source, &cp.l1_result) {
                    if let Some(ref opacity) = l1.opacity_slider_rect {
                        if opacity.contains(x, y) {
                            // For L1 opacity slider drag we use dummy sv/hue rects
                            self.color_picker_drag = Some(crate::ColorPickerDragState {
                                area: crate::ColorPickerDragArea::OpacitySlider,
                                source: src.to_string(),
                                sv_rect: (0.0, 0.0, 0.0, 0.0),
                                hue_rect: (0.0, 0.0, 0.0, 0.0),
                                opacity_rect: (opacity.x, opacity.y, opacity.width, opacity.height),
                            });
                            self.apply_color_picker_drag(x, y);
                            eprintln!("[ChartApp] color_picker L1 opacity drag started ({})", src);
                            return;
                        }
                    }
                }
            }
        }

        // Search / compare / indicator-search modal scrollbar drag start
        if self.modal_state.is_open() {
            if let Some(ref smr) = self.search_modal_result {
                if let Some(ref handle_rect) = smr.scrollbar_handle_rect {
                    // Inflate scrollbar handle ±4px horizontally for easier grab.
                    let hit = x >= handle_rect.x - 4.0 && x <= handle_rect.x + handle_rect.width + 4.0
                        && y >= handle_rect.y && y <= handle_rect.y + handle_rect.height;
                    if hit {
                        self.modal_state.scroll.start_drag(y);
                        eprintln!("[ChartApp] search_modal scrollbar drag started");
                        return;
                    }
                }
            }
        }

        // Block drags when a modal is open (and we didn't start a modal drag above).
        if self.input_coordinator.borrow_mut().is_blocked_by_modal(x, y) {
            return;
        }

        // Click-based drawing tool on canvas: mouse-press = click immediately.
        // on_click from runner (mouse-release) is ignored via guard in on_click().
        let _has_ct = self.has_click_drawing_tool();
        if !self.ui_drag_active && _has_ct {
            self.handle_canvas_click(x, y);
            return;
        }

        // Freehand tool (brush/highlighter) — start stroke on drag start
        let is_freehand = self.panel_app.panel_grid.active_window()
            .map(|w| w.drawing_manager.is_freehand_tool())
            .unwrap_or(false);
        if is_freehand {
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;
            if local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height
            {
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .map(|w| (w.price_scale.price_min, w.price_scale.price_max))
                    .unwrap_or((0.0, 1.0));
                let bar = self.panel_app.panel_grid.active_window()
                    .map(|w| w.viewport.x_to_bar_f64(local_x))
                    .unwrap_or(0.0);
                let price = price_max - (local_y / chart_rect.height) * (price_max - price_min);
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.start_freehand(bar, price);
                }
                return;
            }
        }

        // Split panel: route drag to the correct leaf or start separator drag.
        if self.panel_app.panel_grid.is_split() {
            use zengeld_chart::state::ChartInputTarget;
            let target = self.panel_app.panel_grid.resolve_input(
                x, y, self.content_rect.x, self.content_rect.y,
            );
            match target {
                ChartInputTarget::Separator { idx, orientation } => {
                    self.split_separator_drag = Some(crate::SplitSeparatorDragState {
                        separator_idx: idx,
                        orientation,
                        start_x: x,
                        start_y: y,
                    });
                    return;
                }
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => {
                    self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    // Fall through to chart engine drag handling.
                }
                ChartInputTarget::None => return,
            }
        }

        // Capture viewport BEFORE drag starts so on_drag_end can record a
        // ViewportChange command if panning or zooming occurred.
        //
        // Guard: do NOT capture when the mousedown lands on a toolbar button or
        // any other UI widget (undo/redo, drawing tools, etc.).  If we captured
        // here, the subsequent on_drag_end() would see the viewport modified by
        // the undo/redo action and push a spurious ViewportChange — creating an
        // infinite undo loop.  When a widget is hovered the drag is NOT a chart
        // pan/zoom, so there is nothing to record.
        let on_widget = self.input_coordinator.borrow_mut().hovered_widget().is_some();
        if !on_widget {
            self.viewport_before_drag = self.panel_app.panel_grid.active_window().map(|w| {
                zengeld_chart::ViewportState::new(
                    w.viewport.view_start,
                    w.viewport.bar_spacing,
                    w.price_scale.price_min,
                    w.price_scale.price_max,
                )
            });
        }

        let extended = self.build_extended_layout();
        let hit_tester = ExtendedLayoutHitTester::new(&extended);

        // Check whether the drag starts on a primitive (main chart or sub-pane)
        // or on a control point of the currently selected primitive.  If so,
        // pass the resolved DragMode so the input handler does not fall back to
        // chart-pan, and also emit the appropriate Start*Drag action so
        // process_output_actions can initialise drawing_manager.start_drag().
        let primitive_drag_mode: DragMode;
        let extra_actions: Vec<ChartOutputAction>;

        {
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;
            let in_main = local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height;

            let mut found_mode = DragMode::None;
            let mut found_extra: Vec<ChartOutputAction> = Vec::new();

            if in_main {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    // Control points have higher priority than primitive body.
                    if let Some(cp_type) = window.drawing_manager.hit_test_control_point(
                        local_x, local_y, &window.viewport, &window.price_scale,
                    ) {
                        if let Some(selected_idx) = window.drawing_manager.selected() {
                            let prim_id = window.drawing_manager.primitives()
                                .get(selected_idx)
                                .map(|p| p.data().id);
                            if let Some(id) = prim_id {
                                found_mode = DragMode::ControlPoint { primitive_id: id, point_index: 0 };
                                found_extra.push(ChartOutputAction::StartControlPointDrag {
                                    primitive_id: id,
                                    control_point: cp_type,
                                    bar: x,
                                    price: y,
                                });
                            }
                        }
                    } else if let Some(prim_idx) = window.drawing_manager.hit_test(
                        local_x, local_y, &window.viewport, &window.price_scale,
                    ) {
                        let prim_id = window.drawing_manager.primitives()
                            .get(prim_idx)
                            .map(|p| p.data().id);
                        if let Some(id) = prim_id {
                            found_mode = DragMode::Primitive { id };
                            found_extra.push(ChartOutputAction::StartPrimitiveDrag {
                                id,
                                bar: x,
                                price: y,
                            });
                        }
                    }
                }
            }

            // If no hit on main chart, check sub-panes.
            if found_mode == DragMode::None {
                'sub_pane_loop: for pane_layout in extended.sub_panes.iter() {
                    let content = pane_layout.content;
                    let plx = x - content.x;
                    let ply = y - content.y;
                    if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                        continue;
                    }
                    let instance_id = pane_layout.instance_id;
                    let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                        .and_then(|w| {
                            w.sub_panes.iter()
                                .find(|sp| sp.instance_id == instance_id)
                                .map(|sp| (sp.price_min, sp.price_max))
                        })
                        .unwrap_or((0.0, 100.0));
                    let sub_price_scale = zengeld_chart::PriceScale::new(price_min, price_max);
                    let sub_viewport = self.panel_app.panel_grid.active_window()
                        .map(|w| {
                            let mut vp = w.viewport.clone();
                            vp.chart_height = content.height;
                            vp
                        });
                    if let Some(sub_viewport) = sub_viewport {
                        if let Some(window) = self.panel_app.panel_grid.active_window() {
                            // Control points first.
                            if let Some(cp_type) = window.drawing_manager.hit_test_control_point_in_pane(
                                plx, ply, instance_id, &sub_viewport, &sub_price_scale,
                            ) {
                                if let Some(selected_idx) = window.drawing_manager.selected() {
                                    let prim_id = window.drawing_manager.primitives()
                                        .get(selected_idx)
                                        .map(|p| p.data().id);
                                    if let Some(id) = prim_id {
                                        found_mode = DragMode::ControlPoint { primitive_id: id, point_index: 0 };
                                        found_extra.push(ChartOutputAction::StartControlPointDrag {
                                            primitive_id: id,
                                            control_point: cp_type,
                                            bar: x,
                                            price: y,
                                        });
                                        break 'sub_pane_loop;
                                    }
                                }
                            } else if let Some(prim_idx) = window.drawing_manager.hit_test_in_pane(
                                plx, ply, instance_id, &sub_viewport, &sub_price_scale,
                            ) {
                                let prim_id = window.drawing_manager.primitives()
                                    .get(prim_idx)
                                    .map(|p| p.data().id);
                                if let Some(id) = prim_id {
                                    found_mode = DragMode::Primitive { id };
                                    found_extra.push(ChartOutputAction::StartPrimitiveDrag {
                                        id,
                                        bar: x,
                                        price: y,
                                    });
                                    break 'sub_pane_loop;
                                }
                            }
                        }
                    }
                }
            }

            primitive_drag_mode = found_mode;
            extra_actions = found_extra;
        }

        let drag_start_mode = if primitive_drag_mode != DragMode::None {
            primitive_drag_mode
        } else {
            DragMode::None
        };

        let mut actions = self.input_handler.process_action(
            ChartInputAction::DragStart { mode: drag_start_mode, x, y },
            &hit_tester,
        );
        // Append Start*Drag actions so process_output_actions initialises
        // drawing_manager.start_drag() with the correct coordinates.
        actions.extend(extra_actions);
        self.process_output_actions(actions);
    }

    /// Handle drag move to `(x, y)` with deltas `(dx, dy)`.
    pub fn on_drag_move(&mut self, x: f64, y: f64, dx: f64, dy: f64) {
        // If the sidebar separator drag is active, resize the sidebar.
        if self.sidebar_separator_drag_active {
            // Sidebar width = distance from mouse X to the left edge of the right toolbar.
            let new_width = self.right_toolbar_left_x - x;
            self.sidebar_state.set_right_width(new_width);
            return;
        }

        // Watchlist column-separator drag: update absolute separator offset (clip curtain model).
        if let Some((sep_idx, start_x, sep_offset_at_start)) = self.sidebar_state.watchlist_sep_drag {
            // Compute layout constants (must match render.rs exactly).
            let item_padding = 8.0_f64;
            let scrollbar_width = 8.0_f64;
            let content_width = self.sidebar_state.right_sidebar_width - scrollbar_width;
            let usable_w = content_width - item_padding * 2.0;

            // Count separators (= visible data columns).
            let n_seps = {
                let cfg = self.sidebar_state.watchlist_manager
                    .active_list()
                    .map(|l| &l.column_config);
                let mut n = 0usize;
                if let Some(c) = cfg {
                    if c.show_exchange   { n += 1; }
                    if c.show_last_price { n += 1; }
                    if c.show_change_pct { n += 1; }
                    if c.show_change_abs { n += 1; }
                    if c.show_high_low   { n += 2; }
                    if c.show_volume     { n += 1; }
                }
                n
            };

            if sep_idx < n_seps {
                let delta = x - start_x;
                let new_offset = sep_offset_at_start + delta;
                const MIN_GAP: f64 = 16.0;

                // Clamp against left neighbor (or area edge).
                let prev_limit = if sep_idx == 0 { MIN_GAP } else {
                    self.sidebar_state.watchlist_manager
                        .active_list()
                        .and_then(|l| l.column_config.separator_offsets.as_ref())
                        .and_then(|o| o.get(sep_idx - 1).copied())
                        .unwrap_or(MIN_GAP) + MIN_GAP
                };
                let clamped = new_offset.max(prev_limit).min(usable_w - MIN_GAP);

                // Initialise separator_offsets from defaults when not yet set or wrong length.
                if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                    let needs_init = list.column_config.separator_offsets
                        .as_ref()
                        .map(|o| o.len() != n_seps)
                        .unwrap_or(true);

                    if needs_init {
                        // Mirror the default layout from render.rs — all columns equal width.
                        let n_cols = n_seps + 1;
                        let equal_col_w = usable_w / n_cols as f64;
                        let offsets: Vec<f64> = (0..n_seps)
                            .map(|i| (i + 1) as f64 * equal_col_w)
                            .collect();
                        list.column_config.separator_offsets = Some(offsets);
                    }

                    if let Some(offsets) = list.column_config.separator_offsets.as_mut() {
                        if sep_idx < offsets.len() {
                            offsets[sep_idx] = clamped;
                            // Push-right chain: if this separator moved right past
                            // the next one, push all subsequent separators to maintain
                            // MIN_GAP between each pair.
                            for j in (sep_idx + 1)..offsets.len() {
                                let min_pos = offsets[j - 1] + MIN_GAP;
                                if offsets[j] < min_pos {
                                    offsets[j] = min_pos.min(usable_w - MIN_GAP * (offsets.len() - j) as f64);
                                }
                            }
                        }
                    }
                }
                self.watchlist_actions.push(crate::WatchlistAction::SetSeparatorOffset { index: sep_idx, value: clamped });
                self.watchlists_dirty = true;
            }
            return;
        }

        // Watchlist modal drag-to-reorder: promote pending drag once mouse moves >= 5px.
        if let Some((pending_idx, start_x, start_y)) = self.watchlist_modal.drag_reorder_pending {
            let dx = x - start_x;
            let dy = y - start_y;
            if dx * dx + dy * dy >= 25.0 {
                // Promote: clear pending, begin actual drag.
                self.watchlist_modal.drag_reorder_pending = None;
                self.watchlist_modal.drag_reorder = Some((pending_idx, y));
                self.watchlist_modal.drop_index = Some(pending_idx);
                self.ui_drag_active = true;
                eprintln!("[WatchlistModal] drag-reorder promoted: row {}", pending_idx);
            }
        }

        // Watchlist modal drag-to-reorder: update drop index from mouse Y.
        if self.watchlist_modal.drag_reorder.is_some() {
            // Update Y coordinate in drag state.
            if let Some((drag_idx, _)) = self.watchlist_modal.drag_reorder {
                self.watchlist_modal.drag_reorder = Some((drag_idx, y));
            }
            // Compute drop index from Y position using the modal list geometry.
            if let Some(ref wl) = self.last_watchlist_modal_result {
                // Modal list layout constants (must match render_overview_tab).
                let item_h = 28.0_f64;
                let list_top = wl.list_viewport_rect.y;
                let scroll = self.watchlist_modal.scroll_offset;
                let relative_y = y - list_top + scroll;
                let row_count = wl.item_rects.len();
                let drop = if relative_y < 0.0 {
                    0
                } else {
                    ((relative_y / item_h).round() as usize).min(row_count)
                };
                self.watchlist_modal.drop_index = Some(drop);
            }
            return;
        }

        // Watchlist drag-to-reorder: update drop index from mouse Y.
        if self.sidebar_state.watchlist_drag_index.is_some() {
            self.sidebar_state.watchlist_drag_y = y;
            // Compute drop index from Y position.
            // The watchlist content starts at header (40px) + scroll offset + padding (12px) + header_row (22px) + sep (1px).
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                let content_top = sidebar_result.content_rect.y;
                // Row height matches render (36.0 px). Header row + separator = 23.0 px.
                let watchlist_header_h = 23.0_f64;
                let data_row_h = 36.0_f64;
                let content_padding = 12.0_f64;
                let rows_start = content_top + content_padding + watchlist_header_h
                    - self.sidebar_state.current_right_scroll().offset;
                let relative_y = y - rows_start;
                let row_count = self.sidebar_state.watchlist_items.len();
                let drop = if relative_y < 0.0 {
                    0
                } else {
                    ((relative_y / data_row_h).round() as usize).min(row_count)
                };
                self.sidebar_state.watchlist_drop_index = Some(drop);
            }
            return;
        }

        // Sidebar drag-to-scroll: translate vertical mouse delta into scroll offset.
        if self.sidebar_state.sidebar_drag_active {
            let delta = y - self.sidebar_state.sidebar_drag_last_y; // positive = scroll down (content moves up)
            self.sidebar_state.sidebar_drag_last_y = y;
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                let content_h = sidebar_result.content_height;
                let viewport_h = sidebar_result.content_rect.height;
                let max_offset = (content_h - viewport_h).max(0.0);
                let scroll = self.sidebar_state.current_right_scroll_mut();
                scroll.offset = (scroll.offset + delta).clamp(0.0, max_offset);
            }
            return;
        }

        // If a split separator drag is active, update separator proportion.
        if let Some(sep_drag) = self.split_separator_drag.take() {
            use zengeld_chart::SeparatorOrientation;
            let (delta, total_size) = match sep_drag.orientation {
                SeparatorOrientation::Horizontal => {
                    let delta = (y - sep_drag.start_y) as f32;
                    let total_size = self.content_rect.height as f32;
                    (delta, total_size)
                }
                SeparatorOrientation::Vertical => {
                    let delta = (x - sep_drag.start_x) as f32;
                    let total_size = self.content_rect.width as f32;
                    (delta, total_size)
                }
            };
            self.panel_app.panel_grid.apply_separator_drag(sep_drag.separator_idx, delta, total_size);
            // Update start position for incremental delta.
            self.split_separator_drag = Some(crate::SplitSeparatorDragState {
                start_x: x,
                start_y: y,
                ..sep_drag
            });
            return;
        }

        // If any modal is being dragged, update its position and skip chart drag.
        if self.panel_app.primitive_settings_state.is_dragging {
            self.panel_app.primitive_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.chart_settings_state.is_dragging {
            self.panel_app.chart_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.overlay_settings_state.is_dragging {
            self.panel_app.overlay_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.tags_tabs_state.is_dragging {
            self.panel_app.tags_tabs_state.update_drag(x, y);
            return;
        }
        if self.panel_app.indicator_settings_state.is_dragging {
            self.panel_app.indicator_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.alert_settings_state.is_dragging {
            self.panel_app.alert_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.compare_settings_state.is_dragging {
            self.panel_app.compare_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.user_settings_state.is_dragging {
            self.panel_app.user_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.preset_name_input.text_select_dragging {
            // Update text selection cursor from mouse X position.
            if let Some(ref pni) = self.frame_result.as_ref().and_then(|r| r.preset_name_input.as_ref()) {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &pni.char_x_positions,
                    x,
                );
                self.panel_app.preset_name_input.editing.cursor = new_cursor;
                // selection_start stays as the anchor set during drag_start
            }
            return;
        }
        if self.panel_app.preset_name_input.is_dragging {
            self.panel_app.preset_name_input.update_drag(x, y);
            return;
        }
        if self.panel_app.chart_browser.search_text_select_dragging {
            // Update search text selection cursor from mouse X position.
            if let Some(ref br) = self.frame_result.as_ref().and_then(|r| r.chart_browser.as_ref()) {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &br.search_char_positions,
                    x,
                );
                self.panel_app.chart_browser.search_editing.cursor = new_cursor;
                // selection_start stays as the anchor set during drag_start
            }
            return;
        }
        if self.panel_app.chart_browser.is_dragging {
            self.panel_app.chart_browser.update_drag(x, y);
            return;
        }
        if self.watchlist_modal.search_text_select_dragging {
            // Update search text selection cursor from mouse X position.
            if let Some(ref wl) = self.last_watchlist_modal_result {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &wl.search_char_positions,
                    x,
                );
                self.watchlist_modal.search_editing.cursor = new_cursor;
                // selection_start stays as the anchor set during drag_start
            }
            return;
        }
        if self.watchlist_modal.is_dragging {
            self.watchlist_modal.update_drag(x, y);
            return;
        }
        if self.wl_group_name_input.text_select_dragging {
            // Update text selection cursor from mouse X position.
            if let Some(ref gni) = self.last_wl_group_name_result {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &gni.char_x_positions,
                    x,
                );
                self.wl_group_name_input.editing.cursor = new_cursor;
                // selection_start stays as the anchor set during drag_start
            }
            return;
        }
        if self.panel_app.primitive_settings_state.text_select_dragging {
            // Update text selection cursor from mouse X position.
            let char_positions: Vec<f64> = self.frame_result.as_ref()
                .and_then(|r| r.primitive_settings.as_ref())
                .map(|ps| ps.active_input_char_positions.clone())
                .unwrap_or_default();
            if !char_positions.is_empty() {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &char_positions,
                    x,
                );
                if let Some(ref mut edit) = self.panel_app.primitive_settings_state.editing_text {
                    edit.cursor = new_cursor;
                    // selection_start stays as the anchor set during drag_start
                }
            }
            return;
        }
        if self.panel_app.indicator_settings_state.text_select_dragging {
            // Update text selection cursor from mouse X position.
            let char_positions: Vec<f64> = self.frame_result.as_ref()
                .and_then(|r| r.indicator_settings.as_ref())
                .map(|is| is.active_input_char_positions.clone())
                .unwrap_or_default();
            if !char_positions.is_empty() {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &char_positions,
                    x,
                );
                if let Some(ref mut edit) = self.panel_app.indicator_settings_state.editing_text_state {
                    edit.cursor = new_cursor;
                    // selection_start stays as the anchor set during drag_start
                }
            }
            return;
        }
        if self.panel_app.chart_settings_state.text_select_dragging {
            // Update text selection cursor from mouse X position.
            let char_positions: Vec<f64> = self.frame_result.as_ref()
                .and_then(|r| r.chart_settings.as_ref())
                .map(|cs| cs.active_input_char_positions.clone())
                .unwrap_or_default();
            if !char_positions.is_empty() {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &char_positions,
                    x,
                );
                if let Some(ref mut edit) = self.panel_app.chart_settings_state.editing_text {
                    edit.cursor = new_cursor;
                    // selection_start stays as the anchor set during drag_start
                }
            }
            return;
        }
        if self.panel_app.compare_settings_state.text_select_dragging {
            // Update text selection cursor from mouse X position.
            let char_positions: Vec<f64> = self.frame_result.as_ref()
                .and_then(|r| r.compare_settings.as_ref())
                .map(|cs| cs.tf_active_input_char_positions.clone())
                .unwrap_or_default();
            if !char_positions.is_empty() {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &char_positions,
                    x,
                );
                if let Some(ref mut edit) = self.panel_app.compare_settings_state.editing_text {
                    edit.cursor = new_cursor;
                    // selection_start stays as the anchor set during drag_start
                }
            }
            return;
        }
        if self.wl_group_name_input.is_dragging {
            self.wl_group_name_input.update_drag(x, y);
            return;
        }

        // === Search overlay modal search input text select drag move ===
        if self.modal_state.text_select_dragging {
            if let Some(ref smr) = self.search_modal_result {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                    &smr.search_char_positions,
                    x,
                );
                if let Some(ref mut edit) = self.modal_state.editing_text {
                    edit.cursor = new_cursor;
                    // selection_start stays as the anchor set during drag_start
                }
            }
            return;
        }
        // === Search overlay modal drag move ===
        if self.modal_state.is_dragging {
            self.modal_state.update_drag(x, y);
            return;
        }

        // === Floating inline bar drag move ===
        if self.panel_app.toolbar_state.floating_inline_bar.dragging {
            let panel_layout = zengeld_chart::ChartPanelLayout::compute(
                &zengeld_chart::LayoutRect::new(0.0, 0.0, self.width as f64, self.height as f64),
                &self.panel_app.toolbar_config,
            );
            let bar_width = self.last_inline_bar_rect.map(|r| r.width).unwrap_or(400.0);
            let sidebar_w = self.sidebar_state.right_width();
            self.panel_app.toolbar_state.floating_inline_bar.update_drag(
                x, y,
                &panel_layout,
                bar_width,
                sidebar_w,
            );
            return;
        }

        // === Scrollbar drag move ===
        if self.panel_app.chart_settings_state.scroll.is_dragging {
            if let Some(ref result) = self.frame_result {
                if let Some(ref cs) = result.chart_settings {
                    if let Some(ref track_rect) = cs.scrollbar_track_rect {
                        self.panel_app.chart_settings_state.scroll.handle_drag(
                            y,
                            track_rect.height,
                            cs.total_content_height,
                            cs.viewport_height,
                        );
                    }
                }
            }
            return;
        }
        if self.panel_app.indicator_settings_state.scroll.is_dragging {
            if let Some(ref result) = self.frame_result {
                if let Some(ref is) = result.indicator_settings {
                    if let Some(ref track_rect) = is.scrollbar_track_rect {
                        self.panel_app.indicator_settings_state.scroll.handle_drag(
                            y,
                            track_rect.height,
                            is.total_content_height,
                            is.viewport_height,
                        );
                    }
                }
            }
            return;
        }

        // === Slider drag move — update floating value only, no permanent state write ===
        if self.panel_app.primitive_settings_state.is_slider_dragging() {
            self.panel_app.primitive_settings_state.update_slider_drag_float(x);
            return;
        }
        if self.panel_app.chart_settings_state.is_slider_dragging() {
            self.panel_app.chart_settings_state.update_slider_drag_float(x);
            return;
        }
        if self.panel_app.indicator_settings_state.is_slider_dragging() {
            self.panel_app.indicator_settings_state.update_slider_drag_float(x);
            return;
        }
        if self.panel_app.compare_settings_state.is_slider_dragging() {
            if self.panel_app.compare_settings_state.dual_slider_handle().is_some() {
                // tf_*_slider dual-handle drag — update preview and write live to series
                if let Some((field_id, value, Some(handle))) = self.panel_app.compare_settings_state.update_dual_slider_drag(x) {
                    let field_id = field_id.to_string();
                    self.apply_cmp_dual_slider_value(&field_id, value.round() as u32, handle);
                }
            } else {
                // line_width single-handle drag
                if let Some((_, value)) = self.panel_app.compare_settings_state.update_slider_drag(x) {
                    let idx = self.panel_app.compare_settings_state.series_index;
                    self.panel_app.compare_settings_state.cached_line_width = value as f32;
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.compare_overlay.set_series_line_width_by_index(idx, value as f32);
                    }
                }
            }
            return;
        }

        // === Color picker L2 drag move ===
        if self.color_picker_drag.is_some() {
            self.apply_color_picker_drag(x, y);
            return;
        }

        // === Search modal scrollbar drag move ===
        if self.modal_state.scroll.is_dragging {
            if let Some(ref smr) = self.search_modal_result {
                if let Some(ref track_rect) = smr.scrollbar_track_rect {
                    self.modal_state.scroll.handle_drag(
                        y,
                        track_rect.height,
                        smr.total_content_height,
                        smr.viewport_height,
                    );
                }
            }
            return;
        }

        // If drag started on any UI element but no specific drag handler claimed it,
        // suppress crosshair and skip chart drag processing.
        if self.ui_drag_active {
            self.hide_crosshair();
            return;
        }

        // PRIORITY: Freehand drawing (brush/highlighter) — add points during drag
        let is_freehand_drawing = self.panel_app.panel_grid.active_window()
            .map(|w| w.drawing_manager.is_drawing() && w.drawing_manager.is_freehand_tool())
            .unwrap_or(false);
        if is_freehand_drawing {
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;
            let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                .map(|w| (w.price_scale.price_min, w.price_scale.price_max))
                .unwrap_or((0.0, 1.0));
            let bar = self.panel_app.panel_grid.active_window()
                .map(|w| w.viewport.x_to_bar_f64(local_x))
                .unwrap_or(0.0);
            let price = price_max - (local_y / chart_rect.height) * (price_max - price_min);
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.add_freehand_point(bar, price);
            }
            return;
        }

        let drag_mode = self.input_handler.state.drag_mode;
        let extended = self.build_extended_layout();
        let hit_tester = ExtendedLayoutHitTester::new(&extended);
        let actions = self.input_handler.process_action(
            ChartInputAction::DragMove { mode: drag_mode, x, y, delta_x: dx, delta_y: dy },
            &hit_tester,
        );
        self.process_output_actions(actions);

        // Always update crosshair from global coordinates during drag.
        // process_output_actions handles UpdateCrosshair for main chart only;
        // update_crosshair_from_global correctly detects sub-pane areas and
        // sets pane_index + sub-pane price range.
        //
        // Lock the crosshair to the originating pane so it cannot jump to a
        // different coordinate system when the cursor leaves that pane's rect.
        let extended2 = self.build_extended_layout();
        // For Primitive/ControlPoint drag modes, look up the primitive's pane_id
        // so the crosshair locks to the correct sub-pane rather than always the
        // main chart.
        let primitive_drag_pane: Option<Option<usize>> =
            match self.input_handler.state.drag_mode {
                zengeld_chart::engine::input::DragMode::Primitive { id } => {
                    let pane_id = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.drawing_manager.primitives().iter()
                            .find(|p| p.data().id == id)
                            .and_then(|p| p.data().pane_id));
                    let pane_index = pane_id.and_then(|instance_id| {
                        extended2.sub_panes.iter().position(|sp| sp.instance_id == instance_id)
                    });
                    match pane_index {
                        Some(idx) => Some(Some(idx)),
                        None => Some(None),
                    }
                }
                zengeld_chart::engine::input::DragMode::ControlPoint { primitive_id, .. } => {
                    let pane_id = self.panel_app.panel_grid.active_window()
                        .and_then(|w| w.drawing_manager.primitives().iter()
                            .find(|p| p.data().id == primitive_id)
                            .and_then(|p| p.data().pane_id));
                    let pane_index = pane_id.and_then(|instance_id| {
                        extended2.sub_panes.iter().position(|sp| sp.instance_id == instance_id)
                    });
                    match pane_index {
                        Some(idx) => Some(Some(idx)),
                        None => Some(None),
                    }
                }
                _ => None,
            };
        let drag_pane = match self.input_handler.state.drag_mode {
            zengeld_chart::engine::input::DragMode::SubPaneChart { pane_index }
            | zengeld_chart::engine::input::DragMode::SubPanePriceScale { pane_index } => {
                Some(Some(pane_index))
            }
            zengeld_chart::engine::input::DragMode::Chart
            | zengeld_chart::engine::input::DragMode::PriceScale
            | zengeld_chart::engine::input::DragMode::TimeScale => {
                Some(None) // locked to main chart
            }
            zengeld_chart::engine::input::DragMode::Primitive { .. }
            | zengeld_chart::engine::input::DragMode::ControlPoint { .. } => {
                primitive_drag_pane
            }
            zengeld_chart::engine::input::DragMode::PaneSeparator { .. }
            | zengeld_chart::engine::input::DragMode::Selection
            | zengeld_chart::engine::input::DragMode::None => {
                None // no crosshair lock — hover mode
            }
        };
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.update_crosshair_from_global(x, y, &extended2, drag_pane);
        }
        // Propagate crosshair to sync group peers during drag.
        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
        if let Some(active_leaf) = active_leaf_opt {
            let (bar_f64, price, crosshair_visible, pane_index) = self.panel_app
                .panel_grid
                .active_window()
                .map(|w| (w.crosshair.bar_f64, w.crosshair.price, w.crosshair.visible, w.crosshair.pane_index))
                .unwrap_or((0.0, 0.0, false, None));
            self.propagate_crosshair_to_sync_group(active_leaf, bar_f64, price, crosshair_visible, pane_index);
        }
    }

    /// Handle drag end at `(x, y)`.
    pub fn on_drag_end(&mut self, x: f64, y: f64) {
        self.ui_drag_active = false;

        // End sidebar separator drag.
        if self.sidebar_separator_drag_active {
            self.sidebar_separator_drag_active = false;
            self.persist_profile();
            eprintln!("[ChartApp] Sidebar width: {:.0}", self.sidebar_state.right_width());
            return;
        }

        // End watchlist column-separator drag.
        if self.sidebar_state.watchlist_sep_drag.is_some() {
            self.sidebar_state.watchlist_sep_drag = None;
            self.persist_watchlists();
            eprintln!("[ChartApp] Watchlist sep drag ended");
            return;
        }

        // End watchlist modal drag-to-reorder: clear any pending drag that never promoted.
        self.watchlist_modal.drag_reorder_pending = None;

        // End watchlist modal drag-to-reorder: apply the reorder then clear drag state.
        if let Some((from_idx, _)) = self.watchlist_modal.drag_reorder.take() {
            let to_idx = self.watchlist_modal.drop_index.take().unwrap_or(from_idx);
            if from_idx != to_idx {
                // Clamp to the ungrouped length to avoid panics.
                let list_len = self.sidebar_state.watchlist_manager
                    .active_list()
                    .map(|l| l.ungrouped.len())
                    .unwrap_or(0);
                let clamped_to = to_idx.min(list_len.saturating_sub(1));
                if from_idx != clamped_to {
                    self.sidebar_state.watchlist_manager.reorder_symbol(from_idx, clamped_to);
                    self.watchlist_actions.push(crate::WatchlistAction::Reorder { from_idx, to_idx: clamped_to });
                    self.watchlist_actions.push(crate::WatchlistAction::ClearOrderSnapshot);
                    self.watchlist_actions.push(crate::WatchlistAction::ResetSort);
                    self.sidebar_state.watchlist_sort_mode = 0;
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[WatchlistModal] reorder: {} -> {}", from_idx, clamped_to);
                    // Manual reorder invalidates any saved sort snapshot — the new
                    // order becomes the "original" and sort mode resets to unsorted.
                    if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                        list.order_snapshot = None;
                    }
                }
            }
            return;
        }

        // End watchlist drag-to-reorder: apply the reorder then clear drag state.
        if let Some(drag_idx) = self.sidebar_state.watchlist_drag_index.take() {
            let drop_idx = self.sidebar_state.watchlist_drop_index.take().unwrap_or(drag_idx);
            self.sidebar_state.watchlist_drag_y = 0.0;

            // Clamp drop_idx to the ungrouped length to avoid panics.
            let list_len = self.sidebar_state.watchlist_manager
                .active_list()
                .map(|l| l.ungrouped.len())
                .unwrap_or(0);
            let clamped_drop = drop_idx.min(list_len.saturating_sub(1));

            if drag_idx != clamped_drop {
                self.sidebar_state.watchlist_manager.reorder_symbol(drag_idx, clamped_drop);
                self.watchlist_actions.push(crate::WatchlistAction::Reorder { from_idx: drag_idx, to_idx: clamped_drop });
                self.watchlist_actions.push(crate::WatchlistAction::ClearOrderSnapshot);
                self.watchlist_actions.push(crate::WatchlistAction::ResetSort);
                self.sidebar_state.watchlist_sort_mode = 0;
                self.watchlists_dirty = true;
                self.persist_watchlists();
                eprintln!("[Sidebar] Watchlist reorder: {} -> {}", drag_idx, clamped_drop);
                // Manual reorder invalidates any saved sort snapshot — the new
                // order becomes the "original" and sort mode resets to unsorted.
                if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                    list.order_snapshot = None;
                }
            }
            return;
        }

        // End sidebar drag-to-scroll.
        if self.sidebar_state.sidebar_drag_active {
            self.sidebar_state.sidebar_drag_active = false;
            self.sidebar_state.sidebar_drag_last_y = 0.0;
            // Do NOT return — other end-drag cleanup below may still be needed.
        }

        // End any in-progress separator drag.
        if self.split_separator_drag.is_some() {
            self.split_separator_drag = None;
            // Save the new panel split ratio so it survives a restart.
            self.autosave_snapshot();
            return;
        }

        // End any in-progress modal drag.
        if self.panel_app.primitive_settings_state.is_dragging {
            self.panel_app.primitive_settings_state.end_drag();
            eprintln!("[ChartApp] prim_settings modal drag ended");
            return;
        }
        if self.panel_app.chart_settings_state.is_dragging {
            self.panel_app.chart_settings_state.end_drag();
            eprintln!("[ChartApp] chart_settings modal drag ended");
            return;
        }
        if self.panel_app.overlay_settings_state.is_dragging {
            self.panel_app.overlay_settings_state.end_drag();
            eprintln!("[ChartApp] overlay_settings modal drag ended");
            return;
        }
        if self.panel_app.tags_tabs_state.is_dragging {
            self.panel_app.tags_tabs_state.end_drag();
            eprintln!("[ChartApp] tags_tabs modal drag ended");
            return;
        }
        if self.panel_app.indicator_settings_state.is_dragging {
            self.panel_app.indicator_settings_state.end_drag();
            eprintln!("[ChartApp] ind_settings modal drag ended");
            return;
        }
        if self.panel_app.alert_settings_state.is_dragging {
            self.panel_app.alert_settings_state.end_drag();
            eprintln!("[ChartApp] alert_settings modal drag ended");
            return;
        }
        if self.panel_app.compare_settings_state.is_dragging {
            self.panel_app.compare_settings_state.end_drag();
            eprintln!("[ChartApp] compare_settings modal drag ended");
            return;
        }
        if self.panel_app.user_settings_state.is_dragging {
            self.panel_app.user_settings_state.end_drag();
            eprintln!("[ChartApp] user_settings modal drag ended");
            return;
        }
        if self.panel_app.preset_name_input.text_select_dragging {
            // Finalize text selection: clear if anchor == cursor (no movement = plain click that
            // the platform classified as a drag due to minor movement jitter).
            self.panel_app.preset_name_input.text_select_dragging = false;
            let anchor = self.panel_app.preset_name_input.editing.selection_start;
            let cursor = self.panel_app.preset_name_input.editing.cursor;
            if anchor == Some(cursor) {
                self.panel_app.preset_name_input.editing.selection_start = None;
            }
            eprintln!("[ChartApp] preset_name_input text select drag ended");
            return;
        }
        if self.panel_app.preset_name_input.is_dragging {
            self.panel_app.preset_name_input.end_drag();
            eprintln!("[ChartApp] preset_name_input modal drag ended");
            return;
        }
        if self.panel_app.chart_browser.search_text_select_dragging {
            // Finalize search text selection: clear if anchor == cursor (plain click jitter).
            self.panel_app.chart_browser.search_text_select_dragging = false;
            let anchor = self.panel_app.chart_browser.search_editing.selection_start;
            let cursor = self.panel_app.chart_browser.search_editing.cursor;
            if anchor == Some(cursor) {
                self.panel_app.chart_browser.search_editing.selection_start = None;
            }
            eprintln!("[ChartApp] chart_browser search text select drag ended");
            return;
        }
        if self.panel_app.chart_browser.is_dragging {
            self.panel_app.chart_browser.end_drag();
            eprintln!("[ChartApp] chart_browser modal drag ended");
            return;
        }
        if self.watchlist_modal.search_text_select_dragging {
            // Finalize search text selection: clear if anchor == cursor (plain click jitter).
            self.watchlist_modal.search_text_select_dragging = false;
            let anchor = self.watchlist_modal.search_editing.selection_start;
            let cursor = self.watchlist_modal.search_editing.cursor;
            if anchor == Some(cursor) {
                self.watchlist_modal.search_editing.selection_start = None;
            }
            eprintln!("[ChartApp] watchlist_modal search text select drag ended");
            return;
        }
        if self.watchlist_modal.is_dragging {
            self.watchlist_modal.end_drag();
            eprintln!("[ChartApp] watchlist_modal drag ended");
            return;
        }
        if self.wl_group_name_input.text_select_dragging {
            // Finalize text selection: clear if anchor == cursor (plain click jitter).
            self.wl_group_name_input.text_select_dragging = false;
            let anchor = self.wl_group_name_input.editing.selection_start;
            let cursor = self.wl_group_name_input.editing.cursor;
            if anchor == Some(cursor) {
                self.wl_group_name_input.editing.selection_start = None;
            }
            eprintln!("[ChartApp] wl_group_name_input text select drag ended");
            return;
        }
        if self.panel_app.primitive_settings_state.text_select_dragging {
            // Finalize text selection: clear if anchor == cursor (plain click jitter).
            self.panel_app.primitive_settings_state.text_select_dragging = false;
            let (anchor, cursor) = self.panel_app.primitive_settings_state.editing_text
                .as_ref()
                .map(|e| (e.selection_start, e.cursor))
                .unwrap_or((None, 0));
            if anchor == Some(cursor) {
                if let Some(ref mut edit) = self.panel_app.primitive_settings_state.editing_text {
                    edit.selection_start = None;
                }
            }
            return;
        }
        if self.panel_app.indicator_settings_state.text_select_dragging {
            // Finalize text selection: clear if anchor == cursor (plain click jitter).
            self.panel_app.indicator_settings_state.text_select_dragging = false;
            let (anchor, cursor) = self.panel_app.indicator_settings_state.editing_text_state
                .as_ref()
                .map(|e| (e.selection_start, e.cursor))
                .unwrap_or((None, 0));
            if anchor == Some(cursor) {
                if let Some(ref mut edit) = self.panel_app.indicator_settings_state.editing_text_state {
                    edit.selection_start = None;
                }
            }
            return;
        }
        if self.panel_app.chart_settings_state.text_select_dragging {
            // Finalize text selection: clear if anchor == cursor (plain click jitter).
            self.panel_app.chart_settings_state.text_select_dragging = false;
            let (anchor, cursor) = self.panel_app.chart_settings_state.editing_text
                .as_ref()
                .map(|e| (e.selection_start, e.cursor))
                .unwrap_or((None, 0));
            if anchor == Some(cursor) {
                if let Some(ref mut edit) = self.panel_app.chart_settings_state.editing_text {
                    edit.selection_start = None;
                }
            }
            return;
        }
        if self.panel_app.compare_settings_state.text_select_dragging {
            // Finalize text selection: clear if anchor == cursor (plain click jitter).
            self.panel_app.compare_settings_state.text_select_dragging = false;
            let (anchor, cursor) = self.panel_app.compare_settings_state.editing_text
                .as_ref()
                .map(|e| (e.selection_start, e.cursor))
                .unwrap_or((None, 0));
            if anchor == Some(cursor) {
                if let Some(ref mut edit) = self.panel_app.compare_settings_state.editing_text {
                    edit.selection_start = None;
                }
            }
            return;
        }
        if self.wl_group_name_input.is_dragging {
            self.wl_group_name_input.end_drag();
            eprintln!("[ChartApp] wl_group_name_input modal drag ended");
            return;
        }

        // === Search overlay modal search input text select drag end ===
        if self.modal_state.text_select_dragging {
            self.modal_state.text_select_dragging = false;
            let anchor = self.modal_state.editing_text.as_ref().and_then(|e| e.selection_start);
            let cursor = self.modal_state.editing_text.as_ref().map(|e| e.cursor).unwrap_or(0);
            if anchor == Some(cursor) {
                if let Some(ref mut edit) = self.modal_state.editing_text {
                    edit.selection_start = None;
                }
            }
            return;
        }
        // === Search overlay modal drag end ===
        if self.modal_state.is_dragging {
            self.modal_state.end_drag();
            eprintln!("[ChartApp] search_modal modal drag ended");
            return;
        }

        // === Floating inline bar drag end ===
        if self.panel_app.toolbar_state.floating_inline_bar.dragging {
            self.panel_app.toolbar_state.floating_inline_bar.end_drag();
            eprintln!("[ChartApp] inline bar drag ended");
            self.persist_profile();
            return;
        }

        // === Scrollbar drag end ===
        if self.panel_app.chart_settings_state.scroll.is_dragging {
            self.panel_app.chart_settings_state.scroll.end_drag();
            eprintln!("[ChartApp] chart_settings scrollbar drag ended");
            return;
        }
        if self.panel_app.indicator_settings_state.scroll.is_dragging {
            self.panel_app.indicator_settings_state.scroll.end_drag();
            eprintln!("[ChartApp] ind_settings scrollbar drag ended");
            return;
        }

        // === Slider drag end — apply final floating value once ===
        if self.panel_app.primitive_settings_state.is_slider_dragging() {
            if let Some((field_id, value, dual_handle)) = self.panel_app.primitive_settings_state.take_slider_drag_value() {
                if let Some(handle) = dual_handle {
                    self.apply_dual_slider_value(&field_id, value.round() as u32, handle);
                } else {
                    self.apply_slider_value(&field_id, value);
                }
            } else {
                // No floating value (e.g. drag without move) — clear state.
                self.panel_app.primitive_settings_state.end_slider_drag();
            }
            return;
        }
        if self.panel_app.chart_settings_state.is_slider_dragging() {
            if let Some((field_id, value, _dual_handle)) = self.panel_app.chart_settings_state.take_slider_drag_value() {
                self.apply_slider_value(&field_id, value);
            } else {
                self.panel_app.chart_settings_state.end_slider_drag();
            }
            return;
        }
        if self.panel_app.indicator_settings_state.is_slider_dragging() {
            if let Some((field_id, value, dual_handle)) = self.panel_app.indicator_settings_state.take_slider_drag_value() {
                if let Some(handle) = dual_handle {
                    // tf_*_slider in indicator settings — write to indicator instance
                    self.apply_ind_dual_slider_value(&field_id, value.round() as u32, handle);
                } else {
                    self.apply_slider_value(&field_id, value);
                }
            } else {
                self.panel_app.indicator_settings_state.end_slider_drag();
            }
            return;
        }
        if self.panel_app.compare_settings_state.is_slider_dragging() {
            if let Some((field_id, value, dual_handle)) = self.panel_app.compare_settings_state.take_slider_drag_value() {
                if let Some(handle) = dual_handle {
                    // tf_*_slider dual-handle — write to compare series timeframe_visibility
                    self.apply_cmp_dual_slider_value(&field_id, value.round() as u32, handle);
                } else {
                    // line_width slider — write to compare series line width
                    let idx = self.panel_app.compare_settings_state.series_index;
                    let width = value as f32;
                    self.panel_app.compare_settings_state.cached_line_width = width;
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.compare_overlay.set_series_line_width_by_index(idx, width);
                    }
                    self.autosave_snapshot();
                    eprintln!("[ChartApp] cmp_settings line_width committed: {}", width);
                }
            } else {
                self.panel_app.compare_settings_state.end_slider_drag();
            }
            return;
        }

        // === Color picker L2 drag end ===
        if self.color_picker_drag.is_some() {
            self.color_picker_drag = None;
            eprintln!("[ChartApp] color_picker drag ended");
            return;
        }

        // === Search modal scrollbar drag end ===
        if self.modal_state.scroll.is_dragging {
            self.modal_state.scroll.end_drag();
            eprintln!("[ChartApp] search_modal scrollbar drag ended");
            return;
        }

        // Freehand drawing (brush/highlighter) — complete stroke on drag end
        let is_freehand_drawing = self.panel_app.panel_grid.active_window()
            .map(|w| w.drawing_manager.is_drawing() && w.drawing_manager.is_freehand_tool())
            .unwrap_or(false);
        if is_freehand_drawing {
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.complete_freehand();
            }
            // For grouped windows: move the completed freehand primitive to TagManager.
            // For standalone windows: record undo and propagate to color-tag peers.
            if !self.intercept_completed_primitive_to_group() {
                // Standalone path.
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(idx) = window.drawing_manager.last_index() {
                        if let (Some(type_id), Some(points), Some(data)) = (
                            window.drawing_manager.get_type_id_at(idx),
                            window.drawing_manager.get_points_at(idx),
                            window.drawing_manager.get_data_at(idx),
                        ) {
                            self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                                index: idx, type_id, points, data,
                            });
                        }
                    }
                }
                // Propagate completed freehand primitive to sync group peers.
                if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    self.propagate_new_primitive_to_sync_group(active_leaf);
                }
            } else {
                // Grouped path — record undo for the freehand primitive placed into the group.
                let group_create_cmd = self.panel_app.panel_grid.active_window()
                    .and_then(|w| w.group_id)
                    .and_then(|gid| {
                        let group = self.panel_app.tag_manager.group(gid)?;
                        let idx = group.primitives.len().saturating_sub(1);
                        let prim = group.primitives.get(idx)?;
                        Some(zengeld_chart::Command::CreatePrimitive {
                            index: idx,
                            type_id: prim.type_id().to_string(),
                            points: prim.points().to_vec(),
                            data: prim.data().clone(),
                        })
                    });
                if let Some(cmd) = group_create_cmd {
                    self.push_undo_command(cmd);
                    eprintln!("[ChartApp] Recorded CreatePrimitive (freehand, grouped) via group.primitives");
                }
            }
            // Save after both grouped and standalone paths — intercept may have moved the
            // primitive to TagManager, so the snapshot must be taken after that transfer.
            self.autosave_snapshot();
            return;
        }

        let drag_mode = self.input_handler.state.drag_mode;
        let extended = self.build_extended_layout();
        let hit_tester = ExtendedLayoutHitTester::new(&extended);
        let actions = self.input_handler.process_action(
            ChartInputAction::DragEnd { mode: drag_mode, x, y },
            &hit_tester,
        );
        self.process_output_actions(actions);

        // Record ViewportChange if the viewport moved during this drag.
        // We take() so the Option is reset for the next drag regardless.
        if let Some(previous) = self.viewport_before_drag.take() {
            if let Some(window) = self.panel_app.panel_grid.active_window() {
                let new_state = zengeld_chart::ViewportState::new(
                    window.viewport.view_start,
                    window.viewport.bar_spacing,
                    window.price_scale.price_min,
                    window.price_scale.price_max,
                );
                let changed = (new_state.view_start - previous.view_start).abs() > 0.001
                    || (new_state.bar_spacing - previous.bar_spacing).abs() > 0.001
                    || (new_state.price_min - previous.price_min).abs() > 0.001
                    || (new_state.price_max - previous.price_max).abs() > 0.001;
                // Copy new_state so we can drop the immutable borrow on window
                // before taking the mutable borrow required to push to history.
                let new_state_copy = new_state;
                // Explicitly end the immutable borrow scope.
                let _ = window;
                if changed {
                    self.push_undo_command(zengeld_chart::Command::ViewportChange {
                        previous,
                        new: new_state_copy,
                    });
                    eprintln!("[ChartApp] Recorded ViewportChange");
                }
            } else {
                // No active window — just discard the saved state (take() already done).
            }
        }
    }

    // -------------------------------------------------------------------------
    // Mouse move / leave
    // -------------------------------------------------------------------------

    /// Handle mouse move to `(x, y)`.
    ///
    /// Updates the crosshair and toolbar/modal hover states for the next render.
    pub fn on_mouse_move(&mut self, x: f64, y: f64) {
        self.last_mouse_pos = (x, y);

        // --- Update overlay tab hover state ---
        {
            let mut found_hover = false;
            for (&leaf_id, zones) in &self.leaf_tab_hit_zones {
                let [tx, ty, tw, th] = zones.tab_rect;
                if x >= tx && x < tx + tw && y >= ty && y < ty + th {
                    // Inside this tab — determine which sub-zone
                    let [dx, dy, dw, dh] = zones.dots_rect;
                    let [cx, cy, cw, ch] = zones.color_tag_rect;
                    let zone = if x >= dx && x < dx + dw && y >= dy && y < dy + dh {
                        zengeld_chart::LeafTabHoverZone::GearMenu
                    } else if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                        zengeld_chart::LeafTabHoverZone::ColorTag
                    } else {
                        zengeld_chart::LeafTabHoverZone::Body
                    };
                    self.leaf_tab_hover = zone;
                    self.leaf_tab_hovered_leaf = Some(leaf_id);
                    found_hover = true;
                    break;
                }
            }
            if !found_hover {
                self.leaf_tab_hover = zengeld_chart::LeafTabHoverZone::None;
                self.leaf_tab_hovered_leaf = None;
            }
        }

        // --- Clear all toolbar/dropdown hover states ---
        self.panel_app.toolbar_state.hovered_left_toolbar_id = None;
        self.panel_app.toolbar_state.hovered_top_toolbar_id = None;
        self.panel_app.toolbar_state.hovered_right_toolbar_id = None;
        self.panel_app.toolbar_state.hovered_bottom_toolbar_id = None;
        self.panel_app.toolbar_state.hovered_dropdown_item = None;
        self.panel_app.toolbar_state.hovered_inline_id = None;
        self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
        self.watchlist_modal.hovered_widget = None;

        // --- Update toolbar hover state from InputCoordinator ---
        // The coordinator's hovered_widget() returns the topmost widget under
        // the pointer as of the last begin_frame() (which uses last_mouse_pos).
        // We parse the prefix to determine which toolbar strip is hovered.
        if let Some(hovered) = self.input_coordinator.borrow_mut().hovered_widget() {
            let id_str = &hovered.0;
            if let Some(item_id) = id_str.strip_prefix("dtb:") {
                self.panel_app.toolbar_state.hovered_left_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("csb:") {
                self.panel_app.toolbar_state.hovered_top_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("btb:") {
                self.panel_app.toolbar_state.hovered_bottom_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("rtb:") {
                self.panel_app.toolbar_state.hovered_right_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("ilb:") {
                // Check if this is an inline dropdown item (style_option or width_option)
                if item_id.starts_with("inline:style_option:") || item_id.starts_with("inline:width_option:") {
                    self.panel_app.toolbar_state.hovered_inline_dropdown_item = Some(item_id.to_string());
                } else if item_id == "inline_dropdown:__bg__" {
                    // background absorber — no highlight
                } else {
                    self.panel_app.toolbar_state.hovered_inline_id = Some(item_id.to_string());
                    // Clear inline dropdown hover when moving back to toolbar items
                    self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
                }
            } else if id_str.starts_with("dropdown:") && id_str != "dropdown:__bg__" {
                // "dropdown:{dropdown_id}:{item_id}" → store the item_id part
                let item_id = id_str.splitn(3, ':').nth(2).unwrap_or("").to_string();
                self.panel_app.toolbar_state.hovered_dropdown_item = Some(item_id);
            } else if let Some(rest) = id_str.strip_prefix("context_menu:item:") {
                self.hovered_context_menu_item_id = Some(rest.to_string());
            } else if id_str == "context_menu:bg" {
                self.hovered_context_menu_item_id = None;
            } else if id_str.starts_with("leaf_tab:") {
                // Overlay tab hover is handled separately via leaf_tab_hit_zones.
                // Registration only exists to suppress crosshair and set cursor to default.
            } else if id_str.starts_with("wl_modal:") || id_str.starts_with("wl_group_name:") {
                self.watchlist_modal.hovered_widget = Some(id_str.to_string());
            } else if let Some(rest) = id_str.strip_prefix("ind_search:") {
                // Indicator search sets view — "set_create" or "set:{id}"
                self.modal_state.hovered_item_id = Some(rest.to_string());
            } else if let Some(rest) = id_str.strip_prefix("modal_search:item:") {
                // Search overlay result items
                self.modal_state.hovered_item_id = Some(rest.to_string());
            }
        } else {
            // No widget hovered — clear search overlay hover
            if self.modal_state.current.is_search_overlay() {
                self.modal_state.hovered_item_id = None;
            }
        }

        // --- Update modal hover states from frame_result hit zones ---
        // Always update hover highlights for modals regardless of crosshair visibility.
        let has_primitive_settings = self.panel_app.primitive_settings_state.is_open();
        let has_chart_settings = self.panel_app.chart_settings_state.is_open;
        let has_indicator_settings = self.panel_app.indicator_settings_state.is_open();

        if let Some(result) = &self.frame_result {
            // Primitive settings modal — update hover highlight
            if has_primitive_settings {
                if let Some(ref ps) = result.primitive_settings {
                    let mut found = false;
                    for (id, rect) in &ps.content_items {
                        if rect.contains(x, y) {
                            self.panel_app.primitive_settings_state.hovered_item_id = Some(id.clone());
                            found = true;
                            break;
                        }
                    }
                    if !found && ps.modal_rect.contains(x, y) {
                        self.panel_app.primitive_settings_state.hovered_item_id = None;
                    }
                }
            }

            // Chart settings modal — update hover highlight
            if has_chart_settings {
                if let Some(ref cs) = result.chart_settings {
                    let in_modal = cs.modal_rect.contains(x, y);
                    let mut found = false;
                    let mut found_footer = false;
                    // Footer buttons checked first (includes dropdown items that may extend
                    // outside the modal rect when the template dropdown is open).
                    for (id, rect) in &cs.footer_buttons {
                        if rect.contains(x, y) {
                            self.panel_app.chart_settings_state.hovered_footer_button = Some(id.clone());
                            found_footer = true;
                            break;
                        }
                    }
                    if in_modal {
                        for (id, rect) in &cs.tab_rects {
                            if rect.contains(x, y) {
                                self.panel_app.chart_settings_state.hovered_item_id = Some(id.clone());
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            for (id, rect) in &cs.content_items {
                                if rect.contains(x, y) {
                                    let hover_id = if id.starts_with("dropdown_option:") {
                                        id.split(':').nth(2).unwrap_or(id.as_str()).to_string()
                                    } else {
                                        id.clone()
                                    };
                                    self.panel_app.chart_settings_state.hovered_item_id = Some(hover_id);
                                    found = true;
                                    break;
                                }
                            }
                        }
                        if !found {
                            self.panel_app.chart_settings_state.hovered_item_id = None;
                        }
                    }
                    if !found_footer {
                        self.panel_app.chart_settings_state.hovered_footer_button = None;
                    }
                }
            }

            // Indicator settings modal — update hover highlight
            if has_indicator_settings {
                if let Some(ref is) = result.indicator_settings {
                    let in_modal = is.modal_rect.contains(x, y);
                    let mut found = false;
                    let mut found_footer = false;
                    // Footer buttons checked first (includes dropdown items outside modal rect)
                    for (id, rect) in &is.footer_buttons {
                        if rect.contains(x, y) {
                            self.panel_app.indicator_settings_state.hovered_footer_button = Some(id.clone());
                            found_footer = true;
                            break;
                        }
                    }
                    if in_modal {
                        for (id, rect) in &is.tab_rects {
                            if rect.contains(x, y) {
                                self.panel_app.indicator_settings_state.hovered_item_id = Some(id.clone());
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            for (id, rect) in &is.content_items {
                                if rect.contains(x, y) {
                                    let field_name = if id.starts_with("input:") {
                                        id[6..].to_string()
                                    } else if id.starts_with("color:") {
                                        id[6..].to_string()
                                    } else {
                                        id.clone()
                                    };
                                    self.panel_app.indicator_settings_state.hovered_item_id = Some(field_name);
                                    found = true;
                                    break;
                                }
                            }
                        }
                        if !found {
                            self.panel_app.indicator_settings_state.hovered_item_id = None;
                        }
                    }
                    if !found_footer {
                        self.panel_app.indicator_settings_state.hovered_footer_button = None;
                    }
                }
            }
        }

        // Chart browser modal — update hovered preset for icon visibility
        if self.panel_app.chart_browser.is_open {
            if let Some(ref result) = self.frame_result {
                if let Some(ref br) = result.chart_browser {
                    if br.list_viewport_rect.contains(x, y) {
                        let mut found_id: Option<String> = None;
                        for (preset_id, item_rect, _, _) in &br.item_rects {
                            if item_rect.contains(x, y) {
                                found_id = Some(preset_id.clone());
                                break;
                            }
                        }
                        self.panel_app.chart_browser.hovered_preset_id = found_id;
                    } else {
                        self.panel_app.chart_browser.hovered_preset_id = None;
                    }
                }
            }
        }

        // Watchlist modal — update hovered item for row highlight
        if self.watchlist_modal.is_open() {
            if let Some(ref wl) = self.last_watchlist_modal_result {
                if wl.list_viewport_rect.contains(x, y) {
                    let mut found_id: Option<String> = None;
                    for (symbol, item_rect) in &wl.item_rects {
                        if item_rect.contains(x, y) {
                            found_id = Some(symbol.clone());
                            break;
                        }
                    }
                    self.watchlist_modal.hovered_item_id = found_id;
                } else {
                    self.watchlist_modal.hovered_item_id = None;
                }
            }
        }

        // Tags & Tabs modal — update hover highlight for content items.
        // Uses overlay_settings_state.hovered_item_id so the MAP section
        // can respond to hover on minimap leaves and action buttons.
        if self.panel_app.tags_tabs_state.is_open {
            if let Some(ref result) = self.frame_result {
                if let Some(ref tt) = result.tags_tabs {
                    let mut found = false;
                    for (id, rect) in tt.content_items.iter()
                        .chain(tt.sidebar_rects.iter())
                        .chain(tt.sub_tab_rects.iter())
                    {
                        if rect.contains(x, y) {
                            self.panel_app.overlay_settings_state.hovered_item_id = Some(id.clone());
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        self.panel_app.overlay_settings_state.hovered_item_id = None;
                    }
                }
            }
        }

        // Sync color grid popup — update swatch hover state
        if self.panel_app.sync_color_grid.is_open() {
            if let Some(ref result) = self.frame_result {
                if let Some(ref grid_draw) = result.sync_color_grid {
                    use zengeld_chart::ui::sync_color_grid::hit_test_sync_color_grid;
                    let hit = hit_test_sync_color_grid(grid_draw, x, y);
                    match hit {
                        zengeld_chart::ui::sync_color_grid::SyncColorGridHitResult::Color(idx) => {
                            self.panel_app.sync_color_grid.hovered_index = Some(idx);
                            self.panel_app.sync_color_grid.hovered_remove = false;
                            self.panel_app.sync_color_grid.hovered_add = false;
                        }
                        zengeld_chart::ui::sync_color_grid::SyncColorGridHitResult::Remove => {
                            self.panel_app.sync_color_grid.hovered_index = None;
                            self.panel_app.sync_color_grid.hovered_remove = true;
                            self.panel_app.sync_color_grid.hovered_add = false;
                        }
                        zengeld_chart::ui::sync_color_grid::SyncColorGridHitResult::AddCustom => {
                            self.panel_app.sync_color_grid.hovered_index = None;
                            self.panel_app.sync_color_grid.hovered_remove = false;
                            self.panel_app.sync_color_grid.hovered_add = true;
                        }
                        _ => {
                            self.panel_app.sync_color_grid.hovered_index = None;
                            self.panel_app.sync_color_grid.hovered_remove = false;
                            self.panel_app.sync_color_grid.hovered_add = false;
                        }
                    }
                }
            }
        }

        // --- Split panel: update separator hover and per-leaf crosshair ---
        if self.panel_app.panel_grid.is_split() {
            let local_x = (x - self.content_rect.x) as f32;
            let local_y = (y - self.content_rect.y) as f32;
            self.panel_app.panel_grid.update_separator_hover(local_x, local_y);
        }

        // Hide crosshair during any UI drag (slider, scrollbar, modal header, color picker).
        // ui_drag_active is set in on_drag_start when is_over_ui() was true.
        if self.ui_drag_active {
            if self.panel_app.panel_grid.is_split() {
                self.hide_all_split_crosshairs();
            } else {
                self.hide_crosshair();
            }
            return;
        }

        // When a watchlist modal or group name input is open, suppress the crosshair
        // over the entire screen — modals should capture all interaction.
        if self.watchlist_modal.is_open() || self.wl_group_name_input.is_open() {
            if self.panel_app.panel_grid.is_split() {
                self.hide_all_split_crosshairs();
            } else {
                self.hide_crosshair();
            }
            return;
        }

        // Universal UI check: any registered widget hovered = hide crosshair.
        // Since the chart canvas is NOT a registered widget, is_over_ui() returns
        // true only when the pointer is over a toolbar, modal, context menu,
        // dropdown, color picker, or any other registered UI element.
        if self.input_coordinator.borrow_mut().is_over_ui() {
            // When actively drawing, keep updating the crosshair so the
            // preview line stays visible even when the cursor drifts over the
            // toolbar or sidebar.  Fall through to the crosshair update below.
            let is_drawing = self.panel_app.panel_grid.active_window()
                .map(|w| w.drawing_manager.is_drawing())
                .unwrap_or(false);
            if !is_drawing {
                if self.panel_app.panel_grid.is_split() {
                    self.hide_all_split_crosshairs();
                } else {
                    self.hide_crosshair();
                }
                return;
            }
        }

        // --- Split panel: per-leaf crosshair ---
        // Skip this block when actively drawing — the is_drawing block below
        // must handle projection so the preview extrapolates correctly even when
        // the cursor moves to a sub-pane, neighbor leaf, or outside the window.
        let is_drawing_skip = self.panel_app.panel_grid.active_window()
            .map(|w| w.drawing_manager.is_drawing())
            .unwrap_or(false);
        if self.panel_app.panel_grid.is_split() && !is_drawing_skip {
            use zengeld_chart::state::ChartInputTarget;
            let target = self.panel_app.panel_grid.resolve_input(
                x, y, self.content_rect.x, self.content_rect.y,
            );
            match target {
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => {
                    // Build layout for the hovered leaf.
                    let leaf_rect_opt = self.get_leaf_absolute_rect(leaf_id);
                    if let Some(leaf_rect) = leaf_rect_opt {
                        let extended_opt = self.build_extended_layout_for_leaf(leaf_id, &leaf_rect);
                        if let Some(extended) = extended_opt {
                            // Update crosshair on hovered leaf.
                            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                                window.update_crosshair_from_global(x, y, &extended, None);
                            }
                        }
                    }

                    // Collect crosshair state for sync propagation before any
                    // further mutable borrows.
                    let (bar_f64, crosshair_price, crosshair_visible, crosshair_pane_index) = self.panel_app
                        .panel_grid
                        .window_for_leaf(leaf_id)
                        .map(|w| (w.crosshair.bar_f64, w.crosshair.price, w.crosshair.visible, w.crosshair.pane_index))
                        .unwrap_or((0.0, 0.0, false, None));

                    // Determine which leaves are in the same sync group as the
                    // hovered leaf so they receive crosshair sync instead of hiding.
                    let source_color = self.panel_app.leaf_color_tags.get(&leaf_id).copied();
                    let all_ids: Vec<zengeld_chart::LeafId> = self.panel_app
                        .panel_grid
                        .panel_rects()
                        .keys()
                        .copied()
                        .filter(|&id| id != leaf_id)
                        .collect();
                    for other_id in all_ids {
                        let in_sync_group = source_color
                            .and_then(|sc| self.panel_app.leaf_color_tags.get(&other_id).copied()
                                .map(|c| sync_colors_match(sc, c)))
                            .unwrap_or(false);
                        if in_sync_group {
                            // Sync group peer — mirror crosshair bar position.
                            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(other_id) {
                                window.set_crosshair_from_bar(bar_f64, crosshair_price, crosshair_visible, crosshair_pane_index);
                            }
                        } else {
                            // Not in sync group — hide crosshair.
                            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(other_id) {
                                window.crosshair.visible = false;
                            }
                        }
                    }
                }
                ChartInputTarget::Separator { .. } | ChartInputTarget::None => {
                    self.hide_all_split_crosshairs();
                }
            }
            return;
        }

        // --- Update crosshair (only when not over any UI element, or when
        // actively drawing a primitive so the preview follows the cursor) ---
        // Build the extended layout first so we drop the immutable borrow on
        // panel_grid before the mutable borrow in active_window_mut.
        let extended = self.build_extended_layout();

        let is_drawing = self.panel_app.panel_grid.active_window()
            .map(|w| w.drawing_manager.is_drawing())
            .unwrap_or(false);

        if is_drawing {
            // When actively drawing, compute bar/price from raw screen coords
            // WITHOUT clamping — the preview must extend in the direction of
            // the cursor even when it's outside the chart area.
            let current_pane = self.panel_app.panel_grid.active_window()
                .and_then(|w| w.drawing_manager.current_pane());

            match current_pane {
                Some(instance_id) => {
                    // Drawing on a sub-pane.
                    if let Some(sp_layout) = extended.sub_panes.iter()
                        .find(|sp| sp.instance_id == instance_id)
                    {
                        let local_x = x - sp_layout.content.x;
                        let local_y = y - sp_layout.content.y;
                        let pane_height = sp_layout.content.height;
                        let pane_idx = extended.sub_panes.iter()
                            .position(|sp| sp.instance_id == instance_id);
                        let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                w.sub_panes.iter()
                                    .find(|sp| sp.instance_id == instance_id)
                                    .map(|sp| (sp.price_min, sp.price_max))
                            })
                            .unwrap_or((0.0, 100.0));
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let bar_f64 = window.viewport.x_to_bar_f64(local_x);
                            let bar_idx = window.viewport.x_to_bar(local_x);
                            let price_range = price_max - price_min;
                            let price = if pane_height > 0.0 {
                                price_max - (local_y / pane_height) * price_range
                            } else {
                                price_min
                            };
                            window.crosshair.visible = true;
                            window.crosshair.pane_index = pane_idx;
                            window.crosshair.x = local_x;
                            window.crosshair.y = local_y;
                            window.crosshair.bar_idx = bar_idx;
                            window.crosshair.bar_f64 = bar_f64;
                            window.crosshair.price = price;
                            window.crosshair.snapped_y = local_y;
                            window.crosshair.snapped_price = price;
                        }
                    }
                }
                None => {
                    // Drawing on main chart.
                    let chart_rect = extended.main_chart.chart;
                    let local_x = x - chart_rect.x;
                    let local_y = y - chart_rect.y;
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let bar_f64 = window.viewport.x_to_bar_f64(local_x);
                        let bar_idx = window.viewport.x_to_bar(local_x);
                        let price_range = window.price_scale.price_max - window.price_scale.price_min;
                        let price = if chart_rect.height > 0.0 {
                            window.price_scale.price_max - (local_y / chart_rect.height) * price_range
                        } else {
                            window.price_scale.price_min
                        };
                        window.crosshair.visible = true;
                        window.crosshair.pane_index = None;
                        window.crosshair.x = local_x;
                        window.crosshair.y = local_y;
                        window.crosshair.bar_idx = bar_idx;
                        window.crosshair.bar_f64 = bar_f64;
                        window.crosshair.price = price;
                        if window.crosshair.is_magnet() {
                            let (snapped_price, snapped_y) = window.calculate_magnet_snap(
                                bar_idx, price, chart_rect.height,
                                window.price_scale.price_min, window.price_scale.price_max,
                            );
                            window.crosshair.set_snapped(snapped_price, snapped_y);
                        } else {
                            window.crosshair.snapped_y = local_y;
                            window.crosshair.snapped_price = price;
                        }
                    }
                }
            }
            // Propagate crosshair to sync group peers during drawing so the
            // preview rubber-band line follows on peer windows.
            let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
            if let Some(active_leaf) = active_leaf_opt {
                let (bar_f64, price, crosshair_visible, pane_index) = self.panel_app
                    .panel_grid
                    .active_window()
                    .map(|w| (w.crosshair.bar_f64, w.crosshair.price, w.crosshair.visible, w.crosshair.pane_index))
                    .unwrap_or((0.0, 0.0, false, None));
                self.propagate_crosshair_to_sync_group(active_leaf, bar_f64, price, crosshair_visible, pane_index);
            }
        } else {
            // Compute drag_pane from current drag mode so the crosshair stays
            // locked to the originating pane during drag (clips to sub-pane
            // boundary, not main chart boundary).
            let is_dragging = self.input_handler.state.drag_mode != zengeld_chart::engine::input::DragMode::None;
            // For Primitive/ControlPoint drag modes, look up the primitive's pane_id
            // so the crosshair locks to the correct sub-pane rather than always the
            // main chart.
            let primitive_drag_pane: Option<Option<usize>> =
                match self.input_handler.state.drag_mode {
                    zengeld_chart::engine::input::DragMode::Primitive { id } => {
                        let pane_id = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.drawing_manager.primitives().iter()
                                .find(|p| p.data().id == id)
                                .and_then(|p| p.data().pane_id));
                        let pane_index = pane_id.and_then(|instance_id| {
                            extended.sub_panes.iter().position(|sp| sp.instance_id == instance_id)
                        });
                        match pane_index {
                            Some(idx) => Some(Some(idx)),
                            None => Some(None),
                        }
                    }
                    zengeld_chart::engine::input::DragMode::ControlPoint { primitive_id, .. } => {
                        let pane_id = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.drawing_manager.primitives().iter()
                                .find(|p| p.data().id == primitive_id)
                                .and_then(|p| p.data().pane_id));
                        let pane_index = pane_id.and_then(|instance_id| {
                            extended.sub_panes.iter().position(|sp| sp.instance_id == instance_id)
                        });
                        match pane_index {
                            Some(idx) => Some(Some(idx)),
                            None => Some(None),
                        }
                    }
                    _ => None,
                };
            let drag_pane = if is_dragging {
                match self.input_handler.state.drag_mode {
                    zengeld_chart::engine::input::DragMode::SubPaneChart { pane_index }
                    | zengeld_chart::engine::input::DragMode::SubPanePriceScale { pane_index } => {
                        Some(Some(pane_index))
                    }
                    zengeld_chart::engine::input::DragMode::Chart
                    | zengeld_chart::engine::input::DragMode::PriceScale
                    | zengeld_chart::engine::input::DragMode::TimeScale => {
                        Some(None)
                    }
                    zengeld_chart::engine::input::DragMode::Primitive { .. }
                    | zengeld_chart::engine::input::DragMode::ControlPoint { .. } => {
                        primitive_drag_pane
                    }
                    zengeld_chart::engine::input::DragMode::PaneSeparator { .. }
                    | zengeld_chart::engine::input::DragMode::Selection
                    | zengeld_chart::engine::input::DragMode::None => {
                        None
                    }
                }
            } else {
                None
            };
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.update_crosshair_from_global(x, y, &extended, drag_pane);
            }
            // Propagate crosshair to sync group peers (non-split path).
            let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
            if let Some(active_leaf) = active_leaf_opt {
                let (bar_f64, price, crosshair_visible, pane_index) = self.panel_app
                    .panel_grid
                    .active_window()
                    .map(|w| (w.crosshair.bar_f64, w.crosshair.price, w.crosshair.visible, w.crosshair.pane_index))
                    .unwrap_or((0.0, 0.0, false, None));
                self.propagate_crosshair_to_sync_group(active_leaf, bar_f64, price, crosshair_visible, pane_index);
            }
        }
    }

    /// Handle mouse leaving the canvas area — hide the crosshair.
    pub fn on_mouse_leave(&mut self) {
        // When actively drawing a primitive keep the last crosshair position so
        // the preview line remains visible if the cursor briefly leaves the
        // canvas (e.g. moves into the OS window border).
        let is_drawing = self.panel_app.panel_grid.active_window()
            .map(|w| w.drawing_manager.is_drawing())
            .unwrap_or(false);
        if !is_drawing {
            self.hide_crosshair();
        }
    }

    /// Hide the crosshair on the active window and propagate hide to sync group.
    fn hide_crosshair(&mut self) {
        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.crosshair.visible = false;
        }
        // Propagate hide to sync group peers.
        if let Some(active_leaf) = active_leaf_opt {
            self.propagate_crosshair_to_sync_group(active_leaf, 0.0, 0.0, false, None);
        }
    }

    /// Check if the crosshair magnet snap is currently active.
    /// Used by the window layer to hide the system cursor during magnet-lock.
    pub fn is_magnet_snapped(&self) -> bool {
        self.panel_app.panel_grid.active_window()
            .map(|w| w.crosshair.visible && w.crosshair.is_snapped())
            .unwrap_or(false)
    }

    /// Return the appropriate cursor style for screen position `(x, y)`.
    ///
    /// Checks open modals and color pickers first (highest priority), then
    /// chart-internal toolbar areas (top, left, right, bottom) — these always
    /// show the Default cursor.  Points outside all toolbar rects fall through
    /// to the extended hit tester, which returns Crosshair, NsResize, or
    /// EwResize as appropriate.
    pub fn get_cursor(&self, x: f64, y: f64) -> CursorStyle {
        // Hide system cursor when magnet snap is active — the crosshair renders at the snapped position.
        if self.is_magnet_snapped() {
            return CursorStyle::None;
        }

        // Sidebar separator: show EwResize cursor when hovering over or dragging the separator.
        let on_sidebar_separator = self.sidebar_separator_drag_active
            || self.input_coordinator.borrow_mut().hovered_widget()
                .map(|h| h.0 == "right_sidebar_separator")
                .unwrap_or(false);
        if on_sidebar_separator {
            return CursorStyle::EwResize;
        }

        // Watchlist column separators: show EwResize cursor when hovering or dragging.
        let on_watchlist_col_sep = self.sidebar_state.watchlist_sep_drag.is_some()
            || self.input_coordinator.borrow_mut().hovered_widget()
                .map(|h| h.0.starts_with("watchlist_sep_"))
                .unwrap_or(false);
        if on_watchlist_col_sep {
            return CursorStyle::EwResize;
        }

        // When a watchlist modal or group name input is open, the entire screen
        // should show the default cursor — modals capture all interaction.
        if self.watchlist_modal.is_open() || self.wl_group_name_input.is_open() {
            return CursorStyle::Default;
        }

        // Universal UI check: any registered widget hovered = default cursor.
        // Since the chart canvas is NOT a registered widget, is_over_ui() returns
        // true only when the pointer is over a toolbar, modal, context menu,
        // dropdown, color picker, or any other registered UI element.
        if self.input_coordinator.borrow_mut().is_over_ui() {
            return CursorStyle::Default;
        }

        // Split panel: check for separator cursor first.
        if self.panel_app.panel_grid.is_split() {
            use zengeld_chart::state::ChartInputTarget;
            use zengeld_chart::SeparatorOrientation;
            let target = self.panel_app.panel_grid.resolve_input(
                x, y, self.content_rect.x, self.content_rect.y,
            );
            match target {
                ChartInputTarget::Separator { orientation, .. } => {
                    return match orientation {
                        SeparatorOrientation::Horizontal => CursorStyle::NsResize,
                        SeparatorOrientation::Vertical => CursorStyle::EwResize,
                    };
                }
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => {
                    // Build layout for the hovered leaf to get correct cursor.
                    if let Some(leaf_rect) = self.get_leaf_absolute_rect(leaf_id) {
                        if let Some(extended) = self.build_extended_layout_for_leaf(leaf_id, &leaf_rect) {
                            let tester = zengeld_chart::layout::ExtendedLayoutHitTester::new(&extended);
                            use zengeld_chart::input::ChartHitTester;
                            let hit = tester.hit_test(x, y);
                            return hit.cursor();
                        }
                    }
                    return CursorStyle::Default;
                }
                ChartInputTarget::None => return CursorStyle::Default,
            }
        }

        // Not over any UI element — use extended layout hit_test for chart zones
        // (includes sub-pane price scales and separators).
        let extended = self.build_extended_layout();
        let tester = zengeld_chart::layout::ExtendedLayoutHitTester::new(&extended);
        use zengeld_chart::input::ChartHitTester;
        let hit = tester.hit_test(x, y);
        hit.cursor()
    }

    // -------------------------------------------------------------------------
    // Scroll
    // -------------------------------------------------------------------------

    /// Handle mouse wheel / trackpad scroll at `(x, y)` with deltas `(dx, dy)`.
    ///
    /// - Horizontal scroll (`dx`) pans the chart.
    /// - Vertical scroll (`dy`) zooms the chart (pinch-to-zoom equivalent).
    ///
    /// When a modal is open, scroll is routed to the modal's scroll state
    /// instead of the chart canvas.
    pub fn on_scroll(&mut self, x: f64, y: f64, dx: f64, dy: f64) {
        // Route scroll to open modal content instead of blocking entirely.
        if self.input_coordinator.borrow_mut().is_blocked_by_modal(x, y) {
            // Normalise dy: wheel delta is typically negative when scrolling down
            // (content moves up), so negate to convert to "scroll offset increase".
            let scroll_step = -dy;

            // Color picker opacity slider scroll (works for both L1 and L2, and all sources).
            // Each scroll notch changes opacity by 1% (0.01).
            {
                let opacity_step = 0.01 * -dy.signum(); // scroll up = more opaque
                enum OpacityScrollAction {
                    Apply(String), // source name
                    None,
                }
                let action: OpacityScrollAction = if let Some(ref fr) = self.frame_result {
                    if let Some(ref cp) = fr.color_picker {
                        let slider_hit = if let Some(ref l1) = cp.l1_result {
                            l1.opacity_slider_rect.as_ref().map_or(false, |r| r.contains(x, y))
                        } else if let Some(ref l2) = cp.l2_result {
                            l2.opacity_slider_rect.contains(x, y)
                        } else {
                            false
                        };
                        if slider_hit {
                            if self.panel_app.primitive_settings_state.is_color_picker_open() {
                                OpacityScrollAction::Apply("primitive".to_string())
                            } else if self.panel_app.indicator_settings_state.is_color_picker_open() {
                                OpacityScrollAction::Apply("indicator".to_string())
                            } else if self.panel_app.chart_settings_state.is_color_picker_open() {
                                OpacityScrollAction::Apply("chart".to_string())
                            } else {
                                OpacityScrollAction::None
                            }
                        } else {
                            OpacityScrollAction::None
                        }
                    } else {
                        OpacityScrollAction::None
                    }
                } else {
                    OpacityScrollAction::None
                };
                match action {
                    OpacityScrollAction::Apply(src) => {
                        match src.as_str() {
                            "primitive" => {
                                let current = self.panel_app.primitive_settings_state.color_picker.get_opacity();
                                let new_opacity = (current + opacity_step).clamp(0.0, 1.0);
                                self.panel_app.primitive_settings_state.color_picker.set_opacity(new_opacity);
                                let color = self.panel_app.primitive_settings_state.color_picker.get_final_color();
                                self.apply_primitive_color(&color);
                            }
                            "indicator" => {
                                let current = self.panel_app.indicator_settings_state.color_picker.get_opacity();
                                let new_opacity = (current + opacity_step).clamp(0.0, 1.0);
                                self.panel_app.indicator_settings_state.color_picker.set_opacity(new_opacity);
                                let color = self.panel_app.indicator_settings_state.color_picker.get_final_color();
                                let field = self.panel_app.indicator_settings_state.color_picker_field.clone();
                                if let Some(ref f) = field { self.apply_indicator_color(f, &color); }
                            }
                            "chart" => {
                                let current = self.panel_app.chart_settings_state.color_picker.get_opacity();
                                let new_opacity = (current + opacity_step).clamp(0.0, 1.0);
                                self.panel_app.chart_settings_state.color_picker.set_opacity(new_opacity);
                                let color = self.panel_app.chart_settings_state.color_picker.get_final_color();
                                let field = self.panel_app.chart_settings_state.color_picker_field.clone();
                                if let Some(ref f) = field { self.apply_chart_settings_color(f, &color); }
                            }
                            _ => {}
                        }
                        return;
                    }
                    OpacityScrollAction::None => {}
                }
            }

            // Search / indicator / compare modal (modal_state)
            if self.modal_state.is_open() {
                // Use real content/viewport heights from the last render result when
                // available; fall back to safe defaults so scroll is never unbounded.
                let (content_h, viewport_h) = if let Some(ref smr) = self.search_modal_result {
                    (smr.total_content_height, smr.viewport_height)
                } else {
                    (600.0, 600.0) // no content yet — scroll disabled
                };
                if content_h > viewport_h {
                    self.modal_state.scroll.handle_wheel(scroll_step, content_h, viewport_h);
                }
                return;
            }

            // Primitive settings modal — check slider tracks first
            if self.panel_app.primitive_settings_state.is_open() {
                // +1 or -1 per scroll notch (scrolling "down" = delta_y positive = decrease value)
                let delta = dy.signum();
                let hit_slider = if let Some(ref result) = self.frame_result {
                    if let Some(ref ps) = result.primitive_settings {
                        let mut found = false;
                        for track in &ps.slider_tracks {
                            if let Some((_, item_rect)) = ps.content_items.iter().find(|(id, _)| id == &track.field_id) {
                                if item_rect.contains(x, y) {
                                    let field_id = track.field_id.clone();
                                    let min_val = track.min_val;
                                    let max_val = track.max_val;
                                    if field_id == "stroke_width" {
                                        if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                                            if let Some(data) = self.panel_app.panel_grid.active_window()
                                                .and_then(|w| w.drawing_manager.get_data_at(idx))
                                            {
                                                let new_value = (data.width + delta).clamp(min_val, max_val);
                                                self.apply_slider_value(&field_id, new_value);
                                            }
                                        }
                                    } else if let Some(prop_id_owned) = field_id.strip_prefix("style_prop:").map(|s| s.to_string()) {
                                        if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                                            if let Some(window) = self.panel_app.panel_grid.active_window() {
                                                let prims = window.drawing_manager.primitives();
                                                if idx < prims.len() {
                                                    let style_props = prims[idx].style_properties();
                                                    if let Some(prop) = style_props.iter().find(|p| p.id == prop_id_owned.as_str()) {
                                                        if let Some(current) = prop.value.as_number() {
                                                            let new_value = (current + delta).clamp(min_val, max_val);
                                                            self.apply_slider_value(&field_id, new_value);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else if field_id.starts_with("tf_") && field_id.ends_with("_slider") {
                                        if let Some(tf_idx) = field_id.strip_prefix("tf_")
                                            .and_then(|s| s.strip_suffix("_slider"))
                                            .and_then(|s| s.parse::<usize>().ok())
                                        {
                                            if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                                                if let Some(data) = self.panel_app.panel_grid.active_window()
                                                    .and_then(|w| w.drawing_manager.get_data_at(idx))
                                                {
                                                    let mut new_data = data.clone();
                                                    let mut tf_config = new_data.timeframe_visibility.clone()
                                                        .unwrap_or_else(TimeframeVisibilityConfig::all);
                                                    let (min_allowed, max_allowed): (u32, u32) = match tf_idx {
                                                        1 => (1, 59), 2 => (1, 59), 3 => (1, 24),
                                                        4 => (1, 366), 5 => (1, 52), 6 => (1, 12),
                                                        _ => { found = true; break; }
                                                    };
                                                    let (current_min, current_max) = match tf_idx {
                                                        1 => tf_config.seconds.unwrap_or((1, 59)),
                                                        2 => tf_config.minutes.unwrap_or((1, 59)),
                                                        3 => tf_config.hours.unwrap_or((1, 24)),
                                                        4 => tf_config.days.unwrap_or((1, 366)),
                                                        5 => tf_config.weeks.unwrap_or((1, 52)),
                                                        6 => tf_config.months.unwrap_or((1, 12)),
                                                        _ => { found = true; break; }
                                                    };
                                                    let t = ((x - item_rect.x) / item_rect.width).clamp(0.0, 1.0);
                                                    let min_pos = (current_min - min_allowed) as f64 / (max_allowed - min_allowed) as f64;
                                                    let max_pos = (current_max - min_allowed) as f64 / (max_allowed - min_allowed) as f64;
                                                    let delta_i = delta as i32;
                                                    let new_range = if (t - min_pos).abs() < (t - max_pos).abs() {
                                                        let new_min = ((current_min as i32 + delta_i) as u32).clamp(min_allowed, current_max);
                                                        (new_min, current_max)
                                                    } else {
                                                        let new_max = ((current_max as i32 + delta_i) as u32).clamp(current_min, max_allowed);
                                                        (current_min, new_max)
                                                    };
                                                    match tf_idx {
                                                        1 => tf_config.seconds = Some(new_range),
                                                        2 => tf_config.minutes = Some(new_range),
                                                        3 => tf_config.hours = Some(new_range),
                                                        4 => tf_config.days = Some(new_range),
                                                        5 => tf_config.weeks = Some(new_range),
                                                        6 => tf_config.months = Some(new_range),
                                                        _ => {}
                                                    }
                                                    new_data.timeframe_visibility = Some(tf_config);
                                                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                                        window.drawing_manager.set_data_at(idx, &new_data);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    found = true;
                                    break;
                                }
                            }
                        }
                        found
                    } else {
                        false
                    }
                } else {
                    false
                };
                let _ = hit_slider;
                return;
            }

            // Chart settings modal — check slider tracks first, then content scroll
            if self.panel_app.chart_settings_state.is_open {
                let delta = dy.signum();

                // Collect slider action outside of frame_result borrow scope.
                // Returns (field_id, new_value) if a slider was hit, or None.
                enum CsScrollAction {
                    Slider(String, f64),
                    ContentScroll(f64, f64), // total_content_height, viewport_height
                    Swallow,
                }
                let action: CsScrollAction = if let Some(ref result) = self.frame_result {
                    if let Some(ref cs) = result.chart_settings {
                        let mut found_action = None;
                        'track_loop: for track in &cs.slider_tracks {
                            if let Some((_, item_rect)) = cs.content_items.iter().find(|(id, _)| id == &track.field_id) {
                                if item_rect.contains(x, y) {
                                    let field_id = track.field_id.clone();
                                    let min_val = track.min_val;
                                    let max_val = track.max_val;
                                    if let Some(param_id) = field_id.strip_prefix("appearance:style_") {
                                        let param_id = param_id.to_string();
                                        let params = &self.panel_app.theme_manager.current().style_params;
                                        let current_value = match param_id.as_str() {
                                            "toolbar_opacity"         => params.toolbar_bg_opacity as f64,
                                            "modal_opacity"           => params.modal_bg_opacity as f64,
                                            "sidebar_opacity"         => params.sidebar_bg_opacity as f64,
                                            "menu_opacity"            => params.menu_bg_opacity as f64,
                                            "scale_opacity"           => params.scale_bg_opacity as f64,
                                            "hover_opacity"           => params.hover_bg_opacity as f64,
                                            "crosshair_label_opacity" => params.crosshair_label_bg_opacity as f64,
                                            "blur_radius"             => params.blur_radius as f64,
                                            _ => break 'track_loop,
                                        };
                                        let step = if param_id == "blur_radius" { 1.0 } else { 0.05 };
                                        let new_value = (current_value + delta * step).clamp(min_val, max_val);
                                        found_action = Some(CsScrollAction::Slider(field_id, new_value));
                                    } else if field_id.starts_with("scales:") {
                                        let current_value = if let Some(window) = self.panel_app.panel_grid.active_window() {
                                            match field_id.as_str() {
                                                "scales:price_width_slider"   => window.scale_settings.price_scale_width,
                                                "scales:time_height_slider"   => window.scale_settings.time_scale_height,
                                                "scales:crosshair_line_width" => window.crosshair_options.vert_line.width,
                                                _ => break 'track_loop,
                                            }
                                        } else { break 'track_loop; };
                                        let step = if field_id == "scales:crosshair_line_width" { 0.1 } else { 1.0 };
                                        let new_value = (current_value + delta * step).clamp(min_val, max_val);
                                        found_action = Some(CsScrollAction::Slider(field_id, new_value));
                                    }
                                    break 'track_loop;
                                }
                            }
                        }
                        if found_action.is_none() && cs.content_rect.contains(x, y) {
                            found_action = Some(CsScrollAction::ContentScroll(cs.total_content_height, cs.viewport_height));
                        }
                        found_action.unwrap_or(CsScrollAction::Swallow)
                    } else {
                        CsScrollAction::Swallow
                    }
                } else {
                    CsScrollAction::Swallow
                };
                match action {
                    CsScrollAction::Slider(field_id, new_value) => {
                        self.apply_slider_value(&field_id, new_value);
                    }
                    CsScrollAction::ContentScroll(total_h, viewport_h) => {
                        self.panel_app.chart_settings_state.scroll.handle_wheel(scroll_step, total_h, viewport_h);
                    }
                    CsScrollAction::Swallow => {
                        self.panel_app.chart_settings_state.scroll.handle_wheel(scroll_step, 3000.0, 500.0);
                    }
                }
                return;
            }

            // Compare settings modal — check tf_ dual sliders and line_width slider first
            if self.panel_app.compare_settings_state.is_open() {
                use zengeld_chart::ui::modal_settings::CompareSettingsTab;
                let delta = dy.signum();

                enum CmpScrollAction {
                    TfSlider { field_id: String, tf_idx: usize, x_pos: f64, item_rect_x: f64, item_rect_w: f64 },
                    LineWidth { current: f64, min_val: f64, max_val: f64 },
                    Swallow,
                }

                let action: CmpScrollAction = if let Some(ref result) = self.frame_result {
                    if let Some(ref cs) = result.compare_settings {
                        // Only check dual sliders when visibility tab is active
                        let mut found = CmpScrollAction::Swallow;
                        if self.panel_app.compare_settings_state.active_tab == CompareSettingsTab::Visibility {
                            'cmp_tf_loop: for track in &cs.tf_slider_tracks {
                                if let Some((_, item_rect)) = cs.tf_content_items.iter()
                                    .find(|(id, _)| id == &track.field_id)
                                {
                                    if item_rect.contains(x, y) {
                                        if let Some(tf_idx) = track.field_id.strip_prefix("tf_")
                                            .and_then(|s| s.strip_suffix("_slider"))
                                            .and_then(|s| s.parse::<usize>().ok())
                                        {
                                            found = CmpScrollAction::TfSlider {
                                                field_id: track.field_id.clone(),
                                                tf_idx,
                                                x_pos: x,
                                                item_rect_x: item_rect.x,
                                                item_rect_w: item_rect.width,
                                            };
                                        }
                                        break 'cmp_tf_loop;
                                    }
                                }
                            }
                        }
                        // Check line_width slider when style tab is active
                        if matches!(found, CmpScrollAction::Swallow) {
                            if self.panel_app.compare_settings_state.active_tab == CompareSettingsTab::Style {
                                if let Some(ref lw_track) = cs.line_width_slider {
                                    let hit_x0 = lw_track.track_x - 6.0;
                                    let hit_x1 = lw_track.track_x + lw_track.track_width + 6.0;
                                    let hit_y0 = lw_track.track_y;
                                    let hit_y1 = lw_track.track_y + lw_track.track_height;
                                    if x >= hit_x0 && x <= hit_x1 && y >= hit_y0 && y <= hit_y1 {
                                        found = CmpScrollAction::LineWidth {
                                            current: self.panel_app.compare_settings_state.cached_line_width as f64,
                                            min_val: lw_track.min_val,
                                            max_val: lw_track.max_val,
                                        };
                                    }
                                }
                            }
                        }
                        found
                    } else {
                        CmpScrollAction::Swallow
                    }
                } else {
                    CmpScrollAction::Swallow
                };

                match action {
                    CmpScrollAction::TfSlider { field_id, tf_idx, x_pos, item_rect_x, item_rect_w } => {
                        use zengeld_chart::drawing::TimeframeVisibilityConfig;
                        use zengeld_chart::ui::modal_settings::DualSliderHandle;
                        let (min_allowed, max_allowed): (u32, u32) = match tf_idx {
                            1 => (1, 59), 2 => (1, 59), 3 => (1, 24),
                            4 => (1, 366), 5 => (1, 52), 6 => (1, 12),
                            _ => { return; }
                        };
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
                            _ => { return; }
                        };
                        let t = if item_rect_w > 0.0 { ((x_pos - item_rect_x) / item_rect_w).clamp(0.0, 1.0) } else { 0.5 };
                        let min_pos = (current_min - min_allowed) as f64 / (max_allowed - min_allowed) as f64;
                        let max_pos = (current_max - min_allowed) as f64 / (max_allowed - min_allowed) as f64;
                        let delta_i = delta as i32;
                        let new_range = if (t - min_pos).abs() <= (t - max_pos).abs() {
                            let new_min = ((current_min as i32 + delta_i) as u32).clamp(min_allowed, current_max);
                            (new_min, current_max)
                        } else {
                            let new_max = ((current_max as i32 + delta_i) as u32).clamp(current_min, max_allowed);
                            (current_min, new_max)
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
                        let series_idx = self.panel_app.compare_settings_state.series_index;
                        self.panel_app.compare_settings_state.cached_timeframe_visibility = Some(tf_config.clone());
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.compare_overlay.set_series_timeframe_visibility(series_idx, tf_config);
                        }
                        self.autosave_snapshot();
                        eprintln!("[ChartApp] cmp_settings scroll on tf_{}_slider", tf_idx);
                    }
                    CmpScrollAction::LineWidth { current, min_val, max_val } => {
                        let step = 0.5_f64;
                        let new_val = (current + delta * step).clamp(min_val, max_val) as f32;
                        let series_idx = self.panel_app.compare_settings_state.series_index;
                        self.panel_app.compare_settings_state.cached_line_width = new_val;
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.compare_overlay.set_series_line_width_by_index(series_idx, new_val);
                        }
                        self.autosave_snapshot();
                        eprintln!("[ChartApp] cmp_settings scroll on line_width: {}", new_val);
                    }
                    CmpScrollAction::Swallow => {}
                }
                return;
            }

            // Indicator settings modal — check tf_ dual sliders first, then content scroll
            if self.panel_app.indicator_settings_state.is_open() {
                use zengeld_chart::ui::modal_settings::IndicatorSettingsTab;
                let delta = dy.signum();

                // Check slider tracks first when visibility tab is active
                let hit_slider = if self.panel_app.indicator_settings_state.active_tab
                    == IndicatorSettingsTab::Visibility
                {
                    if let Some(ref result) = self.frame_result {
                        if let Some(ref is) = result.indicator_settings {
                            let mut found = false;
                            'ind_tf_loop: for track in &is.slider_tracks {
                                if let Some((_, item_rect)) = is.content_items.iter()
                                    .find(|(id, _)| id == &track.field_id)
                                {
                                    if item_rect.contains(x, y) {
                                        if let Some(tf_idx) = track.field_id.strip_prefix("tf_")
                                            .and_then(|s| s.strip_suffix("_slider"))
                                            .and_then(|s| s.parse::<usize>().ok())
                                        {
                                            use zengeld_chart::drawing::TimeframeVisibilityConfig;
                                            let (min_allowed, max_allowed): (u32, u32) = match tf_idx {
                                                1 => (1, 59), 2 => (1, 59), 3 => (1, 24),
                                                4 => (1, 366), 5 => (1, 52), 6 => (1, 12),
                                                _ => { break 'ind_tf_loop; }
                                            };
                                            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                                                if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                                                    let mut tf_config = inst.timeframe_visibility.clone()
                                                        .unwrap_or_else(TimeframeVisibilityConfig::all);
                                                    let (current_min, current_max) = match tf_idx {
                                                        1 => tf_config.seconds.unwrap_or((1, 59)),
                                                        2 => tf_config.minutes.unwrap_or((1, 59)),
                                                        3 => tf_config.hours.unwrap_or((1, 24)),
                                                        4 => tf_config.days.unwrap_or((1, 366)),
                                                        5 => tf_config.weeks.unwrap_or((1, 52)),
                                                        6 => tf_config.months.unwrap_or((1, 12)),
                                                        _ => { break 'ind_tf_loop; }
                                                    };
                                                    let t = if item_rect.width > 0.0 {
                                                        ((x - item_rect.x) / item_rect.width).clamp(0.0, 1.0)
                                                    } else { 0.5 };
                                                    let min_pos = (current_min - min_allowed) as f64 / (max_allowed - min_allowed) as f64;
                                                    let max_pos = (current_max - min_allowed) as f64 / (max_allowed - min_allowed) as f64;
                                                    let delta_i = delta as i32;
                                                    let new_range = if (t - min_pos).abs() <= (t - max_pos).abs() {
                                                        let new_min = ((current_min as i32 + delta_i) as u32).clamp(min_allowed, current_max);
                                                        (new_min, current_max)
                                                    } else {
                                                        let new_max = ((current_max as i32 + delta_i) as u32).clamp(current_min, max_allowed);
                                                        (current_min, new_max)
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
                                                    self.autosave_snapshot();
                                                    eprintln!("[ChartApp] ind_settings scroll on tf_{}_slider", tf_idx);
                                                }
                                            }
                                            found = true;
                                        }
                                        break 'ind_tf_loop;
                                    }
                                }
                            }
                            found
                        } else { false }
                    } else { false }
                } else { false };

                if hit_slider {
                    return;
                }

                if let Some(ref result) = self.frame_result {
                    if let Some(ref is) = result.indicator_settings {
                        if is.content_rect.contains(x, y) {
                            self.panel_app.indicator_settings_state.scroll.handle_wheel(
                                scroll_step,
                                is.total_content_height,
                                is.viewport_height,
                            );
                            return;
                        }
                    }
                }
                self.panel_app.indicator_settings_state.scroll.handle_wheel(scroll_step, 3000.0, 500.0);
                return;
            }

            // Watchlist modal list scroll
            if self.watchlist_modal.is_open() {
                if let Some(ref wl) = self.last_watchlist_modal_result {
                    if wl.list_viewport_rect.contains(x, y) {
                        self.watchlist_modal.scroll_offset = (
                            self.watchlist_modal.scroll_offset - dy * 30.0
                        ).max(0.0).min((wl.total_content_height - wl.list_viewport_rect.height).max(0.0));
                        return;
                    }
                }
                // Swallow scroll when watchlist modal is open even outside list area.
                return;
            }

            // Chart browser modal list scroll
            if self.panel_app.chart_browser.is_open {
                if let Some(ref result) = self.frame_result {
                    if let Some(ref br) = result.chart_browser {
                        if br.list_viewport_rect.contains(x, y) {
                            self.panel_app.chart_browser.scroll_offset = (
                                self.panel_app.chart_browser.scroll_offset - dy * 30.0
                            ).max(0.0).min((br.total_content_height - br.list_viewport_rect.height).max(0.0));
                            return;
                        }
                    }
                }
                // Swallow scroll when browser is open even outside list area.
                return;
            }

            // Any other modal or widget layer — swallow the event.
            return;
        }

        // Check if mouse is over the right sidebar — route scroll there.
        if self.sidebar_state.is_right_open() {
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                let sr = &sidebar_result.sidebar_rect;
                if x >= sr.x && x <= sr.x + sr.width && y >= sr.y && y <= sr.y + sr.height {
                    // First check per-group signal content rects — they have priority over
                    // the outer sidebar scroller when the mouse is inside one of them.
                    let mut handled_by_group = false;
                    for (instance_id, group_rect) in &sidebar_result.signal_group_content_rects {
                        if x >= group_rect.x
                            && x <= group_rect.x + group_rect.width
                            && y >= group_rect.y
                            && y <= group_rect.y + group_rect.height
                        {
                            let iid = *instance_id;
                            let row_height = 24.0_f64; // signal_row_height
                            let max_visible = 8usize;
                            let signal_count = self
                                .sidebar_state
                                .indicator_signals
                                .groups
                                .iter()
                                .find(|g| g.instance_id == iid)
                                .map(|g| g.signals.len())
                                .unwrap_or(0);
                            let viewport_h = (signal_count.min(max_visible)) as f64 * row_height;
                            let total_h = signal_count as f64 * row_height;
                            let max_offset = (total_h - viewport_h).max(0.0);
                            let current_offset = self
                                .sidebar_state
                                .signal_group_scroll_offsets
                                .get(&iid)
                                .copied()
                                .unwrap_or(0.0);
                            let new_offset = (current_offset - dy * 30.0).clamp(0.0, max_offset);
                            self.sidebar_state
                                .signal_group_scroll_offsets
                                .insert(iid, new_offset);
                            handled_by_group = true;
                            break;
                        }
                    }
                    if handled_by_group {
                        return;
                    }

                    // Scroll down (dy > 0) increases offset; scroll up (dy < 0) decreases it.
                    let content_h = sidebar_result.content_height;
                    let viewport_h = sidebar_result.content_rect.height;
                    let max_offset = (content_h - viewport_h).max(0.0);
                    let scroll = self.sidebar_state.current_right_scroll_mut();
                    scroll.offset = (scroll.offset - dy * 30.0).clamp(0.0, max_offset);
                    return;
                }
            }
        }

        // Split panel: route scroll to the correct leaf.
        if self.panel_app.panel_grid.is_split() {
            use zengeld_chart::state::ChartInputTarget;
            let target = self.panel_app.panel_grid.resolve_input(
                x, y, self.content_rect.x, self.content_rect.y,
            );
            match target {
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => {
                    self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    // Fall through to chart engine with active leaf set.
                }
                ChartInputTarget::Separator { .. } | ChartInputTarget::None => return,
            }
        }

        let extended = self.build_extended_layout();
        let hit_tester = ExtendedLayoutHitTester::new(&extended);
        let actions = self.input_handler.process_action(
            ChartInputAction::Scroll { x, y, delta_x: dx, delta_y: dy },
            &hit_tester,
        );
        self.process_output_actions(actions);
    }

    // -------------------------------------------------------------------------
    // Keyboard
    // -------------------------------------------------------------------------

    /// Handle the Escape key — close any open modal or deselect the active tool.
    pub fn on_escape(&mut self) {
        // If chart browser modal is open, close it.
        if self.panel_app.chart_browser.is_open {
            self.panel_app.chart_browser.close();
            self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
            eprintln!("[ChartApp] chart browser closed via Escape");
            return;
        }

        // If Tags & Tabs modal is open, close it.
        if self.panel_app.tags_tabs_state.is_open {
            self.panel_app.close_tags_tabs();
            eprintln!("[ChartApp] tags_tabs closed via Escape");
            return;
        }

        // If watchlist group name input is open, close it.
        if self.wl_group_name_input.is_open() {
            self.wl_group_name_input.close();
            eprintln!("[ChartApp] wl_group_name_input cancelled");
            return;
        }

        // If preset name input modal is open, close it.
        if self.panel_app.preset_name_input.is_open {
            self.panel_app.preset_name_input.close();
            self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
            eprintln!("[ChartApp] preset name input cancelled");
            return;
        }

        // If text editing is active in any modal, cancel it first before closing the modal.
        // This follows the pattern: first Escape cancels editing, second Escape closes the modal.
        if self.panel_app.primitive_settings_state.editing_text.is_some() {
            self.panel_app.primitive_settings_state.editing_text = None;
            eprintln!("[ChartApp] prim_settings text editing cancelled");
            return;
        }
        if self.panel_app.indicator_settings_state.editing_text_state.is_some() {
            self.panel_app.indicator_settings_state.editing_text_state = None;
            eprintln!("[ChartApp] ind_settings text editing cancelled");
            return;
        }
        if self.panel_app.chart_settings_state.editing_text.is_some() {
            self.panel_app.chart_settings_state.editing_text = None;
            eprintln!("[ChartApp] chart_settings text editing cancelled");
            return;
        }

        if self.modal_state.is_open() {
            self.modal_state.close();
            return;
        }

        if self.panel_app.context_menu_state.is_open() {
            self.panel_app.context_menu_state.close();
            return;
        }

        if self.panel_app.sync_color_grid.is_open() {
            self.panel_app.sync_color_grid.close();
            return;
        }

        // Close modals in priority order (state lives inside panel_app at checkpoint).
        if self.panel_app.primitive_settings_state.is_open() {
            self.panel_app.primitive_settings_state.close();
            return;
        }
        if self.panel_app.chart_settings_state.is_open {
            self.panel_app.chart_settings_state.close();
            return;
        }
        if self.panel_app.indicator_settings_state.is_open() {
            self.panel_app.indicator_settings_state.close();
            return;
        }
        if self.panel_app.alert_settings_state.is_open() {
            self.panel_app.alert_settings_state.close();
            return;
        }
        if self.panel_app.compare_settings_state.is_open() {
            self.panel_app.compare_settings_state.close();
            return;
        }

        // Deselect drawing tool.
        self.panel_app.toolbar_state.deselect_tool();
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.drawing_manager.set_tool(None);
        }
        // Clear any in-progress preview on sync peers (state is now Idle).
        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
            self.propagate_drawing_state_to_sync_group(active_leaf);
        }
    }

    /// Handle a printable character input event.
    ///
    /// Routes to active text editing state (primitive settings text_content).
    /// On Enter, commits the edited text back to the primitive.
    pub fn on_char_input(&mut self, ch: char) {
        // Handle chart browser modal search input
        if self.panel_app.chart_browser.is_open {
            match ch {
                '\r' | '\n' => {
                    // Enter does nothing — items are clicked to load
                }
                '\x08' => {
                    // Backspace — delete selection if active, else single char.
                    let editing = &mut self.panel_app.chart_browser.search_editing;
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_pos = editing.char_to_byte_pos(editing.cursor - 1);
                        let byte_end = editing.char_to_byte_pos(editing.cursor);
                        editing.text.drain(byte_pos..byte_end);
                        editing.cursor -= 1;
                    }
                    editing.reset_blink(0);
                    self.panel_app.chart_browser.search_query = self.panel_app.chart_browser.search_editing.text.clone();
                    self.panel_app.chart_browser.scroll_offset = 0.0;
                }
                c if !c.is_control() => {
                    let editing = &mut self.panel_app.chart_browser.search_editing;
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_pos = editing.char_to_byte_pos(editing.cursor);
                    editing.text.insert(byte_pos, c);
                    editing.cursor += 1;
                    editing.reset_blink(0);
                    self.panel_app.chart_browser.search_query = self.panel_app.chart_browser.search_editing.text.clone();
                    self.panel_app.chart_browser.scroll_offset = 0.0;
                }
                _ => {}
            }
            return;
        }

        // Handle watchlist modal search input (Overview tab only)
        {
            use zengeld_chart::ui::modal_settings::WatchlistModalTab;
            if self.watchlist_modal.is_open()
                && self.watchlist_modal.active_tab == WatchlistModalTab::Overview
            {
                match ch {
                    '\r' | '\n' => {
                        // Enter does nothing in the watchlist search field
                    }
                    '\x08' => {
                        // Backspace — delete selection if active, else single char.
                        let editing = &mut self.watchlist_modal.search_editing;
                        if editing.has_selection() {
                            editing.delete_selection();
                        } else if editing.cursor > 0 {
                            let byte_pos = editing.char_to_byte_pos(editing.cursor - 1);
                            let byte_end = editing.char_to_byte_pos(editing.cursor);
                            editing.text.drain(byte_pos..byte_end);
                            editing.cursor -= 1;
                        }
                        editing.reset_blink(0);
                        self.watchlist_modal.search_query = self.watchlist_modal.search_editing.text.clone();
                        self.watchlist_modal.scroll_offset = 0.0;
                    }
                    c if !c.is_control() => {
                        let editing = &mut self.watchlist_modal.search_editing;
                        if editing.has_selection() {
                            editing.delete_selection();
                        }
                        let byte_pos = editing.char_to_byte_pos(editing.cursor);
                        editing.text.insert(byte_pos, c);
                        editing.cursor += 1;
                        editing.reset_blink(0);
                        self.watchlist_modal.search_query = self.watchlist_modal.search_editing.text.clone();
                        self.watchlist_modal.scroll_offset = 0.0;
                    }
                    _ => {}
                }
                return;
            }
        }

        // Handle preset name input modal
        if self.panel_app.preset_name_input.is_open {
            let editing = &mut self.panel_app.preset_name_input.editing;
            match ch {
                '\r' | '\n' => {
                    // Commit — extract the name and mode, then close
                    let name = self.panel_app.preset_name_input.name().to_string();
                    if !name.trim().is_empty() {
                        use zengeld_chart::ui::modal_settings::PresetNameInputMode;
                        match self.panel_app.preset_name_input.mode {
                            PresetNameInputMode::SaveAs => {
                                self.process_chart_out_event(
                                    zengeld_chart::events::ChartOutEvent::SavePreset { name }
                                );
                            }
                            PresetNameInputMode::Rename => {
                                let id = self.panel_app.preset_name_input.rename_preset_id
                                    .clone().unwrap_or_default();
                                self.process_chart_out_event(
                                    zengeld_chart::events::ChartOutEvent::RenamePreset { id, new_name: name }
                                );
                            }
                            PresetNameInputMode::NewChart => {
                                self.execute_new_chart_with_name(name);
                            }
                            PresetNameInputMode::CreateIndicatorSet => {
                                self.execute_create_indicator_set(name);
                            }
                        }
                    }
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                }
                '\x08' => {
                    // Backspace — delete selection if active, else single char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                        let byte_start = editing.text.char_indices().nth(editing.cursor - 1).map(|(i, _)| i).unwrap_or(0);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    // If there is an active selection, replace it with the typed char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }

        // Handle new API key label input in User Settings Server tab
        if self.panel_app.user_settings_state.is_open
            && self.panel_app.user_settings_state.new_key_label_focused
        {
            match ch {
                '\r' | '\n' => {
                    // Enter submits the create form if label is non-empty
                    let label = self.panel_app.user_settings_state.new_key_label.trim().to_string();
                    let tier = self.panel_app.user_settings_state.new_key_tier.clone();
                    if !label.is_empty() {
                        self.key_create_request = Some((label, tier));
                        self.panel_app.user_settings_state.new_key_label.clear();
                    }
                    self.panel_app.user_settings_state.new_key_label_focused = false;
                }
                '\x1b' => {
                    // Escape unfocuses without submitting
                    self.panel_app.user_settings_state.new_key_label_focused = false;
                }
                '\x08' => {
                    // Backspace
                    let label = &mut self.panel_app.user_settings_state.new_key_label;
                    if !label.is_empty() {
                        label.pop();
                    }
                }
                c if !c.is_control() => {
                    self.panel_app.user_settings_state.new_key_label.push(c);
                }
                _ => {}
            }
            return;
        }

        // Handle watchlist group name input modal
        if self.wl_group_name_input.is_open() {
            let editing = &mut self.wl_group_name_input.editing;
            match ch {
                '\r' | '\n' => {
                    // Commit
                    let name = self.wl_group_name_input.editing.text.trim().to_string();
                    if !name.is_empty() {
                        use zengeld_chart::ui::modal_settings::WatchlistGroupNameMode;
                        match self.wl_group_name_input.mode.clone() {
                            WatchlistGroupNameMode::CreateNew => {
                                let new_id = self.sidebar_state.watchlist_manager.create_list(name.clone());
                                self.sidebar_state.watchlist_manager.active_list_id = new_id;
                                self.watchlist_actions.push(crate::WatchlistAction::CreateList { name: name.clone() });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[WatchlistGroupName] created new list '{}' id={}", name, new_id);
                            }
                            WatchlistGroupNameMode::Rename(id) => {
                                if let Some(list) = self.sidebar_state.watchlist_manager.lists.iter_mut().find(|l| l.id == id) {
                                    list.name = name.clone();
                                    self.watchlist_actions.push(crate::WatchlistAction::RenameList { id, new_name: name.clone() });
                                    self.watchlists_dirty = true;
                                    self.persist_watchlists();
                                    eprintln!("[WatchlistGroupName] renamed list id={} to '{}'", id, name);
                                }
                            }
                        }
                    }
                    self.wl_group_name_input.close();
                }
                '\x1b' => {
                    // Escape handled in on_escape, but also handle here for safety
                    self.wl_group_name_input.close();
                }
                '\x08' => {
                    // Backspace
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                        let byte_start = editing.text.char_indices().nth(editing.cursor - 1).map(|(i, _)| i).unwrap_or(0);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }

        // Handle text_content editing in primitive settings
        // Guard with is_open() so stale editing_text from a closed modal cannot
        // intercept keypresses intended for other active modals (e.g. chart settings).
        if self.panel_app.primitive_settings_state.is_open() {
        if let Some(ref mut editing) = self.panel_app.primitive_settings_state.editing_text {
            match ch {
                '\r' | '\n' => {
                    // Commit the text on Enter
                    let text = editing.text.clone();
                    let field = editing.field_id.clone();
                    self.panel_app.primitive_settings_state.editing_text = None;
                    if field == "text_content" {
                        if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                if let Some(mut data) = window.drawing_manager.get_data_at(idx) {
                                    if let Some(ref mut t) = data.text {
                                        t.content = text.clone();
                                    } else {
                                        data.text = Some({
                                            use zengeld_chart::drawing::primitives_v2::PrimitiveText;
                                            let mut pt = PrimitiveText::default();
                                            pt.content = text.clone();
                                            pt
                                        });
                                    }
                                    window.drawing_manager.set_data_at(idx, &data);
                                }
                            }
                        }
                        eprintln!("[ChartApp] prim_settings text_content committed: {}", text);
                    } else if field == "stroke_width_value" || field == "stroke_width" {
                        if let Ok(width) = text.trim().parse::<f64>() {
                            if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                    if let Some(mut data) = window.drawing_manager.get_data_at(idx) {
                                        data.width = width.max(0.5).min(20.0);
                                        window.drawing_manager.set_data_at(idx, &data);
                                    }
                                }
                            }
                        }
                    } else if field == "text_font_size" {
                        if let Ok(font_size) = text.trim().parse::<f64>() {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                window.drawing_manager.set_selected_text_font_size(font_size);
                            }
                        }
                        eprintln!("[ChartApp] prim_settings text_font_size committed: {}", text);
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
                                        let prims = window.drawing_manager.primitives_mut();
                                        if idx < prims.len() {
                                            let mut pts = prims[idx].points().to_vec();
                                            if pt_idx < pts.len() {
                                                pts[pt_idx].0 = bar;
                                                prims[idx].set_points(&pts);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(prop_id) = field.strip_prefix("text_prop:") {
                        // text_prop field — commit as string or number based on prop type
                        use zengeld_chart::drawing::primitives_v2::config::{PropertyValue, PropertyType};
                        if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                            // Look up the property type to decide how to commit
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
                                Some(PropertyType::Text { .. }) => {
                                    PropertyValue::String(text.clone())
                                }
                                _ => {
                                    // Number or unknown: try numeric parse, fall back to string
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
                        eprintln!("[ChartApp] prim_settings text_prop '{}' committed: {}", prop_id, text);
                    } else if let Some(prop_id) = field.strip_prefix("style_prop:") {
                        // style_prop Number field — commit parsed value
                        use zengeld_chart::drawing::primitives_v2::config::PropertyValue;
                        if let Ok(val) = text.trim().parse::<f64>() {
                            if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                    window.drawing_manager.apply_style_property(idx, prop_id, PropertyValue::Number(val));
                                }
                            }
                        }
                        eprintln!("[ChartApp] prim_settings style_prop '{}' committed: {}", prop_id, text);
                    } else {
                        // For all other unrecognized fields, just close editing.
                        eprintln!("[ChartApp] prim_settings '{}' editing closed (value: {})", field, text);
                    }
                }
                '\x08' => {
                    // Backspace — delete selection if active, else single char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                        let byte_start = editing.text.char_indices().nth(editing.cursor - 1).map(|(i, _)| i).unwrap_or(0);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    // If there is an active selection, replace it with the typed char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }
        } // end is_open() guard for primitive_settings

        // Handle text editing in indicator settings
        // Guard with is_open() so stale editing_text_state cannot intercept keypresses.
        if self.panel_app.indicator_settings_state.is_open() {
        if let Some(ref mut editing) = self.panel_app.indicator_settings_state.editing_text_state {
            match ch {
                '\r' | '\n' => {
                    // Commit the indicator param value on Enter
                    let text = editing.text.clone();
                    let field = editing.field_id.clone();
                    self.panel_app.indicator_settings_state.editing_text_state = None;
                    if let Some(param_name) = field.strip_prefix("indicator_param:") {
                        use zengeld_terminal_indicators::IndicatorParamValue as IndValue;
                        if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                            if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                                // Try parsing as float first, then int, else string
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
                            let symbol = self.panel_app.panel_grid.active_window()
                                .map(|w| w.symbol.clone()).unwrap_or_default();
                            if let Some(bars) = bars_opt {
                                self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
                            }
                        }
                        eprintln!("[ChartApp] ind_settings param '{}' committed: {}", param_name, text);
                    } else if field.starts_with("tf_") && field.ends_with("_min") {
                        if let Ok(val) = text.trim().parse::<u32>() {
                            if let Some(tf_idx) = field.strip_prefix("tf_")
                                .and_then(|s| s.strip_suffix("_min"))
                                .and_then(|s| s.parse::<usize>().ok())
                            {
                                self.apply_ind_tf_min_value(tf_idx, val);
                            }
                        }
                    } else if field.starts_with("tf_") && field.ends_with("_max") {
                        if let Ok(val) = text.trim().parse::<u32>() {
                            if let Some(tf_idx) = field.strip_prefix("tf_")
                                .and_then(|s| s.strip_suffix("_max"))
                                .and_then(|s| s.parse::<usize>().ok())
                            {
                                self.apply_ind_tf_max_value(tf_idx, val);
                            }
                        }
                    }
                    return;
                }
                '\x08' => {
                    // Backspace — delete selection if active, else single char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                        let byte_start = editing.text.char_indices().nth(editing.cursor - 1).map(|(i, _)| i).unwrap_or(0);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    // If there is an active selection, replace it with the typed char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }
        } // end is_open() guard for indicator_settings

        // Handle text editing in chart settings
        // Guard with is_open so zombie editing_text cannot intercept keypresses
        // when chart settings is closed.
        if self.panel_app.chart_settings_state.is_open {
        if let Some(ref mut editing) = self.panel_app.chart_settings_state.editing_text {
            match ch {
                '\r' | '\n' => {
                    // Enter: commit watermark text and close editing
                    let text = editing.text.clone();
                    let field = editing.field_id.clone();
                    self.panel_app.chart_settings_state.editing_text = None;
                    if field == "status:watermark_text" {
                        if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                            if let Some(ref mut watermark) = w.watermark {
                                if let Some(line) = watermark.lines.first_mut() {
                                    line.text = text.clone();
                                }
                            }
                        }
                        eprintln!("[ChartApp] chart_settings watermark_text committed: {}", text);
                    }
                }
                '\x08' => {
                    // Backspace — delete selection if active, else single char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                        let byte_start = editing.text.char_indices().nth(editing.cursor - 1).map(|(i, _)| i).unwrap_or(0);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    // If there is an active selection, replace it with the typed char.
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }
        } // end is_open guard for chart_settings

        // Handle text editing in compare settings (tf_*_min / tf_*_max fields).
        if self.panel_app.compare_settings_state.is_open() {
        if let Some(ref mut editing) = self.panel_app.compare_settings_state.editing_text {
            match ch {
                '\r' | '\n' => {
                    let text = editing.text.clone();
                    let field = editing.field_id.clone();
                    self.panel_app.compare_settings_state.editing_text = None;
                    self.apply_cmp_tf_text_commit(&field, &text);
                }
                '\x08' => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                        let byte_start = editing.text.char_indices().nth(editing.cursor - 1).map(|(i, _)| i).unwrap_or(0);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                '\x7f' => {
                    if editing.cursor < editing.text.chars().count() {
                        let byte_start = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                        let byte_end = editing.text.char_indices().nth(editing.cursor + 1).map(|(i, _)| i).unwrap_or(editing.text.len());
                        editing.text.drain(byte_start..byte_end);
                    }
                }
                c if !c.is_control() => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.text.char_indices().nth(editing.cursor).map(|(i, _)| i).unwrap_or(editing.text.len());
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }
        } // end is_open guard for compare_settings

        // Handle text input for Telegram bot token field in the Alert Settings modal.
        if self.panel_app.alert_settings_state.is_open() {
            let tg_token_focused = self.panel_app.alert_settings_state.tg_token_focused;

            if tg_token_focused {
                match ch {
                    '\r' | '\n' | '\x1b' => {
                        // Enter/Escape unfocuses and syncs buffer to settings
                        self.panel_app.alert_settings_state.tg_token_focused = false;
                        let token = self.panel_app.alert_settings_state.tg_bot_token_input.clone();
                        self.panel_app.alert_settings_state.notification_settings.telegram.bot_token = token;
                        self.panel_app.alert_settings_state.notification_settings_dirty = true;
                    }
                    '\x08' => {
                        // Backspace
                        self.panel_app.alert_settings_state.tg_bot_token_input.pop();
                    }
                    c if !c.is_control() => {
                        self.panel_app.alert_settings_state.tg_bot_token_input.push(c);
                    }
                    _ => {}
                }
                return;
            }
        }

        // Route typing to the search modal when it is open (indicator, symbol, or compare).
        if self.modal_state.current.is_search_overlay() {
            match ch {
                '\r' | '\n' => {
                    // Enter: select the first result if any, or just close.
                    let first_item = match self.modal_state.current {
                        OpenModal::SymbolSearch | OpenModal::CompareSearch => {
                            self.modal_state.symbol_search_results
                                .first()
                                .map(|r| format!("{}:{}", r.symbol, r.exchange_id))
                        }
                        OpenModal::IndicatorSearch => {
                            // Use hovered item or first result from IndicatorManager.
                            self.modal_state.hovered_item_id.clone()
                        }
                        _ => None,
                    };
                    if let Some(item_id) = first_item {
                        let rest = format!("item:{}", item_id);
                        self.handle_search_modal_click(&rest, 0.0, 0.0);
                    } else if self.modal_state.current != OpenModal::IndicatorSearch {
                        self.modal_state.close();
                    }
                }
                '\x08' => {
                    // Backspace — delete char before cursor in search query.
                    self.modal_state.delete_char_before(0);
                    // Re-filter symbol results after query change.
                    if self.modal_state.current == OpenModal::SymbolSearch
                        || self.modal_state.current == OpenModal::CompareSearch
                    {
                        let q = self.modal_state.search_query.clone();
                        self.modal_state.symbol_search_results =
                            crate::ChartApp::build_demo_symbol_results(&q, &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
                    }
                }
                c if !c.is_control() => {
                    // Regular character — insert into search query.
                    self.modal_state.insert_char(c, 0);
                    // Re-filter symbol results after query change.
                    if self.modal_state.current == OpenModal::SymbolSearch
                        || self.modal_state.current == OpenModal::CompareSearch
                    {
                        let q = self.modal_state.search_query.clone();
                        self.modal_state.symbol_search_results =
                            crate::ChartApp::build_demo_symbol_results(&q, &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
                    }
                }
                _ => {}
            }
        }
    }

    /// Handle named key presses that cannot be expressed as a single `char`.
    ///
    /// Routes cursor movement and delete to the active text editing state.
    /// Supports all `TextEditingState`-bearing modals:
    /// - `primitive_settings_state.editing_text`
    /// - `indicator_settings_state.editing_text_state`
    /// - `chart_settings_state.editing_text`
    /// - `modal_state.editing_text` (search input)
    pub fn on_key_press(&mut self, key: super::input::KeyPress) {
        use super::input::KeyPress;

        // ── Telegram input fields: intercept Paste for focused tg fields ──
        if self.panel_app.alert_settings_state.is_open() {
            if let KeyPress::Paste(ref text) = key {
                if self.panel_app.alert_settings_state.tg_token_focused {
                    self.panel_app.alert_settings_state.tg_bot_token_input.push_str(text);
                    return;
                }
            }
        }

        // Helper closure operating on a mutable TextEditingState reference.
        // Returns true if the key was consumed.
        fn apply_key(editing: &mut zengeld_chart::ui::modal_settings::TextEditingState, key: KeyPress) -> bool {
            let char_count = editing.text.chars().count();
            match key {
                // ── Delete (forward) ──────────────────────────────────────────
                KeyPress::Delete => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor < char_count {
                        let byte_idx = editing.text
                            .char_indices()
                            .nth(editing.cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(editing.text.len());
                        editing.text.remove(byte_idx);
                    }
                    true
                }
                // ── Plain movement — collapses any active selection ───────────
                KeyPress::ArrowLeft => {
                    if editing.has_selection() {
                        // Collapse to the left edge of the selection.
                        let (lo, _) = editing.selection_range().unwrap();
                        editing.cursor = lo;
                        editing.selection_start = None;
                    } else if editing.cursor > 0 {
                        editing.cursor -= 1;
                    }
                    true
                }
                KeyPress::ArrowRight => {
                    if editing.has_selection() {
                        // Collapse to the right edge of the selection.
                        let (_, hi) = editing.selection_range().unwrap();
                        editing.cursor = hi;
                        editing.selection_start = None;
                    } else if editing.cursor < char_count {
                        editing.cursor += 1;
                    }
                    true
                }
                KeyPress::Home => {
                    editing.cursor = 0;
                    editing.selection_start = None;
                    true
                }
                KeyPress::End => {
                    editing.cursor = char_count;
                    editing.selection_start = None;
                    true
                }
                // ── Select-all (Ctrl+A) ───────────────────────────────────────
                KeyPress::SelectAll => {
                    editing.select_all();
                    true
                }
                // ── Shift movement — extends/creates selection ────────────────
                KeyPress::ShiftLeft => {
                    if editing.selection_start.is_none() {
                        editing.selection_start = Some(editing.cursor);
                    }
                    if editing.cursor > 0 {
                        editing.cursor -= 1;
                    }
                    // If anchor == cursor after move, clear selection.
                    if editing.selection_start == Some(editing.cursor) {
                        editing.selection_start = None;
                    }
                    true
                }
                KeyPress::ShiftRight => {
                    if editing.selection_start.is_none() {
                        editing.selection_start = Some(editing.cursor);
                    }
                    if editing.cursor < char_count {
                        editing.cursor += 1;
                    }
                    if editing.selection_start == Some(editing.cursor) {
                        editing.selection_start = None;
                    }
                    true
                }
                KeyPress::ShiftHome => {
                    if editing.selection_start.is_none() {
                        editing.selection_start = Some(editing.cursor);
                    }
                    editing.cursor = 0;
                    if editing.selection_start == Some(editing.cursor) {
                        editing.selection_start = None;
                    }
                    true
                }
                KeyPress::ShiftEnd => {
                    if editing.selection_start.is_none() {
                        editing.selection_start = Some(editing.cursor);
                    }
                    editing.cursor = char_count;
                    if editing.selection_start == Some(editing.cursor) {
                        editing.selection_start = None;
                    }
                    true
                }
                // ── Copy (Ctrl+C) — handled externally; no state change here ─
                KeyPress::Copy => {
                    // Copy is handled by the platform runner via on_copy_selection().
                    // Return false so the caller knows no state was mutated.
                    false
                }
                // ── Paste (Ctrl+V) — insert supplied text at cursor ───────────
                KeyPress::Paste(ref text) => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    for ch in text.chars() {
                        if !ch.is_control() {
                            let byte_idx = editing.char_to_byte_pos(editing.cursor);
                            editing.text.insert(byte_idx, ch);
                            editing.cursor += 1;
                        }
                    }
                    true
                }
                // ── Undo/Redo — not consumed by text fields ───────────────────
                KeyPress::Undo | KeyPress::Redo => false,
            }
        }

        // ── Global undo/redo — handled before any modal short-circuits ────────
        match key {
            KeyPress::Undo => {
                self.perform_undo_with_group();
                return;
            }
            KeyPress::Redo => {
                self.perform_redo_with_group();
                return;
            }
            _ => {}
        }

        // 0a. Chart browser search input
        if self.panel_app.chart_browser.is_open {
            apply_key(&mut self.panel_app.chart_browser.search_editing, key);
            // Keep search_query in sync with search_editing.text
            self.panel_app.chart_browser.search_query = self.panel_app.chart_browser.search_editing.text.clone();
            self.panel_app.chart_browser.scroll_offset = 0.0;
            return;
        }

        // 0b. Watchlist modal search input (Overview tab only)
        {
            use zengeld_chart::ui::modal_settings::WatchlistModalTab;
            if self.watchlist_modal.is_open()
                && self.watchlist_modal.active_tab == WatchlistModalTab::Overview
            {
                apply_key(&mut self.watchlist_modal.search_editing, key);
                // Keep search_query in sync with search_editing.text
                self.watchlist_modal.search_query = self.watchlist_modal.search_editing.text.clone();
                self.watchlist_modal.scroll_offset = 0.0;
                return;
            }
        }

        // 0a. Watchlist group name input modal
        if self.wl_group_name_input.is_open() {
            apply_key(&mut self.wl_group_name_input.editing, key);
            return;
        }

        // 0. Preset name input modal
        if self.panel_app.preset_name_input.is_open {
            apply_key(&mut self.panel_app.preset_name_input.editing, key);
            return;
        }

        // 0c. Primitive settings template name modal
        if self.panel_app.primitive_settings_state.save_template_mode {
            if let Some(ref mut editing) = self.panel_app.primitive_settings_state.template_name_editing {
                apply_key(editing, key);
                return;
            }
        }

        // 0d. Indicator settings template name modal
        if self.panel_app.indicator_settings_state.save_template_mode {
            if let Some(ref mut editing) = self.panel_app.indicator_settings_state.template_name_editing {
                apply_key(editing, key);
                return;
            }
        }

        // 0e. Compare settings template name modal
        if self.panel_app.compare_settings_state.save_template_mode {
            if let Some(ref mut editing) = self.panel_app.compare_settings_state.template_name_editing {
                apply_key(editing, key);
                return;
            }
        }

        // 0f. Chart settings template name modal
        if self.panel_app.chart_settings_state.save_template_mode {
            if let Some(ref mut editing) = self.panel_app.chart_settings_state.template_name_editing {
                apply_key(editing, key);
                return;
            }
        }

        // 1. Primitive settings text editing
        if let Some(ref mut editing) = self.panel_app.primitive_settings_state.editing_text {
            apply_key(editing, key);
            return;
        }

        // 2. Indicator settings text editing
        if let Some(ref mut editing) = self.panel_app.indicator_settings_state.editing_text_state {
            apply_key(editing, key);
            return;
        }

        // 3. Chart settings inline text editing
        if let Some(ref mut editing) = self.panel_app.chart_settings_state.editing_text {
            apply_key(editing, key);
            return;
        }

        // 3b. Compare settings tf_min/max text editing
        if self.panel_app.compare_settings_state.is_open() {
            if let Some(ref mut editing) = self.panel_app.compare_settings_state.editing_text {
                apply_key(editing, key);
                return;
            }
        }

        // 4. Search modal input
        if let Some(ref mut editing) = self.modal_state.editing_text {
            apply_key(editing, key);
            // Sync search_query with the updated editing text so that subsequent
            // renders and search-result filtering reflect the latest content.
            // (Paste and Delete via apply_key modify editing.text directly —
            // without this sync the displayed results would be stale.)
            self.modal_state.search_query = editing.text.clone();
            // Rebuild symbol search results if the text changed (Paste / Delete).
            if self.modal_state.current == OpenModal::SymbolSearch
                || self.modal_state.current == OpenModal::CompareSearch
            {
                let q = self.modal_state.search_query.clone();
                self.modal_state.symbol_search_results =
                    crate::ChartApp::build_demo_symbol_results(&q, &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
            }
            return;
        }
    }

    /// Return the currently selected text from whichever text field is active,
    /// or `None` if no text is selected.
    ///
    /// The platform runner calls this on Ctrl+C to obtain text to place on the
    /// system clipboard.
    pub fn on_copy_selection(&self) -> Option<String> {
        fn get_selection(editing: &zengeld_chart::ui::modal_settings::TextEditingState) -> Option<String> {
            let (start, end) = editing.selection_range()?;
            let start_byte = editing.char_to_byte_pos(start);
            let end_byte = editing.char_to_byte_pos(end);
            Some(editing.text[start_byte..end_byte].to_string())
        }

        // Chart browser search
        if self.panel_app.chart_browser.is_open {
            return get_selection(&self.panel_app.chart_browser.search_editing);
        }

        // Watchlist group name input modal
        if self.wl_group_name_input.is_open() {
            return get_selection(&self.wl_group_name_input.editing);
        }

        // Preset name input modal
        if self.panel_app.preset_name_input.is_open {
            return get_selection(&self.panel_app.preset_name_input.editing);
        }

        // Primitive settings text editing
        if let Some(ref editing) = self.panel_app.primitive_settings_state.editing_text {
            return get_selection(editing);
        }

        // Indicator settings text editing
        if let Some(ref editing) = self.panel_app.indicator_settings_state.editing_text_state {
            return get_selection(editing);
        }

        // Chart settings inline text editing
        if let Some(ref editing) = self.panel_app.chart_settings_state.editing_text {
            return get_selection(editing);
        }

        // Watchlist modal search input (Overview tab only)
        {
            use zengeld_chart::ui::modal_settings::WatchlistModalTab;
            if self.watchlist_modal.is_open()
                && self.watchlist_modal.active_tab == WatchlistModalTab::Overview
            {
                return get_selection(&self.watchlist_modal.search_editing);
            }
        }

        // Search modal input (symbol search, indicator search, compare search)
        if let Some(ref editing) = self.modal_state.editing_text {
            return get_selection(editing);
        }

        None
    }

    // -------------------------------------------------------------------------
    // Undo/Redo command application
    // -------------------------------------------------------------------------

    /// Apply a `Command` to the active window's drawing manager / viewport.
    ///
    /// Used by undo (apply `cmd.inverse()`) and redo (apply `cmd` directly).
    ///
    /// When the active window belongs to a SyncGroup, primitive-mutating commands
    /// operate on `group.primitives` (the group's authoritative list) rather than
    /// the window's local `drawing_manager`, which only holds a render-cache copy.
    pub(crate) fn apply_command_to_active_window(&mut self, cmd: &zengeld_chart::Command) {
        use zengeld_chart::Command;

        // Determine up front whether we are operating in grouped or standalone mode.
        // We must not hold a borrow on panel_grid across the mutable tag_manager calls below.
        let group_id = self.panel_app.panel_grid.active_window().and_then(|w| w.group_id);

        match cmd {
            Command::CreatePrimitive { index, type_id, points, data } => {
                if let Some(gid) = group_id {
                    // Grouped: insert into the group's primitive list using the registry.
                    if let Ok(reg) = zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                        if let Some(mut prim) = reg.create(type_id, points, Some(&data.color.stroke)) {
                            *prim.data_mut() = data.clone();
                            if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                                let insert_pos = (*index).min(group.primitives.len());
                                group.primitives.insert(insert_pos, prim);
                            }
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.insert_at(*index, type_id, points, data);
                    }
                }
            }
            Command::DeletePrimitive { index, .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if *index < group.primitives.len() {
                            group.primitives.remove(*index);
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.delete_at(*index);
                    }
                }
            }
            Command::DeleteAllPrimitives { .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.primitives.clear();
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.clear();
                    }
                }
            }
            Command::RestoreAllPrimitives { primitives } => {
                if let Some(gid) = group_id {
                    if let Ok(reg) = zengeld_chart::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                        if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                            group.primitives.clear();
                            for (type_id, points, data) in primitives {
                                if let Some(mut prim) = reg.create(type_id, points, Some(&data.color.stroke)) {
                                    *prim.data_mut() = data.clone();
                                    group.primitives.push(prim);
                                }
                            }
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.clear();
                        for (i, (type_id, points, data)) in primitives.iter().enumerate() {
                            w.drawing_manager.insert_at(i, type_id, points, data);
                        }
                    }
                }
            }
            Command::MovePrimitive { index, new_points, .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if let Some(prim) = group.primitives.get_mut(*index) {
                            prim.set_points(new_points);
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.set_points_at(*index, new_points);
                    }
                }
            }
            Command::SetPrimitiveVisibility { index, visible, .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if let Some(prim) = group.primitives.get_mut(*index) {
                            prim.data_mut().visible = *visible;
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.set_visibility_at(*index, *visible);
                    }
                }
            }
            Command::SetPrimitiveLock { index, locked, .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if let Some(prim) = group.primitives.get_mut(*index) {
                            prim.data_mut().locked = *locked;
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.set_lock_at(*index, *locked);
                    }
                }
            }
            Command::ModifyPrimitiveData { index, new_data, .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if let Some(prim) = group.primitives.get_mut(*index) {
                            *prim.data_mut() = new_data.clone();
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.set_data_at(*index, new_data);
                    }
                }
            }
            Command::ReorderPrimitive { old_index, new_index } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if *old_index < group.primitives.len() {
                            let prim = group.primitives.remove(*old_index);
                            let insert_pos = (*new_index).min(group.primitives.len());
                            group.primitives.insert(insert_pos, prim);
                        }
                    }
                } else {
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        w.drawing_manager.move_to_index(*old_index, *new_index);
                    }
                }
            }
            // Non-primitive commands: viewport, symbol, timeframe, chart type, indicators.
            // These always act on the window directly regardless of group membership.
            Command::ViewportChange { new, .. } => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.viewport.view_start = new.view_start;
                    window.viewport.bar_spacing = new.bar_spacing;
                    window.price_scale.price_min = new.price_min;
                    window.price_scale.price_max = new.price_max;
                }
            }
            Command::ChangeChartType { new_type, .. } => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    // chart_type is &'static str; leak the string to produce a 'static reference.
                    let static_str: &'static str = Box::leak(new_type.clone().into_boxed_str());
                    window.chart_type = static_str;
                }
            }
            Command::ChangeSymbol { new_symbol, .. } => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.change_symbol(new_symbol);
                    eprintln!("[Undo/Redo] Changed symbol to {}", new_symbol);
                }
            }
            Command::ChangeTimeframe { new_timeframe, .. } => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.change_timeframe(new_timeframe.clone());
                    eprintln!("[Undo/Redo] Changed timeframe to {:?}", new_timeframe);
                }
            }
            Command::AddCompareSeries { series } => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    if !w.compare_overlay.has_symbol(&series.symbol) {
                        w.compare_overlay.add_series(series.clone());
                    }
                }
            }
            Command::RemoveCompareSeries { symbol, .. } => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    if w.compare_overlay.has_symbol(symbol) {
                        w.compare_overlay.remove_series_by_symbol(symbol);
                    }
                }
            }
            Command::SetCompareSeriesVisibility { symbol, visible, .. } => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(s) = w.compare_overlay.get_series_mut(symbol) {
                        s.visible = *visible;
                    }
                }
            }
            Command::SetCompareSeriesColor { symbol, color, .. } => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(s) = w.compare_overlay.get_series_mut(symbol) {
                        s.color = color.clone();
                    }
                }
            }
            Command::ClearAllCompareSeries { .. } => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.compare_overlay.clear();
                }
            }
            Command::RestoreAllCompareSeries { series } => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.compare_overlay.clear();
                    for s in series {
                        if !w.compare_overlay.has_symbol(&s.symbol) {
                            w.compare_overlay.add_series(s.clone());
                        }
                    }
                    eprintln!("[Undo/Redo] Restored {} compare series", series.len());
                }
            }
            Command::AddIndicator { instance_id, type_id, .. } => {
                let (symbol, bars_snapshot, chart_id_val) = match self.panel_app.panel_grid.active_window() {
                    Some(w) => (w.symbol.clone(), w.bars.clone(), w.id),
                    None => return,
                };
                if self.indicator_manager.create_instance_with_id(*instance_id, type_id, &symbol) {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(*instance_id) {
                        inst.window_id = Some(chart_id_val.0);
                    }
                    self.indicator_manager.calculate(*instance_id, &bars_snapshot);
                    self.sync_sub_panes_from_manager();
                    eprintln!("[Undo/Redo] Re-created indicator {} (id={}) window_id={}", type_id, instance_id, chart_id_val.0);
                } else {
                    eprintln!("[Undo/Redo] Failed to re-create indicator {} (id={})", type_id, instance_id);
                }
            }
            Command::RemoveIndicator { instance_id, .. } => {
                if self.indicator_manager.remove_instance(*instance_id).is_some() {
                    self.sync_sub_panes_from_manager();
                    eprintln!("[Undo/Redo] Removed indicator instance {}", instance_id);
                } else {
                    eprintln!("[Undo/Redo] Indicator instance {} not found", instance_id);
                }
            }
            other => {
                eprintln!("[Undo/Redo] Unhandled command: {}", other.description());
            }
        }
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Build an `ExtendedFrameLayout` for the chart content area.
    ///
    /// Uses the toolbar-offset content rect (after carving out the 50 px left
    /// drawing toolbar and 40 px top control strip) so that hit-test
    /// coordinates match the coordinates used during rendering.
    /// This method only borrows `self` immutably and returns an owned value,
    /// so callers can immediately follow with a mutable borrow.
    pub(crate) fn build_extended_layout(&self) -> ExtendedFrameLayout {
        let w = self.width as f64;
        let h = self.height as f64;
        // Mirror render(): full window width (right toolbar stays at edge),
        // then shrink content_rect by sidebar width.
        let sidebar_w = self.sidebar_state.right_width();
        let window_rect = LayoutRect::new(0.0, 0.0, w, h);

        // Carve out toolbar areas to get the same content rect used in render().
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);
        let mut content_rect = panel_layout.content_rect;
        // Sidebar sits between content and right toolbar — shrink content.
        content_rect.width = (content_rect.width - sidebar_w).max(0.0);

        // In split mode, use the active leaf's absolute rect instead of the
        // full content area.  This ensures chart_rect.x/y in the resulting
        // layout match the leaf the cursor is in, so coordinate conversions
        // (screen → chart-local) are correct for hit-tests and drawing.
        if self.panel_app.panel_grid.is_split() {
            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                if let Some(leaf_rect) = self.get_leaf_absolute_rect(active_leaf) {
                    content_rect = leaf_rect;
                }
            }
        }

        let scale_settings = self.panel_app.panel_grid
            .active_window()
            .map(|win| win.scale_settings.clone())
            .unwrap_or_default();

        // Collect sub-pane instance IDs so that ExtendedFrameLayout matches
        // the one computed during render (render_full_chart_panel).  Without
        // these, main_chart height is wrong and coordinate transforms break.
        // Scope the query to the active chart window to avoid counting
        // indicators from other split panes (which would double the sub_pane
        // count and offset the crosshair).
        let active_chart_id = self.panel_app.panel_grid.active_chart_id();
        let sub_pane_ids: Vec<u64> = self.panel_app.panel_grid
            .active_window()
            .map(|win| {
                let symbol = &win.symbol;
                if let Some(cid) = active_chart_id {
                    self.indicator_manager
                        .get_instances_for_symbol_in_window(symbol, cid.0)
                } else {
                    self.indicator_manager
                        .get_instances_for_symbol(symbol)
                }
                .into_iter()
                .filter(|i| i.visible && i.pane > 0)
                .map(|i| i.id)
                .collect()
            })
            .unwrap_or_default();

        // sub_pane_height=100.0 and separator_height=1.0 must match the
        // values used in render_full_chart_panel (render_chart.rs:2205-2206).
        ExtendedFrameLayout::compute_from_chart_panel(
            &content_rect,
            &sub_pane_ids,
            &scale_settings,
            100.0,  // sub_pane_height — height of each sub-pane, NOT viewport.chart_height
            1.0,    // separator_height
        )
    }

    /// Build an `ExtendedFrameLayout` for a specific split leaf.
    ///
    /// Unlike `build_extended_layout()` which uses the toolbar-carved content rect
    /// for the active window, this uses the leaf's own absolute rect as the
    /// layout area (minimal config = no toolbar carving).
    pub(crate) fn build_extended_layout_for_leaf(
        &self,
        leaf_id: zengeld_chart::LeafId,
        leaf_rect: &LayoutRect,
    ) -> Option<ExtendedFrameLayout> {
        let window = self.panel_app.panel_grid.window_for_leaf(leaf_id)?;
        let scale_settings = &window.scale_settings;

        // Collect sub-pane IDs for this leaf's window symbol, scoped to the
        // leaf's own chart window so that indicators from other split panes
        // are not counted (which would inflate sub_pane count and offset the
        // crosshair position).
        let chart_id = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id);
        let sub_pane_ids: Vec<u64> = if let Some(cid) = chart_id {
            self.indicator_manager
                .get_instances_for_symbol_in_window(&window.symbol, cid.0)
        } else {
            self.indicator_manager
                .get_instances_for_symbol(&window.symbol)
        }
        .into_iter()
        .filter(|i| i.visible && i.pane > 0)
        .map(|i| i.id)
        .collect();

        Some(ExtendedFrameLayout::compute_from_chart_panel(
            leaf_rect,
            &sub_pane_ids,
            scale_settings,
            100.0, // sub_pane_height — must match render_full_chart_panel
            1.0,   // separator_height
        ))
    }

    /// Convert a leaf's panel-relative rect to an absolute screen rect.
    ///
    /// `panel_rects()` returns rects relative to the content area top-left.
    /// This method adds `content_rect` as an offset so the result is in
    /// absolute screen coordinates, matching the coordinate space used by
    /// mouse events.
    pub(crate) fn get_leaf_absolute_rect(
        &self,
        leaf_id: zengeld_chart::LeafId,
    ) -> Option<LayoutRect> {
        let sub_rect = self.panel_app.panel_grid.panel_rects().get(&leaf_id)?;
        Some(LayoutRect {
            x: self.content_rect.x + sub_rect.x as f64,
            y: self.content_rect.y + sub_rect.y as f64,
            width: sub_rect.width as f64,
            height: sub_rect.height as f64,
        })
    }

    /// Hide the crosshair on all split leaves.
    fn hide_all_split_crosshairs(&mut self) {
        let leaf_ids: Vec<zengeld_chart::LeafId> = self.panel_app
            .panel_grid
            .panel_rects()
            .keys()
            .copied()
            .collect();
        for leaf_id in leaf_ids {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                window.crosshair.visible = false;
            }
        }
    }

    /// Dispatch a click that landed on a registered widget.
    ///
    /// Handles toolbar clicks and dropdown item selections by forwarding to
    /// the toolbar state's handler methods.  Modal widget clicks are logged
    /// but not fully wired — modal input routing would require a handle_input()
    /// on ChartPanelApp which does not exist at this checkpoint.
    fn dispatch_panel_click(&mut self, widget_id: &str, x: f64, y: f64) {
        // === Right sidebar widgets ===
        if widget_id == "right_sidebar_close" {
            if let Some((_closing, _width)) = self.sidebar_state.close_right() {
                eprintln!("[ChartApp] Sidebar closed via close button");
            }
            return;
        }

        // === Watchlist panel header buttons ===

        if widget_id == "watchlist_open_modal" {
            self.watchlist_modal.open();
            eprintln!("[Sidebar] Watchlist expand button clicked — opening watchlist modal");
            return;
        }

        if widget_id == "watchlist_column_config" {
            // Toggle the column-config dropdown panel inside the sidebar.
            self.sidebar_state.watchlist_config_dropdown_open =
                !self.sidebar_state.watchlist_config_dropdown_open;
            eprintln!("[Sidebar] Watchlist column-config dropdown toggled: {}",
                self.sidebar_state.watchlist_config_dropdown_open);
            return;
        }

        if widget_id == "watchlist_sort_color" {
            // Cycle through 3 sort modes: 0 = no sort, 1 = red first, 2 = gray first.
            self.sidebar_state.watchlist_sort_mode =
                (self.sidebar_state.watchlist_sort_mode + 1) % 3;
            let mode = self.sidebar_state.watchlist_sort_mode;
            eprintln!("[Sidebar] watchlist_sort_mode -> {}", mode);
            if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                match mode {
                    0 => {
                        // Reset to original order from snapshot.
                        if let Some(snap) = list.order_snapshot.take() {
                            list.ungrouped = snap;
                        }
                        eprintln!("[Sidebar] Watchlist sort reset to original order");
                    }
                    1 | 2 => {
                        // Save snapshot if not already saved (first sort from unsorted state).
                        if list.order_snapshot.is_none() {
                            list.order_snapshot = Some(list.ungrouped.clone());
                        }
                        // Always sort from the snapshot base so switching between modes
                        // 1 and 2 does not compound on an already-sorted order.
                        let base = list.order_snapshot.as_ref().unwrap().clone();
                        let color_order: &[&str] = &[
                            "#ef5350", "#f59e0b", "#22c55e", "#3b82f6",
                            "#a855f7", "#ec4899", "#6b7280",
                        ];
                        let mut symbols = base;
                        symbols.sort_by(|a, b| {
                            let a_idx = list.get_color_flag(&a.symbol, &a.exchange)
                                .and_then(|f| color_order.iter().position(|c| *c == f))
                                .unwrap_or(99);
                            let b_idx = list.get_color_flag(&b.symbol, &b.exchange)
                                .and_then(|f| color_order.iter().position(|c| *c == f))
                                .unwrap_or(99);
                            if mode == 1 {
                                a_idx.cmp(&b_idx)
                            } else {
                                b_idx.cmp(&a_idx)
                            }
                        });
                        list.ungrouped = symbols;
                        let label = if mode == 1 { "red first" } else { "gray first" };
                        eprintln!("[Sidebar] Watchlist sorted: flagged first ({})", label);
                    }
                    _ => {}
                }
            }
            self.watchlist_actions.push(crate::WatchlistAction::SortCycle);
            return;
        }

        // Backdrop click inside the dropdown — just consume the event (keeps dropdown open).
        if widget_id == "watchlist_cfg_backdrop" {
            return;
        }

        // Column config checkbox toggles.
        if let Some(field) = widget_id.strip_prefix("watchlist_cfg:") {
            // Strip the "show_" prefix to match the action enum keys used by
            // about_to_wait() in main.rs (e.g. "show_volume" → "volume").
            let column_key = field.strip_prefix("show_").unwrap_or(field);
            eprintln!("[Sidebar] watchlist column toggled: {}", field);
            self.watchlist_actions.push(crate::WatchlistAction::ToggleColumnVisibility { column: column_key.to_string() });
            self.watchlist_actions.push(crate::WatchlistAction::ResetSeparatorOffsets);
            self.watchlists_dirty = true;
            return;
        }

        // Any other click while the dropdown is open closes it.
        if self.sidebar_state.watchlist_config_dropdown_open {
            self.sidebar_state.watchlist_config_dropdown_open = false;
        }

        // Close color picker on any click that isn't on the picker or its flag.
        if self.sidebar_state.watchlist_color_picker_open.is_some()
            && !widget_id.starts_with("watchlist_color_")
            && !widget_id.starts_with("watchlist_flag_")
        {
            self.sidebar_state.watchlist_color_picker_open = None;
        }

        // === Watchlist panel clicks ===

        // Pattern: "watchlist_flag_{index}" — left-edge flag stripe click.
        // Toggles the color picker popup for that row.
        if widget_id.starts_with("watchlist_flag_") {
            if let Some(idx) = widget_id.strip_prefix("watchlist_flag_").and_then(|s| s.parse::<usize>().ok()) {
                // Toggle: close if same row already open, otherwise open.
                if self.sidebar_state.watchlist_color_picker_open.map(|(i, _, _)| i) == Some(idx) {
                    self.sidebar_state.watchlist_color_picker_open = None;
                } else {
                    if let Some(ref sidebar_result) = self.last_sidebar_result {
                        if let Some((_, row_rect)) = sidebar_result.watchlist_row_rects.get(idx) {
                            let popup_x = row_rect.x;
                            let popup_y = row_rect.y + row_rect.height;
                            self.sidebar_state.watchlist_color_picker_open = Some((idx, popup_x, popup_y));
                        }
                    }
                }
            }
            return;
        }

        // Pattern: "watchlist_color_{row}_{ci}" — swatch click in the color picker.
        if widget_id.starts_with("watchlist_color_") {
            if let Some(rest) = widget_id.strip_prefix("watchlist_color_") {
                let parts: Vec<&str> = rest.split('_').collect();
                if parts.len() == 2 {
                    if let (Ok(row_idx), Ok(color_idx)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                        let colors: &[&str] = &["#ef5350", "#f59e0b", "#22c55e", "#3b82f6", "#a855f7", "#ec4899", "#6b7280", ""];
                        if let Some(item) = self.sidebar_state.watchlist_items.get(row_idx) {
                            let symbol = item.symbol.clone();
                            let exchange = item.exchange.clone();
                            if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                                let color = colors.get(color_idx).copied().unwrap_or("");
                                list.set_color_flag(&symbol, &exchange, color);
                                let color_opt = if color.is_empty() { None } else { Some(color.to_string()) };
                                self.watchlist_actions.push(crate::WatchlistAction::SetColorFlag { symbol: symbol.clone(), exchange: exchange.clone(), color: color_opt });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[Sidebar] Color flag set: {}:{} = {:?}", symbol, exchange, color);
                            }
                        }
                        self.sidebar_state.watchlist_color_picker_open = None;
                    }
                }
            }
            return;
        }

        // Pattern: "watchlist_delete_{index}" — delete button on a row.
        // Must be checked BEFORE the generic "watchlist_" prefix so it doesn't
        // fall through to the row-click handler.
        if widget_id.starts_with("watchlist_delete_") {
            if let Some(idx) = widget_id.strip_prefix("watchlist_delete_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(item) = self.sidebar_state.watchlist_items.get(idx) {
                    let symbol = item.symbol.clone();
                    let exchange = item.exchange.clone();
                    // Remove symbol from snapshot (if active) before removing from list.
                    if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                        if let Some(ref mut snap) = list.order_snapshot {
                            snap.retain(|s| !(s.symbol == symbol && s.exchange == exchange));
                        }
                    }
                    self.sidebar_state.watchlist_manager.remove_symbol(&symbol, &exchange);
                    self.watchlist_actions.push(crate::WatchlistAction::Remove { symbol: symbol.clone(), exchange: exchange.clone() });
                    self.watchlist_actions.push(crate::WatchlistAction::ClearOrderSnapshot);
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[Sidebar] Watchlist delete: {} @ {} ({})", symbol, exchange, idx);
                }
            }
            return;
        }

        // Pattern: "watchlist_{index}" — row click (switch symbol).
        if widget_id.starts_with("watchlist_") {
            if let Some(idx) = widget_id.strip_prefix("watchlist_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(item) = self.sidebar_state.watchlist_items.get(idx) {
                    let symbol = item.symbol.clone();
                    let item_exchange = item.exchange.clone();
                    eprintln!("[Sidebar] Watchlist click: {} @ {} ({})", symbol, item_exchange, idx);

                    // Resolve ExchangeId from the item's exchange string.
                    let resolved_exchange = self.exchange_symbols
                        .keys()
                        .find(|eid| eid.as_str() == item_exchange)
                        .copied()
                        .unwrap_or(self.active_exchange);

                    let prev_symbol = self.panel_app.panel_grid.active_window()
                        .map(|w| w.symbol.clone());
                    let prev_exchange = self.panel_app.panel_grid.active_window()
                        .map(|w| w.exchange.clone());
                    if prev_symbol.as_deref() != Some(&symbol) || prev_exchange.as_deref() != Some(&item_exchange) {
                        let timeframe = self.panel_app.panel_grid.active_window()
                            .map(|w| w.timeframe.clone())
                            .unwrap_or_default();
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let old_sym = window.symbol.clone();
                            window.snapshot_drawings_for_symbol(&old_sym);
                            window.symbol = symbol.clone();
                            window.exchange = item_exchange.clone();
                            window.update_title();
                            window.bars.clear();
                            window.drawing_manager.clear_all_primitives();
                            window.restore_drawings_for_symbol(&symbol);
                        }
                        self.active_exchange = resolved_exchange;
                        self.bridge.unsubscribe_all();
                        let eid_str = resolved_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (watchlist sidebar click)", eid_str);
                        } else {
                            self.bridge.ensure_connector(resolved_exchange);
                            self.bridge.request_bars(resolved_exchange, &symbol, &timeframe, None, Some(self.panel_app.user_manager.profile.bar_count as usize));
                        }
                        self.autosave_snapshot();
                    }
                }
            }
            return;
        }

        // === Connector panel clicks ===
        // Pattern: "connector_row:{exchange_id}" — toggle expand/collapse
        if widget_id.starts_with("connector_row:") {
            if let Some(exchange_id) = widget_id.strip_prefix("connector_row:") {
                let expanded = self.sidebar_state.connector_expanded.entry(exchange_id.to_string()).or_insert(false);
                *expanded = !*expanded;
                eprintln!("[Sidebar] Connector row toggled: {} -> expanded={}", exchange_id, *expanded);
            }
            return;
        }

        // Pattern: "connector_toggle:{exchange_id}" — toggle enabled/disabled dot
        if widget_id.starts_with("connector_toggle:") {
            if let Some(exchange_id_str) = widget_id.strip_prefix("connector_toggle:") {
                let enabled = self.sidebar_state.connector_enabled
                    .entry(exchange_id_str.to_string()).or_insert(true);
                *enabled = !*enabled;
                let now_enabled = *enabled;
                self.connector_actions.push(crate::ConnectorAction::ToggleEnabled { exchange_id: exchange_id_str.to_string() });

                if let Some(eid) = digdigdig3::ExchangeId::from_str(exchange_id_str) {
                    if now_enabled {
                        self.bridge.enable_connector(eid);
                        eprintln!("[Sidebar] Connector enabled: {}", exchange_id_str);
                    } else {
                        self.bridge.disable_connector(eid);
                        eprintln!("[Sidebar] Connector disabled: {}", exchange_id_str);
                    }
                } else {
                    eprintln!("[Sidebar] Connector toggle: {} -> enabled={}", exchange_id_str, now_enabled);
                }
                self.persist_profile();
            }
            return;
        }

        // Pattern: "connector_metrics:{exchange_id}" — toggle metrics section visibility
        if widget_id.starts_with("connector_metrics:") {
            if let Some(exchange_id) = widget_id.strip_prefix("connector_metrics:") {
                let visible = self.sidebar_state.connector_metrics_visible
                    .entry(exchange_id.to_string()).or_insert(false);
                *visible = !*visible;
                eprintln!("[Sidebar] Connector metrics toggled: {} -> visible={}", exchange_id, *visible);
            }
            return;
        }

        // Pattern: "connector_group:{group_label}" — toggle group collapse state
        if widget_id.starts_with("connector_group:") {
            if let Some(group_label) = widget_id.strip_prefix("connector_group:") {
                let collapsed = self.sidebar_state.connector_group_collapsed
                    .entry(group_label.to_string())
                    .or_insert(false);
                *collapsed = !*collapsed;
                eprintln!("[Sidebar] Connector group toggled: {} -> collapsed={}", group_label, *collapsed);
            }
            return;
        }

        // === Performance panel control clicks — cycle through values ===
        if widget_id == "perf:backend" {
            use sidebar_content::state::RenderBackend;
            let current = &self.sidebar_state.performance_data.render_backend;
            let all = RenderBackend::all();
            let idx = all.iter().position(|b| b == current).unwrap_or(0);
            let next = all[(idx + 1) % all.len()].clone();
            eprintln!("[ChartApp] perf: backend -> {}", next.label());
            self.perf_actions.push(crate::PerfAction::SetBackend(next.label().to_string()));
            return;
        }
        if widget_id == "perf:vsync" {
            self.perf_actions.push(crate::PerfAction::ToggleVsync);
            return;
        }
        if widget_id == "perf:fps_limit" {
            // Cycle: 30 → 60 → 120 → 0 (unlimited) → 30
            let current = self.sidebar_state.performance_data.fps_limit;
            let next = match current {
                30 => 60,
                60 => 120,
                120 => 0,
                _ => 30,
            };
            self.perf_actions.push(crate::PerfAction::SetFpsLimit(next));
            eprintln!("[ChartApp] perf: FPS limit -> {}", if next == 0 { "unlimited".to_string() } else { format!("{}", next) });
            return;
        }
        if widget_id == "perf:msaa" {
            // Cycle: 16 → 8 → 4 → 0 (off) → 16
            let current = self.sidebar_state.performance_data.msaa_samples;
            let next = match current {
                16 => 8,
                8 => 4,
                4 => 0,
                _ => 16,
            };
            self.perf_actions.push(crate::PerfAction::SetMsaa(next));
            eprintln!("[ChartApp] perf: MSAA -> {}", if next == 0 { "off".to_string() } else { format!("{}x", next) });
            return;
        }
        if widget_id == "perf:max_bars" {
            // Cycle: 0 (unlimited) → 2000 → 5000 → 10000 → 0
            let current = self.sidebar_state.performance_data.max_bars;
            let next = match current {
                0 => 2000,
                2000 => 5000,
                5000 => 10000,
                _ => 0,
            };
            self.perf_actions.push(crate::PerfAction::SetMaxBars(next));
            eprintln!("[ChartApp] perf: max bars -> {}", if next == 0 { "unlimited".to_string() } else { format!("{}", next) });
            return;
        }
        if widget_id == "perf:recalc_mode" {
            // Cycle: PerFrame → PerBar → PerTick → PerFrame
            let current = self.sidebar_state.performance_data.recalc_mode.clone();
            let next = match current.as_str() {
                "PerFrame" => "PerBar",
                "PerBar" => "PerTick",
                _ => "PerFrame",
            };
            self.perf_actions.push(crate::PerfAction::SetRecalcMode(next.to_string()));
            eprintln!("[ChartApp] perf: recalc mode -> {}", next);
            return;
        }
        if widget_id == "perf:log_toggle" {
            self.perf_actions.push(crate::PerfAction::TogglePerfLog);
            return;
        }

        // === Alert panel clicks ===
        // Pattern: "alert_delete_{id}"
        if widget_id.starts_with("alert_delete_") {
            if let Some(id) = widget_id.strip_prefix("alert_delete_").and_then(|s| s.parse::<u64>().ok()) {
                self.alert_manager.remove(id);
                eprintln!("[Sidebar] Alert deleted: {}", id);
                self.sidebar_data_dirty = true;
                self.autosave_snapshot();
            }
            return;
        }
        // Pattern: "alert_add_button" — open alert settings modal for new alert
        if widget_id == "alert_add_button" {
            let price = self.panel_app.panel_grid.active_window()
                .and_then(|w| w.bars.last())
                .map(|b| b.close)
                .unwrap_or(0.0);
            let symbol = self.panel_app.panel_grid.active_window()
                .map(|w| w.symbol.clone())
                .unwrap_or_else(|| "BTCUSD".to_string());
            let source = alerts::AlertSource::Price { symbol: symbol.clone() };
            self.panel_app.alert_settings_state.open_new(source, &symbol, price);
            self.panel_app.alert_settings_state.pin_initial_position(
                self.content_rect.width, self.content_rect.height,
            );
            eprintln!("[Sidebar] Alert settings modal opened (new, price={:.2})", price);
            return;
        }
        // Pattern: "alert_{id}" (row click — open edit modal, numeric id only)
        if let Some(id) = widget_id.strip_prefix("alert_").and_then(|s| s.parse::<u64>().ok()) {
            if let Some(alert) = self.alert_manager.get(id) {
                self.panel_app.alert_settings_state.open_edit(alert);
                self.panel_app.alert_settings_state.pin_initial_position(
                    self.content_rect.width, self.content_rect.height,
                );
                eprintln!("[Sidebar] Alert edit opened: id={}", id);
            }
            return;
        }

        // === Object tree panel clicks ===
        // Widget IDs use "drw_" prefix for drawings, "ind_" for indicators.
        // Buttons: {prefix}_delete_{id}, {prefix}_settings_{id},
        //          {prefix}_vis_{id}, {prefix}_lock_{id}
        // Row:     {prefix}_{id}

        // --- Drawing delete ---
        if widget_id.starts_with("drw_delete_") {
            if let Some(id) = widget_id.strip_prefix("drw_delete_").and_then(|s| s.parse::<u64>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        window.drawing_manager.remove(idx);
                    }
                }
                eprintln!("[Sidebar] Drawing deleted: {}", id);
            }
            return;
        }
        // --- Indicator delete ---
        if widget_id.starts_with("ind_delete_") {
            if let Some(id) = widget_id.strip_prefix("ind_delete_").and_then(|s| s.parse::<u64>().ok()) {
                self.indicator_manager.remove_instance(id);
                eprintln!("[Sidebar] Indicator deleted: {}", id);
            }
            return;
        }
        // --- Drawing settings ---
        if widget_id.starts_with("drw_settings_") {
            if let Some(id) = widget_id.strip_prefix("drw_settings_").and_then(|s| s.parse::<u64>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        self.panel_app.primitive_settings_state.open(idx);
                        eprintln!("[Sidebar] Primitive settings opened: {} (idx={})", id, idx);
                    }
                }
            }
            return;
        }
        // --- Indicator settings ---
        if widget_id.starts_with("ind_settings_") {
            if let Some(id) = widget_id.strip_prefix("ind_settings_").and_then(|s| s.parse::<u64>().ok()) {
                self.panel_app.indicator_settings_state.open(id);
                eprintln!("[Sidebar] Indicator settings opened: {}", id);
            }
            return;
        }
        // --- Drawing visibility ---
        if widget_id.starts_with("drw_vis_") {
            if let Some(id) = widget_id.strip_prefix("drw_vis_").and_then(|s| s.parse::<u64>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        let v = window.drawing_manager.primitives_mut()[idx].data().visible;
                        window.drawing_manager.primitives_mut()[idx].data_mut().visible = !v;
                    }
                }
                eprintln!("[Sidebar] Drawing visibility toggled: {}", id);
            }
            return;
        }
        // --- Indicator visibility ---
        if widget_id.starts_with("ind_vis_") {
            if let Some(id) = widget_id.strip_prefix("ind_vis_").and_then(|s| s.parse::<u64>().ok()) {
                self.indicator_manager.toggle_visibility(id);
                eprintln!("[Sidebar] Indicator visibility toggled: {}", id);
            }
            return;
        }
        // --- Drawing lock ---
        if widget_id.starts_with("drw_lock_") {
            if let Some(id) = widget_id.strip_prefix("drw_lock_").and_then(|s| s.parse::<u64>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        let l = window.drawing_manager.primitives_mut()[idx].data().locked;
                        window.drawing_manager.primitives_mut()[idx].data_mut().locked = !l;
                    }
                }
                eprintln!("[Sidebar] Drawing lock toggled: {}", id);
            }
            return;
        }
        // --- Indicator lock (no-op for now, indicators don't have lock) ---
        if widget_id.starts_with("ind_lock_") {
            return;
        }

        // === Compare overlay object tree handlers ===
        // Widget IDs use "cmp_" prefix. The numeric suffix is the series index.

        // --- Compare delete ---
        if widget_id.starts_with("cmp_delete_") {
            if let Some(idx) = widget_id.strip_prefix("cmp_delete_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(symbol) = window.compare_overlay.remove_series(idx) {
                        eprintln!("[Sidebar] Compare series removed: {} (idx={})", symbol, idx);
                    }
                }
            }
            return;
        }
        // --- Compare visibility toggle ---
        if widget_id.starts_with("cmp_vis_") {
            if let Some(idx) = widget_id.strip_prefix("cmp_vis_").and_then(|s| s.parse::<usize>().ok()) {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    let new_vis = window.compare_overlay.toggle_visibility(idx);
                    eprintln!("[Sidebar] Compare visibility toggled: idx={} -> visible={}", idx, new_vis);
                }
            }
            return;
        }
        // --- Compare settings / alert / lock (no-op stubs) ---
        if widget_id.starts_with("cmp_settings_") || widget_id.starts_with("cmp_alert_") || widget_id.starts_with("cmp_lock_") {
            return;
        }
        // --- Compare row click (row selection) ---
        {
            let cmp_id = widget_id
                .strip_prefix("cmp_")
                .and_then(|s| s.parse::<u64>().ok());
            if let Some(id) = cmp_id {
                if self.sidebar_state.object_tree_items.iter().any(|item| {
                    item.id == id && item.category == zengeld_chart::ObjectCategory::Compare
                }) {
                    for item in &mut self.sidebar_state.object_tree_items {
                        item.selected = item.id == id && item.category == zengeld_chart::ObjectCategory::Compare;
                    }
                    eprintln!("[Sidebar] Compare series selected: idx={}", id);
                }
                return;
            }
        }

        // --- Drawing alert button ---
        if widget_id.starts_with("drw_alert_") {
            if let Some(id) = widget_id.strip_prefix("drw_alert_").and_then(|s| s.parse::<u64>().ok()) {
                let mut price = self.panel_app.panel_grid.active_window()
                    .and_then(|w| w.bars.last())
                    .map(|b| b.close)
                    .unwrap_or(0.0);
                let mut source_name = format!("Drawing {}", id);
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(idx) = window.drawing_manager.find_index_by_id(id) {
                        let prim = &window.drawing_manager.primitives()[idx];
                        source_name = prim.display_name().to_string();
                        let pts = prim.points();
                        if !pts.is_empty() {
                            price = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
                        }
                    }
                }
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone())
                    .unwrap_or_else(|| "BTCUSD".to_string());
                let source = alerts::AlertSource::Drawing { primitive_id: id, label: source_name.clone() };
                self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                self.panel_app.alert_settings_state.pin_initial_position(
                    self.content_rect.width, self.content_rect.height,
                );
                eprintln!("[Sidebar] Alert settings opened for drawing {}", id);
            }
            return;
        }
        // --- Indicator alert button ---
        if widget_id.starts_with("ind_alert_") {
            if let Some(id) = widget_id.strip_prefix("ind_alert_").and_then(|s| s.parse::<u64>().ok()) {
                let price = self.panel_app.panel_grid.active_window()
                    .and_then(|w| w.bars.last())
                    .map(|b| b.close)
                    .unwrap_or(0.0);
                let source_name = self.indicator_manager.get_instance(id)
                    .map(|inst| inst.name.clone())
                    .unwrap_or_else(|| format!("Indicator {}", id));
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone())
                    .unwrap_or_else(|| "BTCUSD".to_string());
                let source = alerts::AlertSource::Indicator { indicator_id: id, output_index: 0, label: source_name.clone() };
                self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                self.panel_app.alert_settings_state.pin_initial_position(
                    self.content_rect.width, self.content_rect.height,
                );
                eprintln!("[Sidebar] Alert settings opened for indicator {}", id);
            }
            return;
        }
        // --- Alert bell on drawing primitive — opens the existing alert edit modal ---
        // Widget ID: "alert_bell_drw_{primitive_id}"
        if widget_id.starts_with("alert_bell_drw_") {
            if let Some(prim_id) = widget_id.strip_prefix("alert_bell_drw_").and_then(|s| s.parse::<u64>().ok()) {
                // Find the first Active alert bound to this primitive.
                let alert = self.alert_manager.items()
                    .iter()
                    .find(|a| matches!(
                        &a.source,
                        alerts::AlertSource::Drawing { primitive_id, .. } if *primitive_id == prim_id
                    ) && a.status == alerts::AlertStatus::Active)
                    .cloned();
                if let Some(alert) = alert {
                    self.panel_app.alert_settings_state.open_edit(&alert);
                    self.panel_app.alert_settings_state.pin_initial_position(
                        self.content_rect.width, self.content_rect.height,
                    );
                    eprintln!("[ChartApp] Bell click: opened edit modal for drawing alert id={}", alert.id);
                }
            }
            return;
        }
        // --- Alert bell on indicator — opens the existing alert edit modal ---
        // Widget ID: "alert_bell_ind_{indicator_id}"
        if widget_id.starts_with("alert_bell_ind_") {
            if let Some(ind_id) = widget_id.strip_prefix("alert_bell_ind_").and_then(|s| s.parse::<u64>().ok()) {
                let alert = self.alert_manager.items()
                    .iter()
                    .find(|a| matches!(
                        &a.source,
                        alerts::AlertSource::Indicator { indicator_id, .. } if *indicator_id == ind_id
                    ) && a.status == alerts::AlertStatus::Active)
                    .cloned();
                if let Some(alert) = alert {
                    self.panel_app.alert_settings_state.open_edit(&alert);
                    self.panel_app.alert_settings_state.pin_initial_position(
                        self.content_rect.width, self.content_rect.height,
                    );
                    eprintln!("[ChartApp] Bell click: opened edit modal for indicator alert id={}", alert.id);
                }
            }
            return;
        }
        // --- Row selection (drw_{id} or ind_{id}) ---
        // Only match if suffix after prefix is a pure number (avoids catching
        // "ind_overlay:toggle" and similar unrelated widget IDs).
        {
            let row_id = widget_id
                .strip_prefix("drw_")
                .or_else(|| widget_id.strip_prefix("ind_"))
                .and_then(|s| s.parse::<u64>().ok());
            if let Some(id) = row_id {
                if self.sidebar_state.object_tree_items.iter().any(|item| item.id == id) {
                    for item in &mut self.sidebar_state.object_tree_items {
                        item.selected = item.id == id;
                    }
                    eprintln!("[Sidebar] Object selected: {}", id);
                }
                return;
            }
        }

        // === Signal panel clicks ===
        // Pattern: "signal_{instance_id}_{bar_index}" (individual signal row — must come before signal_group_)
        if widget_id.starts_with("signal_") && !widget_id.starts_with("signal_group_") {
            let parts: Vec<&str> = widget_id.strip_prefix("signal_").unwrap_or("").splitn(2, '_').collect();
            if parts.len() == 2 {
                if let Ok(bar_index) = parts[1].parse::<i64>() {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let visible_bars = window.viewport.chart_width / window.viewport.bar_spacing;
                        window.viewport.view_start = (bar_index as f64 - visible_bars / 2.0).max(0.0);
                        if window.price_scale.scale_mode.is_auto_y() {
                            window.calc_auto_scale();
                        }
                    }
                    eprintln!("[Sidebar] Signal clicked: center on bar {}", bar_index);
                }
            }
            return;
        }
        // Pattern: "signal_group_{id}"
        if widget_id.starts_with("signal_group_") {
            if let Some(id) = widget_id.strip_prefix("signal_group_").and_then(|s| s.parse::<u64>().ok()) {
                if self.sidebar_state.collapsed_signal_groups.contains(&id) {
                    self.sidebar_state.collapsed_signal_groups.remove(&id);
                } else {
                    self.sidebar_state.collapsed_signal_groups.insert(id);
                }
                eprintln!("[Sidebar] Signal group toggled: {}", id);
            }
            return;
        }

        // === Toolbar widgets ===
        if widget_id.starts_with("toolbar:") || widget_id.starts_with("dtb:") || widget_id.starts_with("csb:") || widget_id.starts_with("btb:") || widget_id.starts_with("rtb:") || widget_id.starts_with("ilb:") {
            let item_id = widget_id
                .strip_prefix("toolbar:")
                .or_else(|| widget_id.strip_prefix("dtb:"))
                .or_else(|| widget_id.strip_prefix("csb:"))
                .or_else(|| widget_id.strip_prefix("btb:"))
                .or_else(|| widget_id.strip_prefix("rtb:"))
                .or_else(|| widget_id.strip_prefix("ilb:"))
                .unwrap_or(widget_id)
                .to_string();

            // Chevron scroll buttons — handle before generic toolbar dispatch.
            if item_id == "__chevron_left" || item_id == "__chevron_right" {
                let forward = item_id == "__chevron_right";
                // Map widget prefix to toolbar name and fetch max_scroll from last result.
                let (toolbar_name, max_scroll) = if widget_id.starts_with("csb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.top_max_scroll).unwrap_or(0.0);
                    ("top", ms)
                } else if widget_id.starts_with("btb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.bottom_max_scroll).unwrap_or(0.0);
                    ("bottom", ms)
                } else if widget_id.starts_with("dtb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.left_max_scroll).unwrap_or(0.0);
                    ("left", ms)
                } else if widget_id.starts_with("rtb:") {
                    let ms = self.last_toolbar_result.as_ref().map(|r| r.right_max_scroll).unwrap_or(0.0);
                    ("right", ms)
                } else {
                    return;
                };
                eprintln!("[ChartApp] chevron click: toolbar={} forward={} max_scroll={}", toolbar_name, forward, max_scroll);
                self.panel_app.toolbar_state.handle_chevron_click(toolbar_name, forward, max_scroll);
                return;
            }

            // Clock button — toggle clock popup
            if item_id == "clock" {
                // TODO: Clock popup not yet implemented in chart-app
                eprintln!("[ChartApp] Clock button clicked");
                return;
            }

            // Expand button — toggle expand mode in the split grid.
            if item_id == "expand" {
                self.panel_app.panel_grid.toggle_expand();
                eprintln!("[ChartApp] Expand button clicked, expanded={}", self.panel_app.panel_grid.is_expanded());
                return;
            }

            // chart_settings button opens/closes the chart settings modal directly.
            if item_id == "chart_settings" {
                self.panel_app.chart_settings_state.toggle();
                eprintln!("[ChartApp] chart_settings modal toggled: {}", self.panel_app.chart_settings_state.is_open);
                return;
            }

            // Undo/Redo — intercept before handle_toolbar_click_with_chart() which
            // returns Consumed without executing the command.  Command history lives
            // on ChartWindow, so we extract the command here and apply it.
            if item_id == "undo" {
                self.perform_undo();
                return;
            }
            if item_id == "redo" {
                self.perform_redo();
                return;
            }
            if item_id == "screenshot" {
                self.request_screenshot();
                return;
            }

            // Close inline dropdowns when clicking any toolbar button that is not
            // an inline dropdown item or the dropdown menu triggers themselves.
            if !item_id.starts_with("inline:style_option:")
                && !item_id.starts_with("inline:width_option:")
                && item_id != "inline:style_menu"
                && item_id != "inline:width_menu"
                && item_id != "inline_dropdown:__bg__"
            {
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            // inline:* actions come from the inline primitive toolbar embedded in
            // the control strip.  They must be intercepted here because
            // handle_toolbar_click_with_chart() does not know about them.
            if item_id.starts_with("inline:") || item_id == "inline_dropdown:__bg__" {
                self.handle_inline_action(&item_id);
                return;
            }

            // Split borrows: toolbar_state and panel_grid are separate fields of panel_app.
            // We can't split-borrow through panel_app, so we use a raw pointer for window.
            let window_ptr: *mut _ = self.panel_app.panel_grid.active_window_mut()
                .map(|w| w as *mut _)
                .unwrap_or(std::ptr::null_mut::<zengeld_chart::ChartWindow>());

            if !window_ptr.is_null() {
                // SAFETY: toolbar_state does not access panel_grid.
                let window = unsafe { &mut *window_ptr };
                let events = self.panel_app.toolbar_state.handle_toolbar_click_with_chart(
                    &item_id,
                    &mut window.crosshair,
                    &mut window.drawing_manager,
                );
                for event in events {
                    self.process_chart_out_event(event);
                }
            }
            return;
        }

        // === Dropdown items ===
        if let Some(rest) = widget_id.strip_prefix("dropdown:") {
            // Background click — close the dropdown without selecting any item.
            if rest == "__bg__" {
                self.panel_app.toolbar_state.open_dropdown_id = None;
                self.panel_app.toolbar_state.open_dropdown_position = None;
                return;
            }

            // Format: "dropdown:{dropdown_id}:{item_id}"
            let mut parts = rest.splitn(2, ':');
            if let (Some(dropdown_id), Some(item_id)) = (parts.next(), parts.next()) {
                // Noop items (disabled items, headers) use the "__noop__:" prefix.
                // The click is already consumed by reaching here (it didn't hit __bg__),
                // so we just return — the dropdown stays open with no action fired.
                if item_id.starts_with("__noop__:") {
                    return;
                }

                let dropdown_id = dropdown_id.to_string();
                let item_id = item_id.to_string();

                let window_ptr: *mut _ = self.panel_app.panel_grid.active_window_mut()
                    .map(|w| w as *mut _)
                    .unwrap_or(std::ptr::null_mut::<zengeld_chart::ChartWindow>());

                if !window_ptr.is_null() {
                    // SAFETY: toolbar_state does not access panel_grid.
                    let window = unsafe { &mut *window_ptr };
                    let autosave = self.panel_app.autosave_enabled;
                    let events = self.panel_app.toolbar_state.handle_dropdown_select_with_chart(
                        &dropdown_id,
                        &item_id,
                        &mut window.crosshair,
                        &mut window.drawing_manager,
                        autosave,
                    );
                    for event in events {
                        self.process_chart_out_event(event);
                    }
                }
            }
            return;
        }

        // === Modal widget clicks — route to dedicated handlers ===
        if let Some(rest) = widget_id.strip_prefix("chart_settings:") {
            self.handle_chart_settings_click(rest, x, y);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("prim_settings:") {
            self.handle_prim_settings_click(rest, x, y);
            return;
        }
        // ── Template name overlay modal for primitive settings ────────────────
        if let Some(rest) = widget_id.strip_prefix("prim_tmpl:") {
            self.handle_prim_tmpl_modal_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("ind_settings:") {
            self.handle_ind_settings_click(rest, x, y);
            return;
        }
        // ── Template name overlay modal for indicator settings ────────────────
        if let Some(rest) = widget_id.strip_prefix("ind_tmpl:") {
            self.handle_ind_tmpl_modal_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("alert_set:") {
            self.handle_alert_settings_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("ind_overlay:") {
            self.handle_ind_overlay_click(rest, x, y);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("cmp_overlay:") {
            self.handle_cmp_overlay_click(rest);
            return;
        }
        // ── Template name overlay modal for compare settings ──────────────────
        if let Some(rest) = widget_id.strip_prefix("cmp_tmpl:") {
            self.handle_cmp_tmpl_modal_click(rest);
            return;
        }
        // ── Template name overlay modal for chart settings ────────────────────
        if let Some(rest) = widget_id.strip_prefix("chart_tmpl:") {
            self.handle_chart_tmpl_modal_click(rest);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("cmp_settings:") {
            self.handle_cmp_settings_click(rest, x, y);
            return;
        }
        if let Some(rest) = widget_id.strip_prefix("overlay_settings:") {
            match rest {
                "close" => {
                    self.panel_app.close_overlay_settings();
                    eprintln!("[ChartApp] overlay_settings closed");
                }
                "modal_bg" => {
                    // No-op — absorb click to prevent fall-through.
                }
                tab_id if tab_id.starts_with("tab:") => {
                    use zengeld_chart::ui::modal_settings::OverlayPanelTreeTab;
                    let tab = match &tab_id["tab:".len()..] {
                        "tree_view"  => OverlayPanelTreeTab::TreeView,
                        "eliminate"  => OverlayPanelTreeTab::Eliminate,
                        "hidden"     => OverlayPanelTreeTab::Hidden,
                        "minimap"    => OverlayPanelTreeTab::Minimap,
                        _            => return,
                    };
                    self.panel_app.overlay_settings_state.set_tab(tab);
                }
                id if id.starts_with("select:") => {
                    if let Ok(node_id) = id["select:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(node_id);
                        let leaf_id = zengeld_chart::LeafId(node_id);
                        if self.panel_app.panel_grid.docking().tree().leaf(leaf_id).is_some() {
                            self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        }
                    }
                }
                id if id.starts_with("minimap_leaf:") => {
                    if let Ok(leaf_id_val) = id["minimap_leaf:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(leaf_id_val);
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                id if id.starts_with("eliminate:") => {
                    if let Ok(leaf_id_val) = id["eliminate:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let removed = self.panel_app.panel_grid.close_leaf(leaf_id);
                        if removed {
                            // If eliminated leaf was the target, clear it
                            if self.panel_app.overlay_settings_state.target_leaf_id == Some(leaf_id) {
                                self.panel_app.overlay_settings_state.target_leaf_id = None;
                                self.panel_app.overlay_settings_state.selected_node_id = None;
                            }
                            eprintln!("[OverlaySettings] Eliminated leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("hide:") => {
                    if let Ok(leaf_id_val) = id["hide:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let hidden = self.panel_app.panel_grid.docking_mut().tree_mut().hide_leaf(leaf_id);
                        if hidden {
                            eprintln!("[OverlaySettings] Hidden leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("show:") || id.starts_with("restore:") => {
                    let prefix = if id.starts_with("show:") { "show:" } else { "restore:" };
                    if let Ok(leaf_id_val) = id[prefix.len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.docking_mut().tree_mut().show_leaf(leaf_id);
                        eprintln!("[OverlaySettings] Shown leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("expand:") => {
                    if let Ok(leaf_id_val) = id["expand:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        // Set as active leaf before toggling expand
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.panel_app.panel_grid.toggle_expand();
                        eprintln!("[OverlaySettings] Toggled expand for leaf {}", leaf_id_val);
                    }
                }
                _ => {
                    eprintln!("[ChartApp] unhandled overlay_settings click: {}", rest);
                }
            }
            return;
        }

        // === Tags & Tabs modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("tags_tabs:") {
            use zengeld_chart::ui::modal_settings::{TagsTabsSidebar, TagsTabsTagsTab};
            use zengeld_chart::tag_manager::SyncGroupId;
            match rest {
                "close" => {
                    self.panel_app.close_tags_tabs();
                    eprintln!("[ChartApp] tags_tabs closed");
                }
                "modal_bg" => {
                    // Absorb click — prevent fall-through.
                }
                "sidebar:tabs" => {
                    self.panel_app.tags_tabs_state.set_sidebar(TagsTabsSidebar::Tabs);
                }
                "sidebar:tags" => {
                    self.panel_app.tags_tabs_state.set_sidebar(TagsTabsSidebar::Tags);
                }
                "sidebar:map" => {
                    self.panel_app.tags_tabs_state.set_sidebar(TagsTabsSidebar::Map);
                }
                id if id.starts_with("tab:") => {
                    use zengeld_chart::ui::modal_settings::OverlayPanelTreeTab;
                    let tab = match &id["tab:".len()..] {
                        "tree_view"  => OverlayPanelTreeTab::TreeView,
                        "eliminate"  => OverlayPanelTreeTab::Eliminate,
                        "hidden"     => OverlayPanelTreeTab::Hidden,
                        "minimap"    => OverlayPanelTreeTab::Minimap,
                        _            => return,
                    };
                    self.panel_app.overlay_settings_state.set_tab(tab);
                }
                id if id.starts_with("tags_tab:") => {
                    let tab = match &id["tags_tab:".len()..] {
                        "groups"  => TagsTabsTagsTab::Groups,
                        "details" => TagsTabsTagsTab::Details,
                        _         => return,
                    };
                    self.panel_app.tags_tabs_state.set_tags_tab(tab);
                }
                id if id.starts_with("tags:select_group:") => {
                    if let Ok(gid_val) = id["tags:select_group:".len()..].parse::<u64>() {
                        self.panel_app.tags_tabs_state.selected_group_id = Some(SyncGroupId(gid_val));
                        self.panel_app.tags_tabs_state.set_tags_tab(TagsTabsTagsTab::Details);
                        eprintln!("[TagsTabs] Selected group {}", gid_val);
                    }
                }
                id if id.starts_with("tags:delete_group:") => {
                    if let Ok(gid_val) = id["tags:delete_group:".len()..].parse::<u64>() {
                        let gid = SyncGroupId(gid_val);
                        self.panel_app.tag_manager.remove_group(gid);
                        // Clear selection if we deleted the selected group.
                        if self.panel_app.tags_tabs_state.selected_group_id == Some(gid) {
                            self.panel_app.tags_tabs_state.selected_group_id = None;
                            self.panel_app.tags_tabs_state.set_tags_tab(TagsTabsTagsTab::Groups);
                        }
                        eprintln!("[TagsTabs] Deleted group {}", gid_val);
                    }
                }
                id if id.starts_with("tags:toggle_flag:") => {
                    // Format: tags:toggle_flag:{group_id}:{flag_id}
                    let suffix = &id["tags:toggle_flag:".len()..];
                    if let Some(colon_pos) = suffix.find(':') {
                        let gid_str  = &suffix[..colon_pos];
                        let flag_str = &suffix[colon_pos + 1..];
                        if let Ok(gid_val) = gid_str.parse::<u64>() {
                            let gid = SyncGroupId(gid_val);
                            if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                                match flag_str {
                                    "sync_crosshair"  => group.sync_flags.sync_crosshair  = !group.sync_flags.sync_crosshair,
                                    "sync_viewport"   => group.sync_flags.sync_viewport   = !group.sync_flags.sync_viewport,
                                    "sync_symbol"     => group.sync_flags.sync_symbol     = !group.sync_flags.sync_symbol,
                                    "sync_timeframe"  => group.sync_flags.sync_timeframe  = !group.sync_flags.sync_timeframe,
                                    "sync_drawings"   => group.sync_flags.sync_drawings   = !group.sync_flags.sync_drawings,
                                    "sync_indicators" => group.sync_flags.sync_indicators = !group.sync_flags.sync_indicators,
                                    _ => {}
                                }
                                eprintln!("[TagsTabs] Toggled {} on group {}", flag_str, gid_val);
                            }
                        }
                    }
                }
                // --- Panel tree actions (ported from overlay_settings) ---
                id if id.starts_with("select:") => {
                    if let Ok(node_id) = id["select:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(node_id);
                        let leaf_id = zengeld_chart::LeafId(node_id);
                        if self.panel_app.panel_grid.docking().tree().leaf(leaf_id).is_some() {
                            self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        }
                    }
                }
                id if id.starts_with("minimap_leaf:") => {
                    if let Ok(leaf_id_val) = id["minimap_leaf:".len()..].parse::<u64>() {
                        self.panel_app.overlay_settings_state.selected_node_id = Some(leaf_id_val);
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                id if id.starts_with("eliminate:") => {
                    if let Ok(leaf_id_val) = id["eliminate:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let removed = self.panel_app.panel_grid.close_leaf(leaf_id);
                        if removed {
                            if self.panel_app.overlay_settings_state.target_leaf_id == Some(leaf_id) {
                                self.panel_app.overlay_settings_state.target_leaf_id = None;
                                self.panel_app.overlay_settings_state.selected_node_id = None;
                            }
                            eprintln!("[TagsTabs] Eliminated leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("hide:") => {
                    if let Ok(leaf_id_val) = id["hide:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        let hidden = self.panel_app.panel_grid.docking_mut().tree_mut().hide_leaf(leaf_id);
                        if hidden {
                            eprintln!("[TagsTabs] Hidden leaf {}", leaf_id_val);
                        }
                    }
                }
                id if id.starts_with("show:") || id.starts_with("restore:") => {
                    let prefix = if id.starts_with("show:") { "show:" } else { "restore:" };
                    if let Ok(leaf_id_val) = id[prefix.len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.docking_mut().tree_mut().show_leaf(leaf_id);
                        eprintln!("[TagsTabs] Shown leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("expand:") => {
                    if let Ok(leaf_id_val) = id["expand:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.panel_app.panel_grid.toggle_expand();
                        eprintln!("[TagsTabs] Toggled expand for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("untag:") => {
                    if let Ok(leaf_id_val) = id["untag:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.perform_desync(leaf_id);
                        eprintln!("[TagsTabs] Untagged leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("split_h:") => {
                    if let Ok(leaf_id_val) = id["split_h:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.do_split(zengeld_chart::SplitKind::Horizontal);
                        eprintln!("[TagsTabs] Split horizontal for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("split_v:") => {
                    if let Ok(leaf_id_val) = id["split_v:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                        self.do_split(zengeld_chart::SplitKind::Vertical);
                        eprintln!("[TagsTabs] Split vertical for leaf {}", leaf_id_val);
                    }
                }
                id if id.starts_with("tag:") => {
                    if let Ok(leaf_id_val) = id["tag:".len()..].parse::<u64>() {
                        let leaf_id = zengeld_chart::LeafId(leaf_id_val);
                        // Open the sync color grid popup anchored near the modal center
                        let anchor_x = self.width as f64 / 2.0;
                        let anchor_y = self.height as f64 / 2.0;
                        self.panel_app.sync_color_grid.open(
                            leaf_id,
                            anchor_x,
                            anchor_y,
                            self.width as f64,
                            self.height as f64,
                        );
                        eprintln!("[TagsTabs] Opening tag color grid for leaf {}", leaf_id_val);
                    }
                }
                _ => {
                    eprintln!("[ChartApp] unhandled tags_tabs click: {}", rest);
                }
            }
            return;
        }

        // === Watchlist group name input modal clicks ===
        // NOTE: checked BEFORE wl_modal: so the higher-layer modal intercepts first.
        if let Some(rest) = widget_id.strip_prefix("wl_group_name:") {
            match rest {
                "save" => {
                    let name = self.wl_group_name_input.editing.text.trim().to_string();
                    if !name.is_empty() {
                        use zengeld_chart::ui::modal_settings::WatchlistGroupNameMode;
                        match self.wl_group_name_input.mode.clone() {
                            WatchlistGroupNameMode::CreateNew => {
                                let new_id = self.sidebar_state.watchlist_manager.create_list(name.clone());
                                self.sidebar_state.watchlist_manager.active_list_id = new_id;
                                self.watchlist_actions.push(crate::WatchlistAction::CreateList { name: name.clone() });
                                self.watchlists_dirty = true;
                                self.persist_watchlists();
                                eprintln!("[WatchlistGroupName] created new list '{}' id={}", name, new_id);
                            }
                            WatchlistGroupNameMode::Rename(id) => {
                                if let Some(list) = self.sidebar_state.watchlist_manager.lists.iter_mut().find(|l| l.id == id) {
                                    list.name = name.clone();
                                    self.watchlist_actions.push(crate::WatchlistAction::RenameList { id, new_name: name.clone() });
                                    self.watchlists_dirty = true;
                                    self.persist_watchlists();
                                    eprintln!("[WatchlistGroupName] renamed list id={} to '{}'", id, name);
                                }
                            }
                        }
                    }
                    self.wl_group_name_input.close();
                }
                "cancel" | "close" => {
                    self.wl_group_name_input.close();
                    eprintln!("[WatchlistGroupName] cancelled");
                }
                "input" => {
                    // Click-to-cursor using pre-computed character positions
                    if let Some(ref gni) = self.last_wl_group_name_result {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &gni.char_x_positions,
                            x,
                        );
                        self.wl_group_name_input.editing.cursor = new_cursor;
                        self.wl_group_name_input.editing.selection_start = None;
                    }
                }
                "modal_bg" => {
                    // Absorb clicks on modal background — do nothing
                }
                _ => {
                    eprintln!("[WatchlistGroupName] unhandled: {}", rest);
                }
            }
            return;
        }

        // === Watchlist modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("wl_modal:") {
            // Clear any leftover drag state so a click (< 5px movement) can
            // always dispatch through this handler properly.
            self.watchlist_modal.drag_reorder = None;
            self.watchlist_modal.drag_reorder_pending = None;
            self.handle_watchlist_modal_click(rest, x, y);
            return;
        }

        // === Search modal clicks ===
        if let Some(rest) = widget_id.strip_prefix("modal_search:") {
            self.handle_search_modal_click(rest, x, y);
            return;
        }

        // === Indicator search sidebar / sets clicks ===
        if let Some(rest) = widget_id.strip_prefix("ind_search:") {
            self.handle_indicator_search_action(rest);
            return;
        }

        // === Color picker clicks ===
        if widget_id.starts_with("color_picker_primitive:") {
            self.handle_color_picker_click(widget_id, x, y, "primitive");
            return;
        }
        if widget_id.starts_with("color_picker_indicator:") {
            self.handle_color_picker_click(widget_id, x, y, "indicator");
            return;
        }
        if widget_id.starts_with("color_picker_chart:") {
            self.handle_color_picker_click(widget_id, x, y, "chart");
            return;
        }
        if widget_id.starts_with("color_picker_panel:") {
            self.handle_color_picker_click(widget_id, x, y, "panel");
            return;
        }
        if widget_id.starts_with("color_picker_compare:") {
            self.handle_color_picker_click(widget_id, x, y, "compare");
            return;
        }

        // === Sync color grid clicks ===
        if widget_id.starts_with("sync_color_grid:") {
            self.handle_sync_color_grid_click(widget_id, x, y);
            return;
        }

        // === Preset name input modal clicks ===
        if widget_id.starts_with("preset_name_input:") {
            let rest = &widget_id["preset_name_input:".len()..];
            match rest {
                "save" => {
                    let name = self.panel_app.preset_name_input.name().to_string();
                    if !name.trim().is_empty() {
                        use zengeld_chart::ui::modal_settings::PresetNameInputMode;
                        match self.panel_app.preset_name_input.mode {
                            PresetNameInputMode::SaveAs => {
                                self.process_chart_out_event(
                                    zengeld_chart::events::ChartOutEvent::SavePreset { name }
                                );
                            }
                            PresetNameInputMode::Rename => {
                                let id = self.panel_app.preset_name_input.rename_preset_id
                                    .clone().unwrap_or_default();
                                self.process_chart_out_event(
                                    zengeld_chart::events::ChartOutEvent::RenamePreset { id, new_name: name }
                                );
                            }
                            PresetNameInputMode::NewChart => {
                                self.execute_new_chart_with_name(name);
                            }
                            PresetNameInputMode::CreateIndicatorSet => {
                                self.execute_create_indicator_set(name);
                            }
                        }
                    }
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] preset name input committed via Save button");
                }
                "cancel" => {
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] preset name input closed via Cancel button");
                }
                "close" => {
                    self.panel_app.preset_name_input.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] preset name input closed via X");
                }
                "input" => {
                    // Click-to-cursor using pre-computed character positions
                    if let Some(ref pni) = self.frame_result.as_ref().and_then(|r| r.preset_name_input.as_ref()) {
                        let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                            &pni.char_x_positions,
                            x,
                        );
                        self.panel_app.preset_name_input.editing.cursor = new_cursor;
                        self.panel_app.preset_name_input.editing.selection_start = None;
                    }
                }
                "modal_bg" => {
                    // No-op — absorb click to prevent fall-through.
                }
                _ => {
                    eprintln!("[ChartApp] preset_name_input unhandled: {}", rest);
                }
            }
            return;
        }

        // === Chart browser modal clicks ===
        if widget_id.starts_with("chart_browser:") {
            let action = &widget_id["chart_browser:".len()..];
            if action == "close" {
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                eprintln!("[ChartApp] chart browser closed via X");
            } else if action == "modal_bg" {
                // Absorb click — no-op.
            } else if action == "search_input" {
                // Click in search input — position cursor using char positions.
                let char_positions = self.frame_result.as_ref()
                    .and_then(|r| r.chart_browser.as_ref())
                    .map(|br| br.search_char_positions.clone());
                if let Some(positions) = char_positions {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&positions, x);
                    self.panel_app.chart_browser.search_editing.cursor = new_cursor;
                    self.panel_app.chart_browser.search_editing.selection_start = None;
                    self.panel_app.chart_browser.search_editing.reset_blink(0);
                }
            } else if let Some(preset_id) = action.strip_prefix("rename:") {
                // Click rename icon — close browser, open rename modal for this preset.
                let preset_id = preset_id.to_string();
                let name_opt = self.panel_app.presets.get(&preset_id).map(|p| p.name.clone());
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                if let Some(name) = name_opt {
                    self.panel_app.preset_name_input.open_rename(&preset_id, &name, 0);
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::PresetNameInput;
                    eprintln!("[ChartApp] chart browser: rename preset '{}'", preset_id);
                }
            } else if let Some(preset_id) = action.strip_prefix("delete:") {
                // Click delete icon — delete preset.
                let preset_id = preset_id.to_string();
                eprintln!("[ChartApp] chart browser: delete preset '{}'", preset_id);
                self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::DeletePreset { id: preset_id });
            } else if let Some(preset_id) = action.strip_prefix("item:") {
                // Click on preset row — load preset and close browser.
                // Read the flag BEFORE close() resets it.
                let open_in_new_tab = self.panel_app.chart_browser.open_in_new_tab;
                let preset_id = preset_id.to_string();
                eprintln!("[ChartApp] chart browser: {} preset '{}' (new_tab={})",
                    if open_in_new_tab { "open tab" } else { "load" }, preset_id, open_in_new_tab);
                self.panel_app.chart_browser.close();
                self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                if open_in_new_tab {
                    self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::OpenTab { id: preset_id });
                } else {
                    self.process_chart_out_event(zengeld_chart::events::ChartOutEvent::LoadPreset { id: preset_id });
                }
            } else {
                eprintln!("[ChartApp] chart_browser unhandled action: {}", action);
            }
            return;
        }

        // === User settings modal clicks ===
        if widget_id.starts_with("user_settings:") {
            use zengeld_chart::ui::modal_settings::UserSettingsTab;
            let action = &widget_id["user_settings:".len()..];
            match action {
                "close" => {
                    self.panel_app.user_settings_state.close();
                    eprintln!("[ChartApp] user settings closed via X");
                }
                "modal_bg" => {
                    // Absorb click — no-op.
                }
                "header" => {
                    // Drag start handled in mouse_down.
                }
                // Tab switching
                rest if rest.starts_with("tab:") => {
                    let tab_id = &rest[4..];
                    if let Some(tab) = UserSettingsTab::from_id(tab_id) {
                        self.panel_app.user_settings_state.set_tab(tab);
                        eprintln!("[ChartApp] user_settings tab switched to: {}", tab_id);
                    }
                }
                // Recalc mode radio option selected — hit id: "recalc_mode:{Key}"
                rest if rest.starts_with("recalc_mode:") => {
                    use crate::RecalcMode;
                    let mode_str = &rest["recalc_mode:".len()..];
                    let label = match mode_str {
                        "PerTick"  => "Per Tick",
                        "PerFrame" => "Per Frame",
                        "PerBar"   => "Per Bar",
                        _ => { eprintln!("[ChartApp] user_settings unknown recalc mode: {}", mode_str); return; }
                    };
                    self.panel_app.user_settings_state.recalc_mode_label = label.to_string();
                    self.indicator_manager.recalc_mode = match mode_str {
                        "PerTick" => RecalcMode::PerTick,
                        "PerBar"  => RecalcMode::PerBar,
                        _         => RecalcMode::PerFrame,
                    };
                    self.recalc_mode_changed = Some(mode_str.to_string());
                    // Reset counters and clear stale dirty flags when mode changes.
                    self.recalc_count = 0;
                    self.trade_count = 0;
                    self.recalc_log_timer = std::time::Instant::now();
                    self.indicator_manager.clear_pending();
                    eprintln!("[ChartApp] user_settings recalc_mode set to: {}", mode_str);
                }
                "diagnostics_toggle" => {
                    self.diagnostics_enabled = !self.diagnostics_enabled;
                    self.panel_app.user_settings_state.diagnostics_enabled = self.diagnostics_enabled;
                    eprintln!("[ChartApp] diagnostics_enabled = {}", self.diagnostics_enabled);
                }
                "server_toggle" => {
                    self.panel_app.user_settings_state.server_enabled =
                        !self.panel_app.user_settings_state.server_enabled;
                    self.server_enabled_changed = Some(self.panel_app.user_settings_state.server_enabled);
                    eprintln!("[ChartApp] server_enabled = {}", self.panel_app.user_settings_state.server_enabled);
                }
                "server_key_create" => {
                    let label = self.panel_app.user_settings_state.new_key_label.trim().to_string();
                    let tier = self.panel_app.user_settings_state.new_key_tier.clone();
                    if !label.is_empty() {
                        self.key_create_request = Some((label, tier));
                        self.panel_app.user_settings_state.new_key_label.clear();
                        self.panel_app.user_settings_state.new_key_label_focused = false;
                    }
                    eprintln!("[ChartApp] server_key_create requested");
                }
                "server_key_tier_toggle" => {
                    let state = &mut self.panel_app.user_settings_state;
                    state.new_key_tier = match state.new_key_tier.as_str() {
                        "read_only" => "read_write".to_string(),
                        "read_write" => "admin".to_string(),
                        _ => "read_only".to_string(),
                    };
                    eprintln!("[ChartApp] server_key_tier_toggle: {}", state.new_key_tier);
                }
                "server_key_copy_new" => {
                    if let Some(ref key) = self.panel_app.user_settings_state.last_created_key {
                        self.clipboard_text = Some(key.clone());
                        eprintln!("[ChartApp] new key copied to clipboard");
                    }
                    // Clear after copy so the reveal box disappears
                    self.panel_app.user_settings_state.last_created_key = None;
                }
                "server_key_label_input" => {
                    self.panel_app.user_settings_state.new_key_label_focused = true;
                    eprintln!("[ChartApp] server_key_label_input focused");
                }
                rest if rest.starts_with("server_key_delete_") => {
                    let label = rest.strip_prefix("server_key_delete_").unwrap_or("");
                    if !label.is_empty() {
                        self.key_delete_request = Some(label.to_string());
                        eprintln!("[ChartApp] server_key_delete: {}", label);
                    }
                }
                _ => {
                    eprintln!("[ChartApp] user_settings unhandled action: {}", action);
                }
            }
            return;
        }

        // === Context menu clicks ===
        if let Some(rest) = widget_id.strip_prefix("context_menu:") {
            if rest == "bg" {
                // Click on context menu background — close the menu.
                self.panel_app.context_menu_state.close();
                return;
            }
            if let Some(action) = rest.strip_prefix("item:") {
                eprintln!("[ChartApp] Context menu action: {}", action);
                self.on_context_menu_action(action);
                return;
            }
        }

        // === Overlay tab clicks — gear menu, color tag, body ===
        if widget_id.starts_with("leaf_tab:") {
            match self.leaf_tab_hover {
                zengeld_chart::LeafTabHoverZone::GearMenu => {
                    if let Some(leaf_id) = self.leaf_tab_hovered_leaf {
                        self.panel_app.open_tags_tabs_for_leaf(leaf_id);
                    } else {
                        self.panel_app.open_tags_tabs();
                    }
                }
                zengeld_chart::LeafTabHoverZone::ColorTag => {
                    if let Some(leaf_id) = self.leaf_tab_hovered_leaf {
                        if let Some(zones) = self.leaf_tab_hit_zones.get(&leaf_id).cloned() {
                            let anchor = zones.color_tag_rect;
                            self.panel_app.sync_color_grid.open(
                                leaf_id,
                                anchor[0],
                                // Position below the color tag square
                                anchor[1] + anchor[3],
                                self.width as f64,
                                self.height as f64,
                            );
                        }
                    }
                }
                zengeld_chart::LeafTabHoverZone::Body => {
                    if let Some(leaf_id) = self.leaf_tab_hovered_leaf {
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                zengeld_chart::LeafTabHoverZone::None => {}
            }
            return;
        }

        eprintln!("[ChartApp] unhandled widget click: {}", widget_id);
    }

    // -------------------------------------------------------------------------
    // Modal widget click handlers
    // -------------------------------------------------------------------------

    /// Handle clicks on widgets registered with the "chart_settings:" prefix.
    fn handle_chart_settings_click(&mut self, rest: &str, x: f64, _y: f64) {
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
        if item_id.starts_with("instrument:") {
            let field = &item_id["instrument:".len()..];
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
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("scales:") {
            let field = &item_id["scales:".len()..];
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let next = w.price_scale.scale_mode.next();
                        w.price_scale.scale_mode = next;
                        if next.is_auto_y() {
                            w.calc_auto_scale();
                        }
                        eprintln!("[ChartApp] scales:auto_scale -> {:?}", next);
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
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("status:") {
            let field = &item_id["status:".len()..];
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
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("appearance:") {
            let field = &item_id["appearance:".len()..];

            // Theme preset buttons
            if field.starts_with("theme_") {
                let theme_id = &field["theme_".len()..];
                self.panel_app.theme_manager.set_preset(theme_id);
                // Signal the App-level coordinator to propagate this change to all windows.
                self.theme_changed = Some(theme_id.to_string());
                eprintln!("[ChartApp] theme preset: {}", theme_id);
                return;
            }

            // UI Style buttons
            if field.starts_with("ui_style:") {
                let style_index = field["ui_style:".len()..].parse::<usize>().unwrap_or(0);
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
                let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("dropdown_option:") {
            let rest = &item_id["dropdown_option:".len()..];
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
        if item_id.starts_with("dropdown_cycle:") {
            let name = &item_id["dropdown_cycle:".len()..];
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
                _ => {
                    eprintln!("[ChartApp] chart_settings dropdown_cycle unknown: {}", name);
                }
            }
            return;
        }

        if item_id.starts_with("dropdown_menu:") {
            let name = &item_id["dropdown_menu:".len()..];
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
    fn handle_prim_settings_click(&mut self, rest: &str, x: f64, y: f64) {
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
                            let mut pt = PrimitiveText::default();
                            pt.content = text.clone();
                            data.text = Some(pt);
                        }
                        window.drawing_manager.set_data_at(idx, &data);
                    }
                }
            }
            eprintln!("[ChartApp] prim_settings text_content auto-committed: {}", text);
        } else if field == "stroke_width_value" || field == "stroke_width" {
            if let Ok(width) = text.trim().parse::<f64>() {
                if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        if let Some(mut data) = window.drawing_manager.get_data_at(idx) {
                            data.width = width.max(0.5).min(20.0);
                            window.drawing_manager.set_data_at(idx, &data);
                        }
                    }
                }
            }
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
                            let prims = window.drawing_manager.primitives_mut();
                            if idx < prims.len() {
                                let mut pts = prims[idx].points().to_vec();
                                if pt_idx < pts.len() {
                                    pts[pt_idx].0 = bar;
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
            let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
                    eprintln!("[ChartApp] prim_settings line_style set to: {}", style_name);
                }
            }
            self.panel_app.primitive_settings_state.open_line_style_dropdown = false;
            self.snapshot_primitive_settings_to_user_manager(idx);
            return;
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
                    eprintln!("[ChartApp] prim_settings text_italic = {}", new_italic);
                }
            }
            return;
        }

        // ── Text position: text_pos_{v}_{h} ──────────────────────────────────
        if item_id.starts_with("text_pos_") {
            let parts: Vec<&str> = item_id["text_pos_".len()..].splitn(2, '_').collect();
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
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("style_prop:") {
            let prop_id = &item_id["style_prop:".len()..];
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
                            eprintln!("[ChartApp] prim_settings style_prop '{}' cycled: {} -> {}", prop_id, current, next_value);
                        }
                    }
                    PropertyType::Color => {
                        let current_color = prop_value.as_color().unwrap_or("#ffffff");
                        let widget_id_str = format!("prim_settings:item:{}", item_id);
                        let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("level_prop:") {
            let prop_id = &item_id["level_prop:".len()..];
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
                        let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("text_prop:") {
            let prop_id = &item_id["text_prop:".len()..];
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
                        let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
    fn handle_prim_tmpl_modal_click(&mut self, rest: &str) {
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
    fn handle_ind_tmpl_modal_click(&mut self, rest: &str) {
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
    fn handle_cmp_tmpl_modal_click(&mut self, rest: &str) {
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
    fn handle_chart_tmpl_modal_click(&mut self, rest: &str) {
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
    fn handle_alert_settings_click(&mut self, rest: &str) {
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
    fn handle_ind_settings_click(&mut self, rest: &str, x: f64, y: f64) {
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
                    if item_id.starts_with("input:") {
                        // "input:<name>" maps to "indicator_param:<name>"
                        let active_field_id = format!("indicator_param:{}", &item_id["input:".len()..]);
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
                            let symbol = self.panel_app.panel_grid.active_window()
                                .map(|w| w.symbol.clone()).unwrap_or_default();
                            if let Some(bars) = bars_opt {
                                self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
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
                                let symbol = self.panel_app.panel_grid.active_window()
                                    .map(|w| w.symbol.clone()).unwrap_or_default();
                                if let Some(bars) = bars_opt {
                                    self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
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
                        let symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone()).unwrap_or_default();
                        if let Some(bars) = bars_opt {
                            self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
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
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone()).unwrap_or_default();
                if let Some(bars) = bars_opt {
                    self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
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
        if item_id.starts_with("color:") {
            let output_name = &item_id["color:".len()..];
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
            let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
        if item_id.starts_with("dropdown_menu:") {
            let param_name = &item_id["dropdown_menu:".len()..];
            if self.panel_app.indicator_settings_state.open_param_dropdown.as_deref() == Some(param_name) {
                self.panel_app.indicator_settings_state.open_param_dropdown = None;
            } else {
                self.panel_app.indicator_settings_state.open_param_dropdown = Some(param_name.to_string());
            }
            eprintln!("[ChartApp] ind_settings dropdown_menu toggled: {}", param_name);
            return;
        }

        // ── Dropdown cycle ────────────────────────────────────────────────────
        if item_id.starts_with("dropdown_cycle:") {
            let param_name = &item_id["dropdown_cycle:".len()..];
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
                    let symbol = self.panel_app.panel_grid.active_window()
                        .map(|w| w.symbol.clone()).unwrap_or_default();
                    if let Some(bars) = bars_opt {
                        self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
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
        if item_id.starts_with("param_option:") {
            let rest = &item_id["param_option:".len()..];
            let mut parts = rest.splitn(2, ':');
            let param_name = parts.next().unwrap_or("").to_string();
            let value = parts.next().unwrap_or("").to_string();

            self.panel_app.indicator_settings_state.open_param_dropdown = None;

            if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
                if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                    inst.set_param(&param_name, IndValue::String(value.clone()));
                }
                let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                let symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone()).unwrap_or_default();
                if let Some(bars) = bars_opt {
                    self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
                }
                self.autosave_snapshot();
                self.snapshot_indicator_settings_to_user_manager();
                eprintln!("[ChartApp] ind_settings param_option: {} = {}", param_name, value);
            }
            return;
        }

        // ── Toggle boolean parameter ──────────────────────────────────────────
        if item_id.starts_with("toggle:") {
            let param_name = &item_id["toggle:".len()..];
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
                    let symbol = self.panel_app.panel_grid.active_window()
                        .map(|w| w.symbol.clone()).unwrap_or_default();
                    if let Some(bars) = bars_opt {
                        self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
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
        if item_id.starts_with("input:") {
            let param_name = item_id["input:".len()..].to_string();
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

        eprintln!("[ChartApp] ind_settings item unhandled: {}", item_id);
    }

    // =========================================================================
    // Color Picker Handlers
    // =========================================================================

    /// Route a click on a color picker widget to the appropriate hit test and handler.
    fn handle_color_picker_click(&mut self, widget_id: &str, x: f64, y: f64, source: &str) {
        use zengeld_chart::ui::widgets::color_picker::{
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
    fn handle_l1_hit(&mut self, hit: zengeld_chart::ui::widgets::color_picker::ColorPickerL1HitResult, source: &str, x: f64, y: f64) {
        use zengeld_chart::ui::widgets::color_picker::ColorPickerL1HitResult;

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
                            let rgba = hex_str_to_rgba(&final_color);
                            self.panel_app.sync_color_grid.custom_colors.push(rgba);
                            self.panel_app.sync_color_grid.adding_custom_color = false;
                            // Grid is still open — just close the color picker.
                            self.panel_app.close_panel_color_tag_picker();
                        } else {
                            // Normal flow — assign color to leaf.
                            if let Some(leaf_id) = self.panel_app.panel_color_picker_leaf {
                                let rgba = hex_str_to_rgba(&final_color);
                                self.panel_app.leaf_color_tags.insert(leaf_id, rgba);
                            }
                            self.panel_app.close_panel_color_tag_picker();
                        }
                    }
                    _ => {}
                }
            }
            ColorPickerL1HitResult::PlusButton => {
                match source {
                    "primitive" => self.panel_app.primitive_settings_state.color_picker.open_l2(),
                    "indicator" => self.panel_app.indicator_settings_state.color_picker.open_l2(),
                    "chart"     => self.panel_app.chart_settings_state.color_picker.open_l2(),
                    "compare"   => self.panel_app.compare_settings_state.color_picker.open_l2(),
                    "panel"     => self.panel_app.panel_color_picker.open_l2(),
                    _ => {}
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
    fn handle_l2_hit(&mut self, hit: zengeld_chart::ui::widgets::color_picker::ColorPickerL2HitResult, source: &str, x: f64, y: f64) {
        use zengeld_chart::ui::widgets::color_picker::ColorPickerL2HitResult;

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
                            let rgba = hex_str_to_rgba(&final_color);
                            self.panel_app.sync_color_grid.custom_colors.push(rgba);
                            self.panel_app.sync_color_grid.adding_custom_color = false;
                            // Grid is still open — just close the color picker.
                            self.panel_app.close_panel_color_tag_picker();
                        } else {
                            // Normal flow — assign color to leaf.
                            if let Some(leaf_id) = self.panel_app.panel_color_picker_leaf {
                                let rgba = hex_str_to_rgba(&final_color);
                                self.panel_app.leaf_color_tags.insert(leaf_id, rgba);
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
                // Toggle hex editing mode for the active picker
                match source {
                    "primitive" => {
                        self.panel_app.primitive_settings_state.color_picker.hex_editing =
                            !self.panel_app.primitive_settings_state.color_picker.hex_editing;
                    }
                    "indicator" => {
                        self.panel_app.indicator_settings_state.color_picker.hex_editing =
                            !self.panel_app.indicator_settings_state.color_picker.hex_editing;
                    }
                    "chart" => {
                        self.panel_app.chart_settings_state.color_picker.hex_editing =
                            !self.panel_app.chart_settings_state.color_picker.hex_editing;
                    }
                    "compare" => {
                        self.panel_app.compare_settings_state.color_picker.hex_editing =
                            !self.panel_app.compare_settings_state.color_picker.hex_editing;
                    }
                    "panel" => {
                        self.panel_app.panel_color_picker.hex_editing =
                            !self.panel_app.panel_color_picker.hex_editing;
                    }
                    _ => {}
                }
                eprintln!("[ChartApp] color picker hex input toggled");
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
    fn handle_sync_color_grid_click(&mut self, widget_id: &str, x: f64, y: f64) {
        use zengeld_chart::ui::sync_color_grid::{hit_test_sync_color_grid, SyncColorGridHitResult};

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
                }
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
                        let same_color = old_color.map_or(false, |oc|
                            (oc[0] - new_color[0]).abs() < 0.01
                            && (oc[1] - new_color[1]).abs() < 0.01
                            && (oc[2] - new_color[2]).abs() < 0.01
                        );
                        if !same_color {
                            // Desync from old group (purge cloned primitives/indicators).
                            if old_color.is_some() {
                                self.perform_desync(leaf_id);
                            }
                            // Assign new color.
                            self.panel_app.leaf_color_tags.insert(leaf_id, new_color);
                            eprintln!(
                                "[ChartApp] Sync color grid: assigned color [{:.2},{:.2},{:.2}] to leaf {:?}",
                                new_color[0], new_color[1], new_color[2], leaf_id
                            );
                            // Sync with new group — clone primitives/indicators from peers.
                            self.sync_join_color_group(leaf_id, new_color);
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
    fn apply_primitive_color(&mut self, color: &str) {
        let field = match self.panel_app.primitive_settings_state.color_picker_field.clone() {
            Some(f) => f,
            None => return,
        };
        let idx = match self.panel_app.primitive_settings_state.primitive_idx {
            Some(i) => i,
            None => return,
        };
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            if let Some(mut data) = window.drawing_manager.get_data_at(idx) {
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
                    _ if field.starts_with("level_") && field.ends_with("_color") => {
                        eprintln!("[ChartApp] level color change: {} = {}", field, color);
                    }
                    _ => {}
                }
                window.drawing_manager.set_data_at(idx, &data);
            }
        }
        eprintln!("[ChartApp] applied primitive color: {} = {}", field, color);
        self.autosave_snapshot();
        if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
            self.snapshot_primitive_settings_to_user_manager(idx);
        }
    }

    /// Apply the color picker color to the active indicator output.
    fn apply_indicator_color(&mut self, output_name: &str, color: &str) {
        if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
            if let Some(inst) = self.indicator_manager.get_instance_mut(ind_id) {
                if let Some(output_cfg) = inst.outputs.get_mut(output_name) {
                    output_cfg.color = Some(color.to_string());
                    eprintln!("[ChartApp] applied indicator color: {} output '{}' = {}", ind_id, output_name, color);
                }
            }
        }
        self.autosave_snapshot();
        self.snapshot_indicator_settings_to_user_manager();
    }

    /// Apply the color picker color to chart settings fields.
    fn apply_chart_settings_color(&mut self, field: &str, color: &str) {
        match field {
            "crosshair_line_color" => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.crosshair_options.vert_line.color = color.to_string();
                    w.crosshair_options.horz_line.color = color.to_string();
                    eprintln!("[ChartApp] applied crosshair_line_color: {}", color);
                }
            }
            "body_up_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_up_body = color.to_string();
                eprintln!("[ChartApp] applied body_up_color: {}", color);
            }
            "body_down_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_down_body = color.to_string();
                eprintln!("[ChartApp] applied body_down_color: {}", color);
            }
            "border_up_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_up_border = Some(color.to_string());
                eprintln!("[ChartApp] applied border_up_color: {}", color);
            }
            "border_down_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_down_border = Some(color.to_string());
                eprintln!("[ChartApp] applied border_down_color: {}", color);
            }
            "wick_up_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_up_wick = color.to_string();
                eprintln!("[ChartApp] applied wick_up_color: {}", color);
            }
            "wick_down_color" => {
                self.panel_app.theme_manager.current_mut().series.candle_down_wick = color.to_string();
                eprintln!("[ChartApp] applied wick_down_color: {}", color);
            }
            "watermark_color" => {
                if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    if let Some(ref mut watermark) = w.watermark {
                        if let Some(line) = watermark.lines.first_mut() {
                            line.color = color.to_string();
                            eprintln!("[ChartApp] applied watermark_color: {}", color);
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
                    eprintln!("[ChartApp] applied appearance color '{}' = {}", other, color);
                } else {
                    eprintln!("[ChartApp] apply_chart_settings_color: unknown field '{}' = {}", other, color);
                }
            }
        }
        self.autosave_snapshot();
        self.snapshot_chart_settings_to_user_manager();
    }

    /// Handle clicks on widgets registered with the "ind_overlay:" prefix.
    fn handle_ind_overlay_click(&mut self, rest: &str, _x: f64, _y: f64) {
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
    fn handle_cmp_overlay_click(&mut self, rest: &str) {
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
    fn handle_cmp_overlay_action(&mut self, action: &str) {
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
    fn handle_leaf_cmp_overlay_action(&mut self, leaf_id: zengeld_chart::LeafId, action: &str) {
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
    fn handle_cmp_settings_click(&mut self, rest: &str, x: f64, y: f64) {
        // Close template dropdown on any non-template click
        if !rest.starts_with("template_") {
            self.panel_app.compare_settings_state.template_dropdown_open = false;
        }
        match rest {
            "close" | "ok" => {
                self.panel_app.compare_settings_state.close();
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
    fn apply_cmp_tf_toggle(&mut self, tf_idx: usize) {
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
    fn apply_cmp_tf_text_commit(&mut self, field_id: &str, text: &str) {
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

    /// Apply a color change to the compare series currently being edited.
    fn apply_compare_color(&mut self, color: &str) {
        let idx = self.panel_app.compare_settings_state.series_index;
        self.panel_app.compare_settings_state.cached_color = color.to_string();
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.compare_overlay.set_series_color_by_index(idx, color);
        }
        eprintln!("[ChartApp] cmp_settings color applied: idx={} color={}", idx, color);
        self.snapshot_compare_settings_to_user_manager();
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
    fn handle_indicator_overlay_click(&mut self, rest: &str) {
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
    fn handle_indicator_overlay_action(&mut self, action: &str) {
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
                        let source = alerts::AlertSource::Indicator {
                            indicator_id: id,
                            output_index: 0,
                            label: label.clone(),
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
    fn handle_leaf_indicator_overlay_action(&mut self, leaf_id: zengeld_chart::LeafId, action: &str) {
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
                        let source = alerts::AlertSource::Indicator {
                            indicator_id: id,
                            output_index: 0,
                            label: label.clone(),
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
    fn delete_indicator_instance(&mut self, id: u64) {
        let type_id_opt = self.indicator_manager.get_instance(id)
            .map(|inst| inst.type_id.clone());
        if let Some(type_id) = type_id_opt {
            self.push_undo_command(zengeld_chart::Command::RemoveIndicator {
                instance_id: id,
                type_id: type_id.clone(),
                params_json: String::new(),
            });
            eprintln!("[ChartApp] Recorded RemoveIndicator {} id={}", type_id, id);
        }
        // If the active window is in a sync group, remove the corresponding config.
        // Config ids are set equal to instance ids (both for seeded and newly-added configs).
        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
            if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                if let Ok(()) = self.panel_app.tag_manager.remove_indicator_config(chart_id, id) {
                    eprintln!(
                        "[TagManager] Removed indicator config id={} from group of chart {:?}",
                        id, chart_id
                    );
                }
            }
        }
        self.indicator_manager.remove_instance(id);
        self.alert_manager.remove_alerts_for_indicator(id);
        self.sync_sub_panes_from_manager();
        self.autosave_snapshot();
        eprintln!("[ChartApp] indicator {} deleted", id);
    }

    /// Handle a click on an inline primitive toolbar action.
    ///
    /// These actions are generated when a drawing primitive is selected and the
    /// user clicks one of the compact action buttons rendered inline in the
    /// control strip (`inline:settings`, `inline:color`, `inline:delete`, …).
    fn handle_inline_action(&mut self, action: &str) {
        use zengeld_chart::drawing::primitives_v2::LineStyle;

        // The selected primitive index comes from the drawing manager, not from
        // a settings-state field, because the primitive may not be open in a
        // modal yet.
        let selected_idx = self.panel_app.panel_grid
            .active_window()
            .and_then(|w| w.drawing_manager.selected());

        let idx = match selected_idx {
            Some(i) => i,
            None => {
                eprintln!("[ChartApp] inline action '{}' ignored — no primitive selected", action);
                return;
            }
        };

        let screen_w = self.width as f64;
        let screen_h = self.height as f64;

        match action {
            // ── Settings ─────────────────────────────────────────────────────
            "inline:settings" => {
                self.panel_app.primitive_settings_state.open(idx);
                eprintln!("[ChartApp] inline: opened primitive settings for #{}", idx);
            }

            // ── Stroke color ─────────────────────────────────────────────────
            "inline:color" => {
                let current_color = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx))
                    .map(|d| d.color.stroke.clone());

                // Look up the button rect from the input coordinator so the
                // color picker can be anchored beneath the clicked button.
                let widget_id_str = "csb:inline:color".to_string();
                let rect = self.input_coordinator.borrow_mut().widget_rect(
                    &uzor::input::WidgetId(widget_id_str),
                );
                let (ax, ay, aw, ah) = rect
                    .map(|r| (r.x, r.y, r.width, r.height))
                    .unwrap_or((0.0, 0.0, 0.0, 0.0));

                self.panel_app.primitive_settings_state.open_color_picker_smart(
                    "stroke_color", ax, ay, aw, ah, screen_w, screen_h,
                    current_color.as_deref(),
                );
                eprintln!("[ChartApp] inline: opened stroke color picker for #{}", idx);
            }

            // ── Text color ───────────────────────────────────────────────────
            "inline:text_color" => {
                let current_color = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx))
                    .and_then(|d| d.text.and_then(|t| t.color));

                let widget_id_str = "csb:inline:text_color".to_string();
                let rect = self.input_coordinator.borrow_mut().widget_rect(
                    &uzor::input::WidgetId(widget_id_str),
                );
                let (ax, ay, aw, ah) = rect
                    .map(|r| (r.x, r.y, r.width, r.height))
                    .unwrap_or((0.0, 0.0, 0.0, 0.0));

                self.panel_app.primitive_settings_state.open_color_picker_smart(
                    "text_color", ax, ay, aw, ah, screen_w, screen_h,
                    current_color.as_deref(),
                );
                eprintln!("[ChartApp] inline: opened text color picker for #{}", idx);
            }

            // ── Delete ───────────────────────────────────────────────────────
            "inline:delete" => {
                // Capture the primitive ID before deletion so alerts can be cleaned up.
                let prim_id = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx))
                    .map(|d| d.id);
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.delete_selected();
                    eprintln!("[ChartApp] inline: deleted selected primitive");
                }
                if let Some(pid) = prim_id {
                    self.alert_manager.remove_alerts_for_drawing(pid);
                }
            }

            // ── Lock toggle ──────────────────────────────────────────────────
            "inline:lock" => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.toggle_selected_lock();
                    eprintln!("[ChartApp] inline: toggled lock on selected primitive");
                }
            }

            // ── Line style cycle ─────────────────────────────────────────────
            "inline:style" => {
                let data_opt = self.panel_app.panel_grid
                    .active_window()
                    .and_then(|w| w.drawing_manager.get_data_at(idx));

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
                    eprintln!("[ChartApp] inline: cycled line style to {}", style_str);
                }
            }

            // ── Width cycle ──────────────────────────────────────────────────
            "inline:width" => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.increase_selected_width();
                    eprintln!("[ChartApp] inline: increased line width");
                }
            }

            // ── Name label — absorb, no action ──────────────────────────────
            "inline:name" => {
                eprintln!("[ChartApp] inline: name label clicked (no-op)");
            }

            // ── Alert — open alert settings modal for selected primitive ────
            "inline:alert" => {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(prim) = window.drawing_manager.selected_primitive() {
                        let prim_id = prim.data().id;
                        let label = prim.display_name().to_string();
                        let pts = prim.points();
                        let price = if !pts.is_empty() {
                            pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64
                        } else {
                            window.bars.last().map(|b| b.close).unwrap_or(0.0)
                        };
                        let symbol = window.symbol.clone();
                        let source = alerts::AlertSource::Drawing { primitive_id: prim_id, label };
                        self.panel_app.alert_settings_state.open_new(source, &symbol, price);
                        self.panel_app.alert_settings_state.pin_initial_position(
                            self.content_rect.width, self.content_rect.height,
                        );
                        eprintln!("[ChartApp] inline alert: opened modal for primitive {}", prim_id);
                    }
                }
            }

            // ── More — not yet implemented ───────────────────────────────────
            "inline:more" => {
                eprintln!("[ChartApp] inline: 'inline:more' not yet implemented");
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

            // ── Background absorber — close inline dropdown ──────────────────
            "inline_dropdown:__bg__" => {
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ if action.starts_with("inline:style_option:") => {
                // ── Style option selected ────────────────────────────────────
                use zengeld_chart::drawing::primitives_v2::LineStyle;
                let value = action.strip_prefix("inline:style_option:").unwrap_or("");
                let new_style = match value {
                    "solid"        => Some(LineStyle::Solid),
                    "dashed"       => Some(LineStyle::Dashed),
                    "dotted"       => Some(LineStyle::Dotted),
                    "large_dashed" => Some(LineStyle::LargeDashed),
                    "sparse_dotted" => Some(LineStyle::SparseDotted),
                    _ => None,
                };
                if let Some(style) = new_style {
                    if let Some(mut data) = self.panel_app.panel_grid
                        .active_window()
                        .and_then(|w| w.drawing_manager.get_data_at(idx))
                    {
                        data.style = style;
                        let style_str = data.style.as_str().to_string();
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.set_data_at(idx, &data);
                        }
                        eprintln!("[ChartApp] inline: set line style to {}", style_str);
                    }
                }
                self.panel_app.toolbar_state.open_inline_style_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ if action.starts_with("inline:width_option:") => {
                // ── Width option selected ────────────────────────────────────
                let value = action.strip_prefix("inline:width_option:").unwrap_or("");
                if let Ok(new_width) = value.parse::<f64>() {
                    if let Some(mut data) = self.panel_app.panel_grid
                        .active_window()
                        .and_then(|w| w.drawing_manager.get_data_at(idx))
                    {
                        data.width = new_width;
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.set_data_at(idx, &data);
                        }
                        eprintln!("[ChartApp] inline: set line width to {}", new_width);
                    }
                }
                self.panel_app.toolbar_state.open_inline_width_dropdown = false;
                self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
            }

            _ => {
                eprintln!("[ChartApp] inline: unhandled action '{}'", action);
            }
        }
    }

    /// Handle clicks on widgets registered with the "wl_modal:" prefix.
    fn handle_watchlist_modal_click(&mut self, rest: &str, x: f64, _y: f64) {
        match rest {
            "close" => {
                self.watchlist_modal.close();
                eprintln!("[WatchlistModal] closed");
            }
            "modal_bg" | "header_drag" | "list_scroll" => {
                // Absorbed — no-op for backdrop clicks, drag zone, scroll zone
            }
            "search_input" => {
                // Click-to-cursor using pre-computed character positions
                if let Some(ref wl) = self.last_watchlist_modal_result {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(
                        &wl.search_char_positions,
                        x,
                    );
                    self.watchlist_modal.search_editing.cursor = new_cursor;
                    self.watchlist_modal.search_editing.selection_start = None;
                    self.watchlist_modal.search_editing.reset_blink(0);
                }
            }
            _ if rest.starts_with("tab:") => {
                use zengeld_chart::ui::modal_settings::WatchlistModalTab;
                let tab = &rest["tab:".len()..];
                match tab {
                    "overview" => self.watchlist_modal.active_tab = WatchlistModalTab::Overview,
                    "groups"   => self.watchlist_modal.active_tab = WatchlistModalTab::Groups,
                    "settings" => self.watchlist_modal.active_tab = WatchlistModalTab::Settings,
                    _ => {}
                }
                eprintln!("[WatchlistModal] tab: {}", tab);
            }
            _ if rest.starts_with("item:") => {
                let composite = &rest["item:".len()..];
                // Parse composite key "SYMBOL:exchange"
                let (sym_part, exchange_part) = if let Some(colon_pos) = composite.rfind(':') {
                    (&composite[..colon_pos], &composite[colon_pos + 1..])
                } else {
                    (composite, self.active_exchange.as_str())
                };
                // Resolve ExchangeId from string.
                let resolved_exchange = self.exchange_symbols
                    .keys()
                    .find(|eid| eid.as_str() == exchange_part)
                    .copied()
                    .unwrap_or(self.active_exchange);
                // Switch the active chart to this symbol+exchange.
                let timeframe = self.panel_app.panel_grid.active_window()
                    .map(|w| w.timeframe.clone())
                    .unwrap_or_default();
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    let old_sym = window.symbol.clone();
                    window.snapshot_drawings_for_symbol(&old_sym);
                    window.symbol = sym_part.to_string();
                    window.exchange = exchange_part.to_string();
                    window.update_title();
                    window.bars.clear();
                    window.drawing_manager.clear_all_primitives();
                    window.restore_drawings_for_symbol(sym_part);
                }
                self.active_exchange = resolved_exchange;
                self.bridge.unsubscribe_all();
                let eid_str = resolved_exchange.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (watchlist modal item click)", eid_str);
                } else {
                    self.bridge.ensure_connector(resolved_exchange);
                    self.bridge.request_bars(resolved_exchange, sym_part, &timeframe, None, Some(self.panel_app.user_manager.profile.bar_count as usize));
                }
                self.autosave_snapshot();
                eprintln!("[WatchlistModal] symbol selected: {} @ {}", sym_part, exchange_part);
                self.watchlist_modal.close();
            }
            _ if rest.starts_with("delete:") => {
                let key = &rest["delete:".len()..];
                // Parse composite key "SYMBOL:exchange" or plain symbol
                let (symbol, exchange_owned): (&str, String) = if let Some(colon_pos) = key.rfind(':') {
                    (&key[..colon_pos], key[colon_pos + 1..].to_string())
                } else {
                    // Fallback: look up exchange from watchlist
                    let ex = self.sidebar_state.watchlist_manager.active_list()
                        .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == key).map(|ws| ws.exchange.clone()))
                        .unwrap_or_else(|| self.active_exchange.as_str().to_string());
                    (key, ex)
                };
                let exchange = exchange_owned.as_str();
                // Remove symbol from snapshot (if active) before removing from list.
                if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                    if let Some(ref mut snap) = list.order_snapshot {
                        snap.retain(|s| !(s.symbol == symbol && s.exchange == exchange));
                    }
                }
                self.sidebar_state.watchlist_manager.remove_symbol(symbol, exchange);
                self.watchlist_actions.push(crate::WatchlistAction::Remove { symbol: symbol.to_string(), exchange: exchange.to_string() });
                self.watchlists_dirty = true;
                self.persist_watchlists();
                eprintln!("[WatchlistModal] symbol removed: {} @ {}", symbol, exchange);
            }
            // Switch active watchlist
            _ if rest.starts_with("group:") => {
                let id_str = &rest["group:".len()..];
                if let Ok(id) = id_str.parse::<u64>() {
                    self.sidebar_state.watchlist_manager.active_list_id = id;
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
                    let deleted = self.sidebar_state.watchlist_manager.delete_list(id);
                    self.watchlist_actions.push(crate::WatchlistAction::DeleteList { id });
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[WatchlistModal] delete list id={} -> {}", id, deleted);
                }
            }
            // Create new watchlist — open name input modal with auto-generated name
            "group_add" => {
                let max_n = self.sidebar_state.watchlist_manager.lists.iter()
                    .filter_map(|l| l.name.strip_prefix("Watchlist "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("Watchlist {}", max_n + 1);
                self.wl_group_name_input.open_create_new_with_name(&default_name);
                eprintln!("[WatchlistModal] opening group name input for create new: '{}'", default_name);
            }
            // Rename a watchlist — open name input modal pre-filled with current name
            _ if rest.starts_with("group_rename:") => {
                let id_str = &rest["group_rename:".len()..];
                if let Ok(id) = id_str.parse::<u64>() {
                    if let Some(list) = self.sidebar_state.watchlist_manager.lists.iter().find(|l| l.id == id) {
                        let name = list.name.clone();
                        self.wl_group_name_input.open_rename(id, &name);
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
    fn handle_indicator_search_action(&mut self, rest: &str) {
        match rest {
            "sets_tab" => {
                self.modal_state.toggle_indicator_sets();
                eprintln!("[ChartApp] ind_search: toggled sets view (now={})", self.modal_state.show_indicator_sets);
            }
            "set_create" => {
                // Compute next available name: "My Indicator Set 1", "My Indicator Set 2", etc.
                let max_n = self.panel_app.template_manager.indicator_sets
                    .iter()
                    .filter_map(|s| s.name.strip_prefix("My Indicator Set "))
                    .filter_map(|s| s.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0);
                let default_name = format!("My Indicator Set {}", max_n + 1);

                let current_time_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                self.panel_app.preset_name_input.open_create_indicator_set(
                    &default_name,
                    current_time_ms,
                );
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
    fn deploy_indicator_set(&mut self, set_id: &str) {
        let set = self.panel_app.template_manager.indicator_sets
            .iter()
            .find(|s| s.id == set_id)
            .cloned();

        let Some(set) = set else {
            eprintln!("[ChartApp] deploy_indicator_set: set not found: {}", set_id);
            return;
        };

        let symbol = self.panel_app.panel_grid.active_window()
            .map(|w| w.symbol.clone())
            .unwrap_or_default();

        // Remove all existing indicators (exclusive deployment, like preset)
        let existing_ids: Vec<u64> = self.indicator_manager
            .get_instances_for_symbol(&symbol)
            .iter()
            .map(|inst| inst.id)
            .collect();
        for id in &existing_ids {
            self.indicator_manager.remove_instance(*id);
        }
        // Clear stale overlay states
        self.panel_app.indicator_overlay_states.clear();
        eprintln!("[ChartApp] deploy_indicator_set: removed {} existing indicators", existing_ids.len());

        // Create new indicators from the set with full params/outputs
        for tmpl in &set.indicators {
            if let Some(new_id) = self.indicator_manager.create_instance(&tmpl.type_id, &symbol) {
                if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                        if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                            inst.window_id = Some(chart_id.0);
                        }
                    }
                }
                // Apply saved params, output styles, and visibility from the template
                if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                    // Restore instance-level visibility
                    inst.visible = tmpl.visible;
                    // Restore params
                    for (k, v) in &tmpl.params {
                        if let Ok(iv) = serde_json::from_value(v.clone()) {
                            inst.params.insert(k.clone(), iv);
                        }
                    }
                    // Restore output styles
                    for (key, out_style) in &tmpl.outputs {
                        let entry = inst.outputs.entry(key.clone())
                            .or_insert_with(Default::default);
                        if let Some(ref c) = out_style.color {
                            entry.color = Some(c.clone());
                        }
                        if let Some(lw) = out_style.line_width {
                            entry.line_width = Some(lw);
                        }
                        if let Some(vis) = out_style.visible {
                            entry.visible = vis;
                        }
                    }
                }
                self.push_undo_command(zengeld_chart::Command::AddIndicator {
                    instance_id: new_id,
                    type_id: tmpl.type_id.clone(),
                    params_json: String::new(),
                });
            }
        }

        let bars = self.panel_app.panel_grid.active_window()
            .map(|w| w.bars.clone())
            .unwrap_or_default();
        self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
        self.sync_sub_panes_from_manager();
        self.autosave_snapshot();
        self.modal_state.close();
        eprintln!("[ChartApp] deployed indicator set '{}' ({} indicators, replaced {})", set.name, set.indicators.len(), existing_ids.len());
    }

    /// Save the current active indicators as a named indicator set.
    ///
    /// Called when the user confirms a name in the Create Indicator Set modal.
    /// Captures a full snapshot of each indicator's params and output styles.
    fn execute_create_indicator_set(&mut self, name: String) {
        use zengeld_chart::templates::{IndicatorTemplate, OutputStyleConfig};

        let symbol = self.panel_app.panel_grid.active_window()
            .map(|w| w.symbol.clone())
            .unwrap_or_default();

        let instances = self.indicator_manager.get_instances_for_symbol(&symbol);
        let templates: Vec<IndicatorTemplate> = instances
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
            .collect();

        let set = zengeld_chart::templates::indicator_set::IndicatorSet::from_templates(
            &name,
            templates,
        );
        eprintln!("[ChartApp] created indicator set '{}' from {} active indicators", name, instances.len());
        self.template_actions.push(crate::TemplateAction::AddIndicatorSet(set));
    }

    /// Handle clicks on widgets registered with the "modal_search:" prefix.
    fn handle_search_modal_click(&mut self, rest: &str, x: f64, _y: f64) {
        match rest {
            "close" => {
                self.modal_state.close();
                eprintln!("[ChartApp] search modal closed");
            }
            "modal_bg" => {
                // Click inside modal body — no-op (prevents backdrop close)
            }
            "search_input" => {
                // Activate text editing on the search input, positioning cursor at click point
                let char_positions = self.search_modal_result.as_ref()
                    .map(|ms| ms.search_char_positions.clone());
                if let Some(positions) = char_positions {
                    let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&positions, x);
                    // Start editing if not already
                    if self.modal_state.editing_text.is_none() {
                        self.modal_state.start_editing(0);
                    }
                    if let Some(ref mut edit) = self.modal_state.editing_text {
                        edit.cursor = new_cursor;
                        edit.selection_start = None;
                    }
                } else {
                    self.modal_state.start_editing(0);
                }
                eprintln!("[ChartApp] search input activated");
            }
            _ if rest.starts_with("star:") => {
                let composite = &rest["star:".len()..];
                // Composite key format: "SYMBOL:exchange_id" — split on the last colon.
                let (sym_part, exchange_part) = if let Some(colon_pos) = composite.rfind(':') {
                    (&composite[..colon_pos], &composite[colon_pos + 1..])
                } else {
                    (composite, self.active_exchange.as_str())
                };
                // Queue action for App to apply on AppState (single source of truth).
                self.watchlist_actions.push(crate::WatchlistAction::Toggle {
                    symbol: sym_part.to_string(),
                    exchange: exchange_part.to_string(),
                });
                self.watchlists_dirty = true;
                eprintln!("[ChartApp] star toggle queued: {}:{}", sym_part, exchange_part);
            }
            _ if rest.starts_with("item:") => {
                let item_id = &rest["item:".len()..];
                eprintln!("[ChartApp] search modal item selected: {}", item_id);

                // Parse composite key "SYMBOL:exchange_id" (split on the LAST colon so that
                // symbols containing colons or hyphens are handled correctly).
                let (symbol_part, exchange_id_part) = if let Some(colon_pos) = item_id.rfind(':') {
                    (&item_id[..colon_pos], &item_id[colon_pos + 1..])
                } else {
                    // No colon — treat the whole string as the symbol and fall back to
                    // the current active exchange.
                    (item_id, "")
                };

                // Resolve the exchange from the parsed exchange_id string.
                // Walk the known exchange_symbols keys to find a matching entry so we
                // never need a hard-coded string→ExchangeId mapping.
                let resolved_exchange = if exchange_id_part.is_empty() {
                    self.active_exchange
                } else {
                    self.exchange_symbols
                        .keys()
                        .find(|eid| eid.as_str() == exchange_id_part)
                        .copied()
                        .unwrap_or(self.active_exchange)
                };

                match self.modal_state.current {
                    OpenModal::SymbolSearch => {
                        // Capture previous symbol and active leaf BEFORE changing.
                        let previous_symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                        let new_symbol_str = symbol_part.to_string();
                        // Set symbol, clear bars, and request data asynchronously.
                        let timeframe = self.panel_app.panel_grid.active_window()
                            .map(|w| w.timeframe.clone())
                            .unwrap_or_default();
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            // Snapshot current drawings before switching symbol
                            let old_sym = window.symbol.clone();
                            window.snapshot_drawings_for_symbol(&old_sym);
                            window.symbol = symbol_part.to_string();
                            window.exchange = resolved_exchange.as_str().to_string();
                            window.update_title();
                            window.bars.clear();
                            window.drawing_manager.clear_all_primitives();
                            window.restore_drawings_for_symbol(symbol_part);
                        }
                        // Switch to the exchange that owns this symbol and request bars.
                        self.active_exchange = resolved_exchange;
                        self.bridge.unsubscribe_all();
                        let eid_str = resolved_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (search symbol select)", eid_str);
                        } else {
                            self.bridge.ensure_connector(resolved_exchange);
                            self.bridge.request_bars(resolved_exchange, symbol_part, &timeframe, None, Some(self.panel_app.user_manager.profile.bar_count as usize));
                        }
                        // Record ChangeSymbol if it actually changed.
                        if previous_symbol != new_symbol_str {
                            self.push_undo_command(zengeld_chart::Command::ChangeSymbol {
                                previous_symbol: previous_symbol.clone(),
                                new_symbol: new_symbol_str.clone(),
                            });
                            eprintln!("[ChartApp] Recorded ChangeSymbol {} -> {}", previous_symbol, new_symbol_str);
                            // Propagate new symbol to all leaves in the same sync group.
                            if let Some(leaf) = active_leaf {
                                self.propagate_symbol_to_sync_group(leaf, &new_symbol_str);
                            }
                        }
                        // Recalculate all indicators for the new symbol.
                        let (sym, bars) = self.panel_app.panel_grid.active_window()
                            .map(|w| (w.symbol.clone(), w.bars.clone()))
                            .unwrap_or_default();
                        self.indicator_manager.calculate_all_for_symbol(&sym, &bars);
                        self.sync_sub_panes_from_manager();
                        self.autosave_snapshot();
                        self.modal_state.close();
                    }
                    OpenModal::CompareSearch => {
                        // Add the selected symbol as a compare overlay on the active chart window.
                        // ChartWindow::add_compare_symbol() fetches bars via the attached
                        // DemoDataProvider (which generates synthetic data for any symbol) and
                        // adds a CompareSeries with percentage normalization already handled.
                        let added_series = if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let added = window.add_compare_symbol(symbol_part);
                            eprintln!(
                                "[ChartApp] Compare symbol {}: {}",
                                symbol_part,
                                if added { "added" } else { "already present or unavailable" }
                            );
                            if added {
                                // Capture the newly-added series for the undo record.
                                window.compare_overlay.get_series(symbol_part).cloned()
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        // Record AddCompareSeries so the user can undo adding this overlay.
                        if let Some(series) = added_series {
                            self.push_undo_command(zengeld_chart::Command::AddCompareSeries {
                                series,
                            });
                            eprintln!("[ChartApp] Recorded AddCompareSeries {}", symbol_part);
                            self.autosave_snapshot();
                        }
                        self.modal_state.close();
                    }
                    OpenModal::IndicatorSearch => {
                        // Create indicator instance from catalog.
                        let symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        let type_id_str = item_id.to_string();
                        if let Some(new_id) = self.indicator_manager.create_instance(item_id, &symbol) {
                            // Set window_id for the new instance so it's scoped to this window.
                            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                                if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                                    if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                                        inst.window_id = Some(chart_id.0);
                                    }
                                    // If this window is in a sync group, also track the config there.
                                    // Use new_id as the config id so deletion can match by instance id.
                                    if let Some(group_id) = self.panel_app.panel_grid
                                        .window_for_leaf(active_leaf)
                                        .and_then(|w| w.group_id)
                                    {
                                        let inst_pane = self.indicator_manager
                                            .get_instance(new_id)
                                            .map(|i| i.pane as u32)
                                            .unwrap_or(0);
                                        let inst_name = self.indicator_manager
                                            .get_instance(new_id)
                                            .map(|i| i.name.clone())
                                            .unwrap_or_else(|| item_id.to_string());
                                        if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                                            group.indicator_configs.push(
                                                zengeld_chart::tag_manager::IndicatorGroupConfig {
                                                    id: new_id,
                                                    type_id: item_id.to_string(),
                                                    name: inst_name,
                                                    params: std::collections::HashMap::new(),
                                                    pane: inst_pane,
                                                    visible: true,
                                                    symbol: symbol.clone(),
                                                },
                                            );
                                            eprintln!(
                                                "[TagManager] Added indicator config id={} '{}' to group {:?}",
                                                new_id, item_id, group_id
                                            );
                                        }
                                        // Sync the new indicator to all peer windows in the group.
                                        self.sync_group_indicator_to_peers(group_id, item_id, active_leaf);
                                    }
                                }
                            }
                            // Record AddIndicator command.
                            self.push_undo_command(zengeld_chart::Command::AddIndicator {
                                instance_id: new_id,
                                type_id: type_id_str.clone(),
                                params_json: String::new(),
                            });
                            eprintln!("[ChartApp] Recorded AddIndicator {} id={}", type_id_str, new_id);
                            // Recalculate the new indicator with current bars.
                            let bars: Option<Vec<zengeld_chart::Bar>> = self.panel_app
                                .panel_grid
                                .active_window()
                                .map(|w| w.bars.clone());
                            if let Some(bars) = bars {
                                self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
                            }
                            self.sync_sub_panes_from_manager();
                            self.autosave_snapshot();
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

    /// Handle a click that landed on the chart canvas (not a widget).
    fn handle_canvas_click(&mut self, x: f64, y: f64) {
        // 1. Check scale corner buttons first.
        match self.scale_corner_zones.hit_test(x, y) {
            ScaleCornerButton::AutoManual => {
                let current_mode = self.panel_app.panel_grid
                    .active_window()
                    .map(|w| w.price_scale.scale_mode)
                    .unwrap_or(ScaleMode::Auto);
                let next_mode = current_mode.next();
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.price_scale.scale_mode = next_mode;
                    if next_mode.is_follow() {
                        // Focus mode: position viewport to last bar + 2 bar right margin
                        let count = window.bars.len();
                        let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
                        let right_margin = 2.0_f64;
                        window.viewport.view_start = (count as f64 + right_margin - visible_f).max(0.0);
                    }
                    if next_mode.is_auto_y() {
                        window.calc_auto_scale();
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

        // 2. Drawing tool check — if a tool is active, convert the click to
        //    chart-local data coordinates and forward to DrawingManager.on_click().
        //
        //    Handles both main chart and sub-pane areas.  Sub-panes share the
        //    X-axis (bar index) with the main chart but have their own Y-axis
        //    (price range = indicator values, e.g. RSI 0-100).
        let tool_id_opt = self.panel_app.toolbar_state.active_tool_id.clone();
        if let Some(ref _tool_id) = tool_id_opt {
            // Build the extended layout so we get the exact same main_chart rect
            // that render_full_chart_panel uses.  This accounts for sub-panes
            // (RSI, MACD, …) that reduce the main chart height below
            // viewport.chart_height.
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;

            let is_freehand = self.panel_app.panel_grid.active_window()
                .map(|w| w.drawing_manager.is_freehand_tool())
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .map(|w| (w.price_scale.price_min, w.price_scale.price_max))
                    .unwrap_or((0.0, 100.0));

                let chart_height = chart_rect.height;
                let bar = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        // Snap to bar center (matching crosshair coordinate system).
                        if let Some(idx) = w.viewport.x_to_bar(local_x_main) {
                            idx as f64
                        } else {
                            w.viewport.x_to_bar_f64(local_x_main)
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
                let price = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        if w.crosshair.is_magnet() {
                            let bar_idx = w.viewport.x_to_bar(local_x_main);
                            let (snapped_price, _) = w.calculate_magnet_snap(
                                bar_idx, raw_price, chart_height, price_min, price_max,
                            );
                            snapped_price
                        } else {
                            raw_price
                        }
                    })
                    .unwrap_or(raw_price);

                // Reset pane context to main chart.
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.set_current_pane(None);
                    if window.drawing_manager.current_tool().map(|t| t != _tool_id).unwrap_or(true) {
                        window.drawing_manager.set_tool(Some(_tool_id));
                    }
                }

                // Dispatch click and record undo if a primitive was just completed.
                eprintln!("[CANVAS_CLICK] screen=({:.0},{:.0}) -> bar={:.2}, price={:.2}", x, y, bar, price);
                let primitive_created = self.panel_app.panel_grid.active_window_mut()
                    .map(|w| w.drawing_manager.on_click(bar, price))
                    .unwrap_or(false);

                if primitive_created {
                    eprintln!("[ChartApp] Primitive created (main) at bar={:.2}, price={:.2}", bar, price);
                    // For grouped windows: move the completed primitive to TagManager so
                    // all group members see it via the per-frame render-cache sync.
                    // For standalone windows: keep it in drawing_manager as before.
                    if !self.intercept_completed_primitive_to_group() {
                        // Standalone path — record undo and propagate to color-tag peers.
                        // Extract snapshot before mutable borrow for push_undo_command.
                        let create_snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let idx = w.drawing_manager.last_index()?;
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = create_snapshot {
                            self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            eprintln!("[ChartApp] Recorded CreatePrimitive (main) at index {}", idx);
                        }
                        // Propagate the new primitive to sync group peers.
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_new_primitive_to_sync_group(active_leaf);
                            // Clear the in-progress preview on peers (state is now Idle).
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    } else {
                        // Grouped path — record undo for the primitive placed into the group,
                        // then clear the in-progress preview on peers.
                        let group_create_cmd = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.group_id)
                            .and_then(|gid| {
                                let group = self.panel_app.tag_manager.group(gid)?;
                                let idx = group.primitives.len().saturating_sub(1);
                                let prim = group.primitives.get(idx)?;
                                Some(zengeld_chart::Command::CreatePrimitive {
                                    index: idx,
                                    type_id: prim.type_id().to_string(),
                                    points: prim.points().to_vec(),
                                    data: prim.data().clone(),
                                })
                            });
                        if let Some(cmd) = group_create_cmd {
                            self.push_undo_command(cmd);
                            eprintln!("[ChartApp] Recorded CreatePrimitive (main, grouped) via group.primitives");
                        }
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    }
                    // Save after both grouped and standalone paths — intercept may have moved
                    // the primitive to TagManager, so snapshot must be taken after transfer.
                    self.autosave_snapshot();
                } else if self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.is_drawing())
                    .unwrap_or(false)
                {
                    eprintln!("[ChartApp] Drawing point added (main) at bar={:.2}, price={:.2}", bar, price);
                    // Propagate in-progress drawing state so peers show the preview.
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                }

                // Re-check if tool was completed (primed tools deactivate after creation).
                let tool_cleared = self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.current_tool().is_none())
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .and_then(|w| {
                        w.sub_panes.iter()
                            .find(|sp| sp.instance_id == instance_id)
                            .map(|sp| (sp.price_min, sp.price_max))
                    })
                    .unwrap_or((0.0, 100.0));

                let bar = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        // Snap to bar center (matching crosshair coordinate system).
                        if let Some(idx) = w.viewport.x_to_bar(local_x) {
                            idx as f64
                        } else {
                            w.viewport.x_to_bar_f64(local_x)
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.set_current_pane(Some(instance_id));
                    if window.drawing_manager.current_tool().map(|t| t != _tool_id).unwrap_or(true) {
                        window.drawing_manager.set_tool(Some(_tool_id));
                    }
                }

                // Dispatch click and record undo if a primitive was just completed.
                let primitive_created = self.panel_app.panel_grid.active_window_mut()
                    .map(|w| w.drawing_manager.on_click(bar, price))
                    .unwrap_or(false);

                if primitive_created {
                    eprintln!("[ChartApp] Primitive created (sub-pane #{}) at bar={:.2}, price={:.2}", pane_idx, bar, price);
                    // For grouped windows: move the completed primitive to TagManager.
                    // For standalone windows: keep in drawing_manager and propagate to peers.
                    if !self.intercept_completed_primitive_to_group() {
                        // Standalone path — extract snapshot then push undo command.
                        let create_snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let idx = w.drawing_manager.last_index()?;
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((idx, type_id, points, data))
                            });
                        if let Some((idx, type_id, points, data)) = create_snapshot {
                            self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            eprintln!("[ChartApp] Recorded CreatePrimitive (sub-pane #{}) at index {}", pane_idx, idx);
                        }
                        // Propagate the new primitive to sync group peers.
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_new_primitive_to_sync_group(active_leaf);
                            // Clear the in-progress preview on peers (state is now Idle).
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    } else {
                        // Grouped path — record undo for the primitive placed into the group,
                        // then clear the in-progress preview on peers.
                        let group_create_cmd = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.group_id)
                            .and_then(|gid| {
                                let group = self.panel_app.tag_manager.group(gid)?;
                                let idx = group.primitives.len().saturating_sub(1);
                                let prim = group.primitives.get(idx)?;
                                Some(zengeld_chart::Command::CreatePrimitive {
                                    index: idx,
                                    type_id: prim.type_id().to_string(),
                                    points: prim.points().to_vec(),
                                    data: prim.data().clone(),
                                })
                            });
                        if let Some(cmd) = group_create_cmd {
                            self.push_undo_command(cmd);
                            eprintln!("[ChartApp] Recorded CreatePrimitive (sub-pane #{}, grouped) via group.primitives", pane_idx);
                        }
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            self.propagate_drawing_state_to_sync_group(active_leaf);
                        }
                    }
                    // Save after both grouped and standalone paths — intercept may have moved
                    // the primitive to TagManager, so snapshot must be taken after transfer.
                    self.autosave_snapshot();
                } else if self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.is_drawing())
                    .unwrap_or(false)
                {
                    eprintln!("[ChartApp] Drawing point added (sub-pane #{}) at bar={:.2}, price={:.2}", pane_idx, bar, price);
                    // Propagate in-progress drawing state so peers show the preview.
                    if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                        self.propagate_drawing_state_to_sync_group(active_leaf);
                    }
                }

                // Sync toolbar if tool completed.
                let tool_cleared = self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.current_tool().is_none())
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
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;
            let local_x = x - chart_rect.x;
            let local_y = y - chart_rect.y;

            // -- Main chart primitive hit test --
            if local_x >= 0.0 && local_x <= chart_rect.width
                && local_y >= 0.0 && local_y <= chart_rect.height
            {
                if let Some(window) = self.panel_app.panel_grid.active_window() {
                    if let Some(prim_idx) = window.drawing_manager.hit_test(
                        local_x, local_y, &window.viewport, &window.price_scale,
                    ) {
                        // Select the primitive, clear indicator selection, and return.
                        self.selected_indicator_id = None;
                        if let Some(win) = self.panel_app.panel_grid.active_window_mut() {
                            win.drawing_manager.set_current_pane(None);
                            win.drawing_manager.select_by_index(prim_idx);
                            eprintln!("[ChartApp] Primitive selected (main): index={}", prim_idx);
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .and_then(|w| {
                        w.sub_panes.iter()
                            .find(|sp| sp.instance_id == instance_id)
                            .map(|sp| (sp.price_min, sp.price_max))
                    })
                    .unwrap_or((0.0, 100.0));
                let sub_price_scale = zengeld_chart::PriceScale::new(price_min, price_max);
                // Use a viewport with pane height so hit_test_in_pane coordinate conversion is correct.
                let sub_viewport = self.panel_app.panel_grid.active_window()
                    .map(|w| {
                        let mut vp = w.viewport.clone();
                        vp.chart_height = content.height;
                        vp
                    });
                if let (Some(sub_viewport), Some(window)) = (sub_viewport, self.panel_app.panel_grid.active_window()) {
                    if let Some(prim_idx) = window.drawing_manager.hit_test_in_pane(
                        plx, ply, instance_id, &sub_viewport, &sub_price_scale,
                    ) {
                        // Select the primitive, clear indicator selection, and return.
                        self.selected_indicator_id = None;
                        if let Some(win) = self.panel_app.panel_grid.active_window_mut() {
                            win.drawing_manager.set_current_pane(Some(instance_id));
                            win.drawing_manager.select_by_index(prim_idx);
                            eprintln!("[ChartApp] Primitive selected (sub-pane #{}): index={}", pane_idx, prim_idx);
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
                let (price_min, price_max) = self.panel_app.panel_grid.active_window()
                    .and_then(|w| {
                        w.sub_panes.iter()
                            .find(|p| p.instance_id == instance_id)
                            .map(|p| (p.price_min, p.price_max))
                    })
                    .unwrap_or((0.0, 100.0));
                let hit = self.panel_app.panel_grid.active_window()
                    .map(|window| {
                        self.indicator_manager.hit_test_sub_pane(
                            instance_id,
                            plx, ply,
                            &window.viewport,
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
                        if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                            w.drawing_manager.deselect();
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
                let ind_hit = self.panel_app.panel_grid.active_window()
                    .and_then(|window| {
                        self.indicator_manager.hit_test_overlay(
                            local_x,
                            local_y,
                            &window.symbol,
                            &window.viewport,
                            &window.price_scale,
                            chart_rect.height,
                            8.0,
                        )
                    });
                if let Some(ind_id) = ind_hit {
                    self.selected_indicator_id = Some(ind_id);
                    // Deselect any drawing primitive when an indicator is selected.
                    if let Some(win) = self.panel_app.panel_grid.active_window_mut() {
                        if win.drawing_manager.selected().is_some() {
                            win.drawing_manager.deselect();
                        }
                    }
                    eprintln!("[ChartApp] Indicator selected: id={}", ind_id);
                    return;
                }
            }
        }

        // No primitive or indicator hit — deselect everything.
        self.selected_indicator_id = None;
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            if window.drawing_manager.selected().is_some() {
                window.drawing_manager.deselect();
            }
        }

        let extended = self.build_extended_layout();
        let hit_tester = ExtendedLayoutHitTester::new(&extended);
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
    fn close_topmost_modal_layer(&mut self) {
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
                "modal_search" => {
                    self.modal_state.close();
                }
                // Top-level modals — close when user clicks outside them.
                "overlay_settings" => {
                    self.panel_app.close_overlay_settings();
                    eprintln!("[ChartApp] close_topmost_modal_layer: overlay_settings closed");
                }
                "tags_tabs" => {
                    self.panel_app.close_tags_tabs();
                    eprintln!("[ChartApp] close_topmost_modal_layer: tags_tabs closed");
                }
                "chart_browser" => {
                    self.panel_app.chart_browser.close();
                    self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
                    eprintln!("[ChartApp] close_topmost_modal_layer: chart_browser closed");
                }
                "user_settings" => {
                    self.panel_app.user_settings_state.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: user_settings closed");
                }
                // Preset name input — close only this modal, not the search overlay underneath.
                "preset_name_input" => {
                    self.panel_app.preset_name_input.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: preset_name_input closed");
                }
                // Watchlist group name input L2 modal — close only this layer.
                "wl_group_name_input" => {
                    self.wl_group_name_input.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: wl_group_name_input closed");
                }
                // Watchlist modal — close when user clicks outside it.
                "watchlist_modal" => {
                    self.watchlist_modal.close();
                    eprintln!("[ChartApp] close_topmost_modal_layer: watchlist_modal closed");
                }
                _ => {
                    // Unknown layer — fall back to closing everything so the UI
                    // never gets stuck in an unrecoverable state.
                    eprintln!("[ChartApp] close_topmost_modal_layer: unknown layer '{}', closing all", name);
                    self.close_open_modals();
                }
            }
        }
    }

    /// Close all open modals (state lives inside panel_app at checkpoint).
    fn close_open_modals(&mut self) {
        // Close search/compare/indicator-search overlays (SymbolSearch, CompareSearch, IndicatorSearch).
        self.modal_state.close();
        self.panel_app.primitive_settings_state.close();
        self.panel_app.chart_settings_state.close();
        self.panel_app.indicator_settings_state.close();
        // Close top-level modals that are managed outside of modal_state.
        self.panel_app.close_overlay_settings();
        self.panel_app.close_tags_tabs();
        if self.panel_app.chart_browser.is_open {
            self.panel_app.chart_browser.close();
            self.panel_app.active_modal = zengeld_chart::modal::ChartOpenModal::None;
        }
    }

    /// Handle a context menu item click.
    fn on_context_menu_action(&mut self, action: &str) {
        let target = self.panel_app.context_menu_state.target.clone();
        self.panel_app.context_menu_state.close();

        match target {
            ContextMenuTarget::Primitive(idx) => {
                match action {
                    "settings" => {
                        self.panel_app.primitive_settings_state.open(idx);
                        eprintln!("[ChartApp] Opened primitive settings for #{}", idx);
                    }
                    "delete" => {
                        // Capture state BEFORE deletion so undo can recreate.
                        let snapshot = self.panel_app.panel_grid.active_window()
                            .and_then(|w| {
                                let type_id = w.drawing_manager.get_type_id_at(idx)?;
                                let points = w.drawing_manager.get_points_at(idx)?;
                                let data = w.drawing_manager.get_data_at(idx)?;
                                Some((type_id, points, data))
                            });
                        if let Some((type_id, points, data)) = snapshot {
                            let prim_id = data.id;
                            self.push_undo_command(zengeld_chart::Command::DeletePrimitive {
                                index: idx,
                                type_id,
                                points,
                                data,
                            });
                            eprintln!("[ChartApp] Recorded DeletePrimitive at index {}", idx);
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                window.drawing_manager.remove(idx);
                                eprintln!("[ChartApp] Deleted primitive #{}", idx);
                            }
                            self.alert_manager.remove_alerts_for_drawing(prim_id);
                        } else {
                            // No undo snapshot — still get the ID for alert cleanup before removing.
                            let prim_id = self.panel_app.panel_grid
                                .active_window()
                                .and_then(|w| w.drawing_manager.get_data_at(idx))
                                .map(|d| d.id);
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                window.drawing_manager.remove(idx);
                                eprintln!("[ChartApp] Deleted primitive #{} (no undo snapshot)", idx);
                            }
                            if let Some(pid) = prim_id {
                                self.alert_manager.remove_alerts_for_drawing(pid);
                            }
                        }
                    }
                    "clone" => {
                        // clone_primitive returns Option<usize> (new index).
                        let cloned_idx = self.panel_app.panel_grid.active_window_mut()
                            .and_then(|w| w.drawing_manager.clone_primitive(idx));
                        if let Some(new_idx) = cloned_idx {
                            // Apply +20px right, -20px up offset to the cloned primitive.
                            // Convert screen pixels to bar/price coordinates using the viewport.
                            if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                                let bar_spacing = w.viewport.bar_spacing;
                                let chart_height = w.viewport.chart_height;
                                let price_range = w.price_scale.price_max - w.price_scale.price_min;
                                // 20px right = 20 / bar_spacing bars
                                let bar_delta = 20.0 / bar_spacing.max(0.001);
                                // 20px up = 20 / chart_height * price_range (up = positive price)
                                let price_delta = if chart_height > 0.0 && price_range > 0.0 {
                                    20.0 / chart_height * price_range
                                } else {
                                    0.0
                                };
                                w.drawing_manager.translate_at(new_idx, bar_delta, price_delta);
                                w.drawing_manager.select_by_index(new_idx);
                            }
                            // Capture the NEW primitive's data for undo.
                            let snapshot = self.panel_app.panel_grid.active_window()
                                .and_then(|w| {
                                    let type_id = w.drawing_manager.get_type_id_at(new_idx)?;
                                    let points = w.drawing_manager.get_points_at(new_idx)?;
                                    let data = w.drawing_manager.get_data_at(new_idx)?;
                                    Some((type_id, points, data))
                                });
                            if let Some((type_id, points, data)) = snapshot {
                                self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                                    index: new_idx,
                                    type_id,
                                    points,
                                    data,
                                });
                                eprintln!("[ChartApp] Recorded CreatePrimitive (clone) at index {}", new_idx);
                            }
                            eprintln!("[ChartApp] Cloned primitive #{} -> #{}", idx, new_idx);
                        } else {
                            eprintln!("[ChartApp] Clone primitive #{} returned None", idx);
                        }
                    }
                    "toggle_lock" => {
                        // Read current state BEFORE mutating.
                        let previous = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.drawing_manager.get_data_at(idx))
                            .map(|d| d.locked)
                            .unwrap_or(false);
                        self.push_undo_command(zengeld_chart::Command::SetPrimitiveLock {
                            index: idx,
                            locked: !previous,
                            previous,
                        });
                        eprintln!("[ChartApp] Recorded SetPrimitiveLock at index {} locked={}", idx, !previous);
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.toggle_lock_primitive(idx);
                        }
                    }
                    "toggle_visibility" => {
                        // Read current state BEFORE mutating.
                        let previous = self.panel_app.panel_grid.active_window()
                            .and_then(|w| w.drawing_manager.get_data_at(idx))
                            .map(|d| d.visible)
                            .unwrap_or(true);
                        self.push_undo_command(zengeld_chart::Command::SetPrimitiveVisibility {
                            index: idx,
                            visible: !previous,
                            previous,
                        });
                        eprintln!("[ChartApp] Recorded SetPrimitiveVisibility at index {} visible={}", idx, !previous);
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            window.drawing_manager.toggle_visibility(idx);
                        }
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
                            crate::ChartApp::build_demo_symbol_results("", &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
                        self.modal_state.start_editing(0);
                        eprintln!("[ChartApp] symbol search opened from context menu");
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

    // =========================================================================
    // Undo / Redo
    // =========================================================================

    /// Execute an undo step, routing through the group history for shared
    /// operations when the active window is in a sync group.
    ///
    /// Priority: group history first (shared ops), then window-local history.
    fn perform_undo_with_group(&mut self) {
        // Extract group_id before any mutable borrows.
        let group_id = self.panel_app.panel_grid.active_window()
            .and_then(|w| w.group_id);

        if let Some(gid) = group_id {
            // Try group history first (shared: primitives, indicators).
            let group_cmd = self.panel_app.tag_manager
                .group_mut(gid)
                .and_then(|g| g.command_history.undo());
            if let Some(cmd) = group_cmd {
                let desc = cmd.description();
                let inverse = cmd.inverse();
                self.apply_command_to_active_window(&inverse);
                self.post_apply_command_effects(&inverse);
                eprintln!("[ChartApp] Undo (group): {}", desc);
                return;
            }
        }

        // Fall back to window-local history (viewport, symbol, timeframe, chart type).
        let local_cmd = self.panel_app.panel_grid.active_window_mut()
            .and_then(|w| w.command_history.undo());
        if let Some(cmd) = local_cmd {
            let desc = cmd.description();
            let inverse = cmd.inverse();
            self.apply_command_to_active_window(&inverse);
            self.post_apply_command_effects(&inverse);
            eprintln!("[ChartApp] Undo (local): {}", desc);
        } else {
            eprintln!("[ChartApp] Nothing to undo");
        }
    }

    /// Execute a redo step, routing through the group history for shared
    /// operations when the active window is in a sync group.
    fn perform_redo_with_group(&mut self) {
        // Extract group_id before any mutable borrows.
        let group_id = self.panel_app.panel_grid.active_window()
            .and_then(|w| w.group_id);

        if let Some(gid) = group_id {
            // Try group history first (shared: primitives, indicators).
            let group_cmd = self.panel_app.tag_manager
                .group_mut(gid)
                .and_then(|g| g.command_history.redo());
            if let Some(cmd) = group_cmd {
                let desc = cmd.description();
                self.apply_command_to_active_window(&cmd);
                self.post_apply_command_effects(&cmd);
                eprintln!("[ChartApp] Redo (group): {}", desc);
                return;
            }
        }

        // Fall back to window-local history.
        let local_cmd = self.panel_app.panel_grid.active_window_mut()
            .and_then(|w| w.command_history.redo());
        if let Some(cmd) = local_cmd {
            let desc = cmd.description();
            self.apply_command_to_active_window(&cmd);
            self.post_apply_command_effects(&cmd);
            eprintln!("[ChartApp] Redo (local): {}", desc);
        } else {
            eprintln!("[ChartApp] Nothing to redo");
        }
    }

    /// Post-apply hook: run side-effects that the forward path does but
    /// `apply_command_to_active_window` does not (indicator recalc, group
    /// propagation, autosave).
    fn post_apply_command_effects(&mut self, cmd: &zengeld_chart::Command) {
        use zengeld_chart::Command;
        match cmd {
            Command::ChangeSymbol { new_symbol, .. } => {
                // Recalculate indicators for new bars
                let (sym, bars) = self.panel_app.panel_grid.active_window()
                    .map(|w| (w.symbol.clone(), w.bars.clone()))
                    .unwrap_or_default();
                self.indicator_manager.calculate_all_for_symbol(&sym, &bars);
                self.sync_sub_panes_from_manager();
                // Propagate to sync group peers
                if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    self.propagate_symbol_to_sync_group(leaf, new_symbol);
                }
                self.autosave_snapshot();
            }
            Command::ChangeTimeframe { new_timeframe, .. } => {
                // Recalculate indicators for new bars
                let (sym, bars) = self.panel_app.panel_grid.active_window()
                    .map(|w| (w.symbol.clone(), w.bars.clone()))
                    .unwrap_or_default();
                self.indicator_manager.calculate_all_for_symbol(&sym, &bars);
                self.sync_sub_panes_from_manager();
                // Propagate to sync group peers
                if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    self.propagate_timeframe_to_sync_group(leaf, new_timeframe.clone());
                }
                self.autosave_snapshot();
            }
            Command::AddIndicator { .. } | Command::RemoveIndicator { .. } => {
                self.autosave_snapshot();
            }
            Command::CreatePrimitive { .. } | Command::DeletePrimitive { .. }
            | Command::DeleteAllPrimitives { .. } | Command::RestoreAllPrimitives { .. }
            | Command::MovePrimitive { .. } | Command::ModifyPrimitiveData { .. }
            | Command::ModifyPrimitiveFull { .. } => {
                self.autosave_snapshot();
            }
            _ => {}
        }
    }

    /// Execute an undo step: pop from command history and apply the inverse command.
    ///
    /// Used by toolbar "undo" button click (routes through group history).
    fn perform_undo(&mut self) {
        self.perform_undo_with_group();
    }

    /// Execute a redo step: pop from redo stack and re-apply the command.
    ///
    /// Used by toolbar "redo" button click (routes through group history).
    fn perform_redo(&mut self) {
        self.perform_redo_with_group();
    }

    /// Push a command to the appropriate history stack.
    ///
    /// Shared commands (primitives, indicators) go to the sync group's
    /// `command_history` when the active window is in a group. Window-local
    /// commands (viewport, symbol, timeframe, chart type) always go to the
    /// active window's own `command_history`.
    pub(crate) fn push_undo_command(&mut self, cmd: zengeld_chart::Command) {
        use zengeld_chart::Command;
        let is_shared = matches!(
            cmd,
            Command::CreatePrimitive { .. }
                | Command::DeletePrimitive { .. }
                | Command::DeleteAllPrimitives { .. }
                | Command::RestoreAllPrimitives { .. }
                | Command::MovePrimitive { .. }
                | Command::SetPrimitiveVisibility { .. }
                | Command::SetPrimitiveLock { .. }
                | Command::ModifyPrimitiveData { .. }
                | Command::ModifyPrimitiveFull { .. }
                | Command::ReorderPrimitive { .. }
                | Command::AddIndicator { .. }
                | Command::RemoveIndicator { .. }
        );

        let group_id = if is_shared {
            self.panel_app.panel_grid.active_window().and_then(|w| w.group_id)
        } else {
            None
        };

        if let Some(gid) = group_id {
            if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                group.command_history.push(cmd);
                return;
            }
        }

        // Local command or no group — push to the active window's history.
        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
            window.command_history.push(cmd);
        }
    }

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
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        window.timeframe = tf.clone();
                        window.update_title();
                        window.bars.clear();
                    }
                    // Unsubscribe old WS and fetch bars for new timeframe.
                    self.bridge.unsubscribe_all();
                    if !symbol.is_empty() {
                        let eid_str = self.active_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping request_bars (timeframe change)", eid_str);
                        } else {
                            self.bridge.request_bars(self.active_exchange, &symbol, &tf, None, Some(self.panel_app.user_manager.profile.bar_count as usize));
                        }
                    }
                    // Propagate new timeframe to all leaves in the same sync group.
                    if tf_changed {
                        if let Some(leaf) = active_leaf {
                            self.propagate_timeframe_to_sync_group(leaf, tf);
                        }
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
            // Chart settings modal — opened by gear dropdown "Chart Settings..." item.
            ChartOutEvent::OpenChartSettings => {
                self.panel_app.chart_settings_state.toggle();
                eprintln!("[ChartApp] chart_settings modal toggled via settings dropdown: {}", self.panel_app.chart_settings_state.is_open);
            }

            // User settings modal — opened by the chrome gear button.
            ChartOutEvent::OpenUserSettings => {
                self.panel_app.user_settings_state.toggle();
                eprintln!("[ChartApp] user_settings modal toggled: {}", self.panel_app.user_settings_state.is_open);
            }

            // Quick-settings toggles from the settings gear dropdown.
            ChartOutEvent::ToggleGrid => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_grid();
                    eprintln!("[ChartApp] grid toggled");
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleCrosshair => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_crosshair();
                    eprintln!("[ChartApp] crosshair toggled");
                }
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_watermark();
                    eprintln!("[ChartApp] watermark toggled");
                }
                state_mutated = true;
            }

            // Left panel — no sidebar in standalone mode.
            ChartOutEvent::ToggleLeftPanel => {
                eprintln!("[ChartApp] Not available in standalone mode: {:?}", event);
            }

            // Right sidebar panels — handled via SidebarState.
            // toggle_right_panel returns Option<(bool, f64)>:
            //   Some((true,  w)) → sidebar opened  → compensate viewport rightward
            //   Some((false, w)) → sidebar closed  → compensate viewport leftward
            //   None             → panel switched  → no viewport change needed
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
            // Split layout events — wire to ChartPanelGrid split system.
            // After each split all new leaves receive a shared sync color tag so
            // that symbol / timeframe changes propagate across the split group.
            ChartOutEvent::InternalSplitHorizontal => { self.do_split(zengeld_chart::SplitKind::Horizontal); state_mutated = true; }
            ChartOutEvent::InternalSplitVertical => { self.do_split(zengeld_chart::SplitKind::Vertical); state_mutated = true; }
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_grid_vertical();
                    eprintln!("[ChartApp] grid vertical toggled -> {}", window.grid_options.vert_lines.visible);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleGridHorizontal => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_grid_horizontal();
                    eprintln!("[ChartApp] grid horizontal toggled -> {}", window.grid_options.horz_lines.visible);
                }
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_tooltip();
                    eprintln!("[ChartApp] tooltip toggled -> {}", window.tooltip.visible);
                }
                state_mutated = true;
            }
            ChartOutEvent::ToggleTooltipFollow => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.toggle_tooltip_follow();
                    eprintln!("[ChartApp] tooltip follow toggled -> {}", window.tooltip.follow_cursor);
                }
                state_mutated = true;
            }
            ChartOutEvent::SetWatermarkText(text) => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.set_watermark_text(text);
                    eprintln!("[ChartApp] watermark text set to {}", text);
                }
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
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.set_watermark_position(horz, vert);
                    eprintln!("[ChartApp] watermark position set to {}", pos);
                }
                state_mutated = true;
            }
            // === Presets (in-memory HashMap) ===
            ChartOutEvent::SavePreset { name } => {
                // "Save As" — create a new named preset from current state.
                use zengeld_chart::preset::preset::unix_timestamp_parts;
                let (secs, nanos) = unix_timestamp_parts();
                let id = format!("preset_{}_{}", secs, nanos);
                eprintln!("[ChartApp] save-as preset '{}' → id={}", name, id);
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
                // Save the outgoing preset (with bars) before switching away.
                let prev_id = self.panel_app.active_preset_id.clone();
                if prev_id != id && !prev_id.is_empty() && prev_id != "__default__" {
                    self.autosave_snapshot();
                }
                self.panel_app.active_preset_id = id.clone();
                self.persist_profile();
                if let Some(preset) = self.panel_app.presets.get(&id).cloned() {
                    eprintln!(
                        "[ChartApp] applying preset '{}': {} windows, {} groups, {} indicators",
                        preset.name, preset.windows.len(), preset.sync_groups.len(), preset.indicators.len()
                    );

                    // ----------------------------------------------------------------
                    // Step 1: Attempt full layout restore (new presets with layout).
                    // Falls back to in-place window patching for old presets without.
                    // ----------------------------------------------------------------
                    // Capture data_provider from current active window (it's Arc-shared).
                    let data_provider = self.panel_app.panel_grid.active_window()
                        .map(|w| w.data_provider.clone())
                        .unwrap_or_else(|| std::sync::Arc::new(zengeld_chart::NullDataProvider));

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

                                    // Set the shared data_provider so bars can be loaded.
                                    window.data_provider = data_provider.clone();

                                    // Apply all snapshot fields
                                    window.exchange = snap.exchange.clone();
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
                                    window.group_id = snap.group_id.map(|g| zengeld_chart::tag_manager::SyncGroupId(g));

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

                                    // Restore per-symbol drawing cache
                                    window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                    // Load bars. Priority:
                                    //   1. Snapshot bars (instant, no network) — preferred.
                                    //   2. Data-provider synchronous cache — e.g. demo provider.
                                    //   3. Async BarsLoaded — defer viewport restore.
                                    //
                                    // After set_bars() the viewport is reset, so we always
                                    // re-apply the snapshotted viewport/price_scale afterwards.
                                    let deferred_vp = zengeld_chart::state::chart_window::ViewportSnapshot {
                                        view_start: snap.viewport.view_start,
                                        bar_spacing: snap.viewport.bar_spacing,
                                        price_min: snap.price_scale.price_min,
                                        price_max: snap.price_scale.price_max,
                                        scale_mode: snap.price_scale.scale_mode.clone(),
                                    };
                                    eprintln!("[ChartApp] LoadPreset window {}: snap.bars.len()={}", snap.window_id, snap.bars.len());
                                    if !snap.bars.is_empty() {
                                        // Bars from snapshot — instant restore with exact coordinates.
                                        eprintln!("[ChartApp] → restoring {} bars from snapshot for {}", snap.bars.len(), snap.symbol);
                                        window.bars = snap.bars.clone();
                                        window.calc_moving_averages();
                                        window.drawing_manager.recalculate_all_bar_caches(&window.bars);
                                        // Restore full viewport + price_scale (all fields persisted).
                                        window.viewport = snap.viewport.clone();
                                        window.price_scale = snap.price_scale.clone();
                                        // Still kick off a background refresh to pull the latest candles.
                                        let _ = window.data_provider.get_bars(&snap.symbol, &snap.timeframe);
                                    } else if let Some(bars) = window.data_provider.get_bars(&snap.symbol, &snap.timeframe) {
                                        window.set_bars(bars);
                                        window.drawing_manager.recalculate_all_bar_caches(&window.bars);
                                        window.viewport = snap.viewport.clone();
                                        window.viewport.bar_count = window.bars.len();
                                        window.price_scale = snap.price_scale.clone();
                                        window.calc_auto_scale();
                                        // No pending restore needed — bars arrived synchronously.
                                    } else {
                                        // Bars will arrive asynchronously via BarsLoaded.
                                        // Stash the desired viewport so it can be applied then.
                                        window.pending_viewport_restore = Some(deferred_vp);
                                    }

                                    eprintln!(
                                        "[ChartApp] built window {} → {}/{} ({} drawings)",
                                        snap.window_id, snap.symbol, snap.timeframe.name,
                                        snap.drawings.primitives.len()
                                    );

                                    new_windows.insert(chart_id, window);
                                    new_leaf_to_chart.insert(leaf_id, chart_id);
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
                                        eprintln!("[ChartApp] restore_tree failed: {} — falling back to patch", e);
                                        false
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("[ChartApp] layout deserialize failed: {} — falling back to patch", e);
                                false
                            }
                        }
                    } else {
                        eprintln!("[ChartApp] preset has no layout snapshot — falling back to patch");
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

                                // Restore per-symbol drawing cache
                                window.symbol_drawings = snap.symbol_drawings_snapshots.clone();

                                // Restore bars using the same priority as the layout path:
                                //   1. Snapshot bars (instant).
                                //   2. Data-provider synchronous cache.
                                //   3. Async BarsLoaded with deferred viewport.
                                let deferred_vp = zengeld_chart::state::chart_window::ViewportSnapshot {
                                    view_start: snap.viewport.view_start,
                                    bar_spacing: snap.viewport.bar_spacing,
                                    price_min: snap.price_scale.price_min,
                                    price_max: snap.price_scale.price_max,
                                    scale_mode: snap.price_scale.scale_mode.clone(),
                                };
                                if !snap.bars.is_empty() {
                                    window.set_bars(snap.bars.clone());
                                    window.drawing_manager.recalculate_all_bar_caches(&window.bars);
                                    window.viewport = snap.viewport.clone();
                                    window.price_scale = snap.price_scale.clone();
                                    let _ = window.data_provider.get_bars(&snap.symbol, &snap.timeframe);
                                } else if let Some(bars) = window.data_provider.get_bars(&snap.symbol, &snap.timeframe) {
                                    window.set_bars(bars);
                                    window.drawing_manager.recalculate_all_bar_caches(&window.bars);
                                    window.viewport = snap.viewport.clone();
                                } else {
                                    window.pending_viewport_restore = Some(deferred_vp);
                                }

                                eprintln!("[ChartApp] patched window {} → {}/{} ({} drawings)",
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

                    // ----------------------------------------------------------------
                    // Step 6: Restore indicators
                    // ----------------------------------------------------------------
                    self.indicator_manager.clear_all();
                    for ind_snap in &preset.indicators {
                        if self.indicator_manager.create_instance_with_id(
                            ind_snap.id,
                            &ind_snap.type_id,
                            &ind_snap.symbol,
                        ) {
                            if let Some(inst) = self.indicator_manager.get_instance_mut(ind_snap.id) {
                                inst.name = ind_snap.name.clone();
                                inst.pane = ind_snap.pane;
                                inst.order = ind_snap.order;
                                inst.visible = ind_snap.visible;
                                inst.locked = ind_snap.locked;
                                inst.window_id = ind_snap.window_id;
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
                            }
                        }
                    }
                    eprintln!("[ChartApp] restored {} indicators", preset.indicators.len());

                    // ----------------------------------------------------------------
                    // Step 7: Clear stale per-leaf UI state
                    // ----------------------------------------------------------------
                    self.panel_app.indicator_overlay_states.clear();
                    self.panel_app.leaf_color_tags.clear();

                    // Re-populate leaf_color_tags from restored sync groups so the
                    // color tab indicators appear correctly after load.
                    for group in self.panel_app.tag_manager.groups() {
                        let color = group.color;
                        for &chart_id in &group.members {
                            // Find the leaf that owns this chart_id and tag it.
                            let leaf_opt = self.panel_app.panel_grid.iter_windows()
                                .find(|(_, w)| w.id == chart_id)
                                .map(|(lid, _)| lid);
                            if let Some(leaf_id) = leaf_opt {
                                self.panel_app.leaf_color_tags.insert(leaf_id, color);
                            }
                        }
                    }

                    // ----------------------------------------------------------------
                    // Step 8: Restore alerts
                    // ----------------------------------------------------------------
                    self.alert_manager.restore(preset.alerts.clone());
                    eprintln!("[ChartApp] restored {} alerts", self.alert_manager.len());

                    eprintln!("[ChartApp] preset '{}' fully restored", preset.name);
                } else {
                    eprintln!("[ChartApp] preset '{}' not found in memory", id);
                }
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
                // If the closed tab was active, switch to another.
                if self.panel_app.active_preset_id == id {
                    if let Some(next) = self.panel_app.open_tabs.last().cloned() {
                        self.process_chart_out_event(ChartOutEvent::LoadPreset { id: next });
                    } else {
                        // No tabs left — fall back to __default__.
                        self.panel_app.active_preset_id = "__default__".to_string();
                    }
                }
                self.persist_profile();
            }

            ChartOutEvent::OpenTab { id } => {
                eprintln!("[ChartApp] open tab: {}", id);
                if self.panel_app.open_tabs.contains(&id) {
                    // Preset already open — just switch to it.
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
                    // No active named preset — treat as Save As
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

                // Now compute the next "Untitled N" — the auto-saved one is already counted.
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

            ref other => {
                eprintln!("[ChartApp] unhandled event: {:?}", other);
            }
        }

        if state_mutated {
            self.autosave_snapshot();
        }
    }

    // -------------------------------------------------------------------------
    // Autosave
    // -------------------------------------------------------------------------

    /// Immediately save the active preset snapshot if autosave is enabled.
    ///
    /// - Does nothing when autosave is disabled.
    /// - Does nothing for empty preset ids.
    /// - For `__default__` — creates the preset entry if it doesn't exist yet.
    pub fn autosave_snapshot(&mut self) {
        eprintln!("[ChartApp] autosave_snapshot called: enabled={}, id='{}'",
            self.panel_app.autosave_enabled, self.panel_app.active_preset_id);
        if !self.panel_app.autosave_enabled { return; }
        let id = self.panel_app.active_preset_id.clone();
        if id.is_empty() { return; }

        let name = if let Some(preset) = self.panel_app.presets.get(&id) {
            preset.name.clone()
        } else if id == "__default__" {
            "Default".to_string()
        } else {
            eprintln!("[ChartApp] autosave: preset id '{}' not found in presets map", id);
            return;
        };

        self.collect_state_into_preset(&id, &name);
        if let Some(preset) = self.panel_app.presets.get(&id).cloned() {
            self.preset_actions.push(crate::PresetAction::Upsert(preset));
        }
        eprintln!("[ChartApp] autosave snapshot for '{}' (persisted)", name);
    }

    // -------------------------------------------------------------------------
    // New-chart helper
    // -------------------------------------------------------------------------

    /// Execute the "New Chart" action with a user-supplied name.
    ///
    /// 1. Auto-saves the current state if it has no named preset yet (__default__).
    /// 2. Performs a full reset (single layout, no primitives, no indicators).
    /// 3. Creates the new blank preset with the given name and makes it active.
    fn execute_new_chart_with_name(&mut self, new_name: String) {
        use zengeld_chart::preset::preset::unix_timestamp_parts;

        // Auto-save current named preset before reset.
        // (__default__ is already saved in OpenPresetNewChart before the modal opens.)
        if self.panel_app.active_preset_id != "__default__" && !self.panel_app.active_preset_id.is_empty() {
            self.autosave_snapshot();
        }

        // Full reset — single pane, no indicators, no primitives.
        self.panel_app.panel_grid.set_layout_single();
        for w in self.panel_app.panel_grid.windows_mut().values_mut() {
            w.drawing_manager.clear_all_primitives();
        }
        self.panel_app.tag_manager.clear();
        self.indicator_manager.clear_all();
        self.panel_app.indicator_overlay_states.clear();
        self.panel_app.leaf_color_tags.clear();
        self.alert_manager.clear();

        // Create the new preset with the user-supplied name.
        let (secs, nanos) = unix_timestamp_parts();
        let new_id = format!("preset_{}_{}", secs, nanos);
        self.collect_state_into_preset(&new_id, &new_name);
        self.panel_app.active_preset_id = new_id.clone();
        // Add the new preset to open_tabs.
        if !self.panel_app.open_tabs.contains(&new_id) {
            self.panel_app.open_tabs.push(new_id.clone());
        }
        if let Some(preset) = self.panel_app.presets.get(&new_id).cloned() {
            self.preset_actions.push(crate::PresetAction::Upsert(preset));
        }
        self.persist_profile();
        eprintln!("[ChartApp] new chart '{}' created (id={})", new_name, new_id);
    }

    // -------------------------------------------------------------------------
    // Preset state collection
    // -------------------------------------------------------------------------

    /// Collect the current chart state into a preset and store it in the
    /// in-memory presets HashMap under the given `id`.
    ///
    /// If a preset with the same `id` already exists, its `name` and
    /// `created_at` are preserved (only the snapshot data is refreshed).
    /// Otherwise a new preset is created with the given `name`.
    pub fn collect_state_into_preset(&mut self, id: &str, name: &str) {
        use zengeld_chart::preset::preset::ChartPreset;
        use zengeld_chart::preset::snapshots::*;

        // Reuse existing metadata if updating an existing slot.
        let (preset_id, preset_name, created_at) =
            if let Some(existing) = self.panel_app.presets.get(id) {
                (existing.id.clone(), existing.name.clone(), existing.created_at)
            } else {
                (id.to_string(), name.to_string(), zengeld_chart::preset::preset::unix_now_secs())
            };

        let mut preset = ChartPreset::new(preset_name);
        preset.id = preset_id;
        preset.created_at = created_at;

        // Capture layout topology before iterating windows.
        if let Ok(layout_val) = serde_json::to_value(
            uzor::panels::serialize::LayoutSnapshot::from_tree(
                self.panel_app.panel_grid.docking().tree(),
                "chart",
            )
        ) {
            preset.layout = layout_val;
        }

        // Snapshot windows
        for (leaf_id, window) in self.panel_app.panel_grid.iter_windows() {
            preset.windows.push(ChartWindowSnapshot::from_window(window, leaf_id.0));
        }

        // Snapshot sync groups
        for group in self.panel_app.tag_manager.groups() {
            preset.sync_groups.push(SyncGroupSnapshot::from_group(group));
        }

        // Snapshot indicators (sorted by display order so restore preserves sequence).
        let mut sorted_instances: Vec<_> = self.indicator_manager.instances_iter().collect();
        sorted_instances.sort_by_key(|inst| inst.order);
        for inst in sorted_instances {
            let outputs = inst.outputs.iter().map(|(k, v)| {
                (k.clone(), OutputConfigSnapshot {
                    color: v.color.clone(),
                    line_width: v.line_width,
                    visible: Some(v.visible),
                })
            }).collect();
            preset.indicators.push(IndicatorSnapshot {
                id: inst.id,
                type_id: inst.type_id.clone(),
                name: inst.name.clone(),
                params: inst.params.iter().map(|(k, v)| {
                    (k.clone(), serde_json::to_value(v).unwrap_or_default())
                }).collect(),
                outputs,
                pane: inst.pane,
                order: inst.order,
                visible: inst.visible,
                locked: inst.locked,
                symbol: inst.symbol.clone(),
                window_id: inst.window_id,
                origin_id: inst.origin_id,
                signals_enabled: inst.signals_enabled,
                timeframe_visibility: inst.timeframe_visibility.clone(),
            });
        }

        // Snapshot alerts
        preset.alerts = self.alert_manager.snapshot();

        self.panel_app.presets.insert(id.to_string(), preset);
    }

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
    fn apply_slider_value(&mut self, field_id: &str, value: f64) {

        // Appearance / style params (glass opacity, blur radius)
        if field_id.starts_with("appearance:style_") {
            let param_id = &field_id[17..]; // strip "appearance:style_"
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
            self.autosave_snapshot();
            return;
        }

        if let Some(prop_id) = field_id.strip_prefix("style_prop:") {
            use zengeld_chart::drawing::primitives_v2::config::PropertyValue;
            let new_val = PropertyValue::Number(value);
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.drawing_manager.apply_style_property(idx, prop_id, new_val);
            }
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
    fn apply_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
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
    // Indicator tf dual-slider helper
    // =========================================================================

    /// Apply a dual-handle (min/max range) slider value for `tf_*_slider` fields in
    /// the **indicator** settings modal. Writes directly to the `IndicatorInstance`'s
    /// `timeframe_visibility` field.
    fn apply_ind_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
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
    fn apply_ind_tf_min_value(&mut self, tf_idx: usize, val: u32) {
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
    fn apply_ind_tf_max_value(&mut self, tf_idx: usize, val: u32) {
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
    fn apply_cmp_dual_slider_value(&mut self, field_id: &str, value: u32, handle: DualSliderHandle) {
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

    // =========================================================================
    // Color picker drag helper
    // =========================================================================

    // =========================================================================
    // Sync group helpers
    // =========================================================================

    /// Desync a leaf from its color tag sync group.
    ///
    /// Removes the color tag, purges cloned drawing primitives, and purges
    /// cloned indicator instances that were added when the split occurred.
    fn perform_desync(&mut self, leaf_id: zengeld_chart::LeafId) {
        // 1. Remove color tag.
        self.panel_app.leaf_color_tags.remove(&leaf_id);

        // Snapshot group info BEFORE disconnect.
        let had_group_id = self.panel_app.panel_grid
            .window_for_leaf(leaf_id)
            .and_then(|w| w.group_id)
            .is_some();
        // Pre-tag indicator ids — these survive desync (will be unhidden).
        let pre_tag_ids: Vec<u64> = self.panel_app.panel_grid
            .window_for_leaf(leaf_id)
            .map(|w| w.pre_tag_indicator_ids.clone())
            .unwrap_or_default();
        // Has stashed primitives? (window joined an existing tag)
        let has_stash = self.panel_app.panel_grid
            .window_for_leaf(leaf_id)
            .map_or(false, |w| !w.stashed_primitives.is_empty());

        // 1b. Disconnect from TagManager group.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
            if let Some(old_group_id) = self.panel_app.tag_manager.disconnect(chart_id) {
                eprintln!(
                    "[TagManager] Disconnected chart {:?} from group {:?}",
                    chart_id, old_group_id
                );
                let remaining = self.panel_app.tag_manager
                    .members(old_group_id)
                    .map_or(0, |m| m.len());
                eprintln!("[TagManager] Group {:?} has {} remaining members", old_group_id, remaining);
            }
        }

        // 2. Restore window's own state: primitives and indicators.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                if had_group_id {
                    // Clear tag primitives (pre-render sync fills these from group).
                    window.drawing_manager.clear_all_primitives();

                    // Restore stashed primitives (window's own, hidden on join).
                    if has_stash {
                        let restored: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                            std::mem::take(&mut window.stashed_primitives);
                        let count = restored.len();
                        window.drawing_manager.add_synced_primitives(restored);
                        eprintln!("[TagManager] Desync: restored {} stashed primitives", count);
                    } else {
                        eprintln!("[TagManager] Desync: no stashed primitives (split child or seed source)");
                    }
                } else {
                    window.drawing_manager.purge_synced_primitives();
                }
                window.group_id = None;
                window.pre_tag_indicator_ids.clear();
                window.stashed_primitives.clear();
                // Restore the window-local command history stashed at join time.
                if let Some(stashed) = window.stashed_command_history.take() {
                    window.command_history = stashed;
                    eprintln!("[TagManager] Desync: restored stashed command history");
                }
            }

            // Remove tag indicators: all indicators for this window EXCEPT pre-tag ones.
            // Then unhide pre-tag indicators.
            if had_group_id {
                let to_remove: Vec<u64> = self.indicator_manager
                    .instances_iter()
                    .filter(|i| {
                        i.window_id == Some(chart_id.0)
                            && !pre_tag_ids.contains(&i.id)
                    })
                    .map(|i| i.id)
                    .collect();
                let count = to_remove.len();
                for id in &to_remove {
                    self.indicator_manager.remove_instance(*id);
                }
                // Unhide pre-tag indicators.
                for &id in &pre_tag_ids {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(id) {
                        inst.visible = true;
                    }
                }
                eprintln!("[TagManager] Desync: removed {} tag indicators, unhid {} pre-tag",
                    count, pre_tag_ids.len());
            } else {
                self.indicator_manager.purge_synced_instances_for_window(chart_id.0);
            }
        }

        // 4. Recalculate sub-panes so indicator panels stay in sync.
        self.sync_sub_panes_from_manager();

        eprintln!("[ChartApp] Desynced leaf {:?} from color tag group", leaf_id);
    }

    /// Join an existing color group: clone primitives and indicators from a peer
    /// that already belongs to this color group into the joining leaf.
    fn sync_join_color_group(&mut self, joining_leaf: zengeld_chart::LeafId, color: [f32; 4]) {
        // Connect the joining leaf's chart to the TagManager group for this color.
        // Find-or-create the group so TagManager stays consistent regardless of
        // whether this is the first leaf in the group or a later joiner.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(joining_leaf) {
            let existing_group = self.panel_app.tag_manager.find_group_by_color(color);
            let is_new_group = existing_group.is_none();
            let group_id = existing_group.unwrap_or_else(|| {
                let (symbol, tf) = self.panel_app.panel_grid
                    .window_for_leaf(joining_leaf)
                    .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                    .unwrap_or_else(|| (
                        "BTCUSDT".to_string(),
                        zengeld_chart::state::Timeframe::h1(),
                    ));
                self.panel_app.tag_manager.create_group(color, symbol, tf)
            });
            let _ = self.panel_app.tag_manager.connect(chart_id, group_id);
            eprintln!(
                "[TagManager] Connected chart {:?} to group {:?} (new={})",
                chart_id, group_id, is_new_group
            );

            // Snapshot pre-tag indicator ids (these survive desync).
            let pre_tag_ids: Vec<u64> = self.indicator_manager
                .instances_iter()
                .filter(|i| i.window_id == Some(chart_id.0) && i.origin_id.is_none())
                .map(|i| i.id)
                .collect();

            if is_new_group {
                // NEW GROUP: seed tag with window's current state.
                // Stash window's own primitives, copy them to group seed.
                // Pre-render sync will fill drawing_manager from group each frame.
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(joining_leaf) {
                    window.group_id = Some(group_id);
                    window.pre_tag_indicator_ids = pre_tag_ids;
                    // Stash window's own primitives (for restore on desync).
                    let own_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                        window.drawing_manager.primitives().iter()
                            .filter(|p| p.data().origin_id.is_none())
                            .map(|p| p.clone_box())
                            .collect();
                    window.stashed_primitives = own_prims;
                    // Stash window's command history — shared ops will go to group history.
                    window.stashed_command_history = Some(std::mem::replace(
                        &mut window.command_history,
                        zengeld_chart::CommandHistory::new(250),
                    ));
                }

                // Seed primitives into group (clone from stash).
                let prim_symbol = self.panel_app.panel_grid.window_for_leaf(joining_leaf)
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let mut source_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                    self.panel_app.panel_grid.window_for_leaf(joining_leaf)
                        .map(|w| {
                            w.stashed_primitives.iter()
                                .map(|p| p.clone_box())
                                .collect()
                        })
                        .unwrap_or_default();
                for p in &mut source_prims {
                    p.data_mut().symbol = prim_symbol.clone();
                }
                if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                    group.primitives = source_prims;
                }

                // Seed indicator configs.
                let seed_symbol = self.panel_app.panel_grid.window_for_leaf(joining_leaf)
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let mut configs: Vec<zengeld_chart::tag_manager::IndicatorGroupConfig> = self
                    .indicator_manager
                    .instances_iter()
                    .filter(|i| i.origin_id.is_none() && i.window_id == Some(chart_id.0))
                    .map(|i| zengeld_chart::tag_manager::IndicatorGroupConfig {
                        id: i.id,
                        type_id: i.type_id.clone(),
                        name: i.name.clone(),
                        params: std::collections::HashMap::new(),
                        pane: i.pane as u32,
                        visible: i.visible,
                        symbol: seed_symbol.clone(),
                    })
                    .collect();
                // Sort by id to preserve original creation order (HashMap iteration is random).
                configs.sort_by_key(|c| c.id);
                if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                    eprintln!(
                        "[TagManager] Seeded new tag {:?} with {} primitives, {} indicators",
                        group_id, group.primitives.len(), configs.len()
                    );
                    group.indicator_configs = configs;
                }
                return;
            }

            // EXISTING GROUP: stash window's own primitives and hide its indicators.
            // The window switches to showing tag content only.

            // Stash primitives: move window's own primitives into stashed_primitives.
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(joining_leaf) {
                let own_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                    window.drawing_manager.primitives().iter()
                        .filter(|p| p.data().origin_id.is_none())
                        .map(|p| p.clone_box())
                        .collect();
                let stash_count = own_prims.len();
                window.stashed_primitives = own_prims;
                window.drawing_manager.clear_all_primitives();
                window.group_id = Some(group_id);
                window.pre_tag_indicator_ids = pre_tag_ids.clone();
                // Stash window's command history — shared ops will go to group history.
                window.stashed_command_history = Some(std::mem::replace(
                    &mut window.command_history,
                    zengeld_chart::CommandHistory::new(250),
                ));
                eprintln!("[TagManager] Stashed {} window primitives before joining existing group", stash_count);
            }

            // Hide pre-tag indicators by setting visible=false.
            for &id in &pre_tag_ids {
                if let Some(inst) = self.indicator_manager.get_instance_mut(id) {
                    inst.visible = false;
                }
            }
            eprintln!("[TagManager] Hid {} pre-tag indicators", pre_tag_ids.len());

            // Sync tag indicators to joining window.
            let leaf_chart_ids = vec![(joining_leaf, chart_id)];
            self.sync_group_indicators_to_new_members(group_id, &leaf_chart_ids);
            self.sync_sub_panes_from_manager();
            eprintln!("[TagManager] Join existing group: stashed own state, synced tag content");
            return;
        }

        // Dead fallback removed — TagManager handles all sync join logic above.
    }

    /// Split the active leaf: snapshot its color tag before split (the old
    /// LeafId is destroyed by `split_leaf`), then assign inherited or new color
    /// to all resulting leaves and propagate crosshair/viewport.
    fn do_split(&mut self, kind: zengeld_chart::SplitKind) {
        // Snapshot color, group_id, and chart_id BEFORE split — active_leaf will be destroyed.
        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
        let pre_split_color = active_leaf
            .and_then(|lid| self.panel_app.leaf_color_tags.remove(&lid));
        let pre_split_group_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.window_for_leaf(lid))
            .and_then(|w| w.group_id);
        let pre_split_chart_id = active_leaf
            .and_then(|lid| self.panel_app.panel_grid.chart_id_for_leaf(lid));

        let new_leaves = self.panel_app.panel_grid.split_active(kind);
        self.propagate_crosshair_after_split(&new_leaves);

        // TagManager: if the source was already in a group, connect new leaves
        // to THAT group. Otherwise get the next unused color from TagManager's
        // preset palette (NOT the legacy SYNC_COLORS).
        if !new_leaves.is_empty() {
            let group_id = if let Some(existing_group) = pre_split_group_id {
                // Source leaf was already in a group — reuse it.
                if let Some(old_cid) = pre_split_chart_id {
                    let _ = self.panel_app.tag_manager.disconnect(old_cid);
                    eprintln!("[TagManager] Disconnected destroyed chart {:?} from group {:?}", old_cid, existing_group);
                }
                existing_group
            } else {
                // No existing group — pick color from TagManager's palette.
                let color = pre_split_color
                    .unwrap_or_else(|| self.panel_app.tag_manager.next_unused_color());
                let (symbol, timeframe) = self.panel_app.panel_grid
                    .window_for_leaf(new_leaves[0])
                    .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                    .unwrap_or_else(|| (
                        "BTCUSDT".to_string(),
                        zengeld_chart::state::Timeframe::h1(),
                    ));
                self.panel_app.tag_manager.create_group(color, symbol, timeframe)
            };

            // Assign the group's color to leaf_color_tags for UI display.
            let group_color = self.panel_app.tag_manager.group(group_id)
                .map(|g| g.color)
                .unwrap_or([0.5, 0.5, 0.5, 0.9]);
            for &leaf_id in &new_leaves {
                self.panel_app.leaf_color_tags.insert(leaf_id, group_color);
            }
            // Determine if this is a truly new group (just created, has no state yet).
            let is_new_group = self.panel_app.tag_manager.group(group_id)
                .map_or(true, |g| g.members.is_empty() && g.indicator_configs.is_empty() && g.primitives.is_empty());
            // Collect (leaf_id, chart_id) first to avoid mixed borrow when setting group_id.
            let leaf_chart_ids: Vec<(zengeld_chart::LeafId, zengeld_chart::ChartId)> = new_leaves
                .iter()
                .filter_map(|&leaf_id| {
                    self.panel_app.panel_grid.chart_id_for_leaf(leaf_id)
                        .map(|cid| (leaf_id, cid))
                })
                .collect();
            for &(_, chart_id) in &leaf_chart_ids {
                let _ = self.panel_app.tag_manager.connect(chart_id, group_id);
            }
            for &(leaf_id, _) in &leaf_chart_ids {
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                    window.group_id = Some(group_id);
                }
            }

            // For new groups: snapshot source window's current indicator and primitive ids as pre-tag.
            // On desync, only items NOT in these lists will be removed.
            if is_new_group {
                if let Some(&(source_leaf, source_cid)) = leaf_chart_ids.first() {
                    let pre_tag_ids: Vec<u64> = self.indicator_manager
                        .instances_iter()
                        .filter(|i| i.window_id == Some(source_cid.0) && i.origin_id.is_none())
                        .map(|i| i.id)
                        .collect();
                    if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(source_leaf) {
                        window.pre_tag_indicator_ids = pre_tag_ids;
                        // Stash source window's primitives for restore on desync.
                        let own_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                            window.drawing_manager.primitives().iter()
                                .filter(|p| p.data().origin_id.is_none())
                                .map(|p| p.clone_box())
                                .collect();
                        window.stashed_primitives = own_prims;
                        // Stash source window's command history for restore on desync.
                        window.stashed_command_history = Some(std::mem::replace(
                            &mut window.command_history,
                            zengeld_chart::CommandHistory::new(250),
                        ));
                    }
                }
            }
            eprintln!(
                "[TagManager] {} {:?} with {} members after split",
                if is_new_group { "Created group" } else { "Joined existing group" },
                group_id,
                new_leaves.len()
            );

            // Clear legacy-cloned primitives from non-source windows.
            // clone_for_split copies primitives via clone_primitives_for_sync,
            // but for grouped windows the pre-render sync will fill them from
            // the group each frame.  The source (first) leaf keeps its originals
            // since they'll be seeded into the group below.
            for &(leaf_id, _) in leaf_chart_ids.iter().skip(1) {
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                    window.drawing_manager.clear_all_primitives();
                }
            }

            // For EXISTING group: primitives are synced per-frame via pre-render,
            // but indicators need to be created on new windows NOW.
            if !is_new_group {
                self.sync_group_indicators_to_new_members(group_id, &leaf_chart_ids);
            }

            if is_new_group {
                // Seed the tag with the source window's current state.
                // Everything in the window at tag creation time becomes tag state.

                // Primitives: clone source originals into the group.
                let prim_sym = self.panel_app.panel_grid
                    .window_for_leaf(new_leaves[0])
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let mut source_prims: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                    self.panel_app.panel_grid
                        .window_for_leaf(new_leaves[0])
                        .map(|w| {
                            w.drawing_manager.primitives().iter()
                                .filter(|p| p.data().origin_id.is_none())
                                .map(|p| p.clone_box())
                                .collect()
                        })
                        .unwrap_or_default();
                for p in &mut source_prims {
                    p.data_mut().symbol = prim_sym.clone();
                }
                if !source_prims.is_empty() {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                        group.primitives = source_prims;
                        eprintln!(
                            "[TagManager] Seeded group {:?} with {} primitives",
                            group_id, group.primitives.len()
                        );
                    }
                }

                // Indicators: snapshot source window's indicators into group configs.
                if let Some(source_chart_id) = leaf_chart_ids.first().map(|(_, cid)| *cid) {
                    let source_symbol = leaf_chart_ids.first()
                        .and_then(|(lid, _)| self.panel_app.panel_grid.window_for_leaf(*lid))
                        .map(|w| w.symbol.clone())
                        .unwrap_or_default();
                    let mut configs: Vec<zengeld_chart::tag_manager::IndicatorGroupConfig> = self
                        .indicator_manager
                        .instances_iter()
                        .filter(|i| {
                            i.origin_id.is_none() && i.window_id == Some(source_chart_id.0)
                        })
                        .map(|i| zengeld_chart::tag_manager::IndicatorGroupConfig {
                            id: i.id,
                            type_id: i.type_id.clone(),
                            name: i.name.clone(),
                            params: std::collections::HashMap::new(),
                            pane: i.pane as u32,
                            visible: i.visible,
                            symbol: source_symbol.clone(),
                        })
                        .collect();
                    // Sort by id to preserve original creation order (HashMap iteration is random).
                    configs.sort_by_key(|c| c.id);
                    if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
                        let count = configs.len();
                        group.indicator_configs = configs;
                        eprintln!(
                            "[TagManager] Seeded group {:?} with {} indicator configs",
                            group_id, count
                        );
                    }
                }
            }

            // For non-source leaves (split children): create indicator instances
            // from the tag. These are empty windows that only see tag state.
            {
                let non_source: Vec<(zengeld_chart::LeafId, zengeld_chart::ChartId)> =
                    leaf_chart_ids.iter().skip(1).copied().collect();
                if !non_source.is_empty() {
                    self.sync_group_indicators_to_new_members(group_id, &non_source);
                }
            }

            // Reconcile sub_panes so split children don't show source's indicator panels.
            self.sync_sub_panes_from_manager();
        }
    }

    /// After a split, propagate the crosshair and viewport from the first leaf
    /// (which inherits the original active window) to all other new leaves in the
    /// sync group so the synced crosshair and viewport appear immediately.
    fn propagate_crosshair_after_split(&mut self, new_leaves: &[zengeld_chart::LeafId]) {
        if new_leaves.len() < 2 {
            return;
        }
        let source_leaf = new_leaves[0];
        // Read crosshair + viewport state from source window.
        let (bar_f64, price, visible, pane_index, view_start, bar_spacing) =
            match self.panel_app.panel_grid.window_for_leaf(source_leaf) {
                Some(w) => (
                    w.crosshair.bar_f64,
                    w.crosshair.price,
                    w.crosshair.visible,
                    w.crosshair.pane_index,
                    w.viewport.view_start,
                    w.viewport.bar_spacing,
                ),
                None => return,
            };
        // Propagate crosshair to all sync peers.
        self.propagate_crosshair_to_sync_group(source_leaf, bar_f64, price, visible, pane_index);
        // Propagate viewport to all sync peers.
        self.propagate_viewport_to_sync_group(source_leaf, view_start, bar_spacing);
    }

    /// Propagate a symbol change to all leaves in the same sync color group.
    ///
    /// `source_leaf` is the leaf that already had its symbol changed.
    /// All other leaves sharing the same color tag get the same symbol applied.
    fn propagate_symbol_to_sync_group(&mut self, source_leaf: zengeld_chart::LeafId, symbol: &str) {
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };
        let sync_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();
        let symbol_owned = symbol.to_string();
        for leaf_id in sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                let _ = window.change_symbol(&symbol_owned);
            }
        }

        // Also update the TagManager group's canonical symbol so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            self.panel_app.tag_manager.set_symbol(chart_id, symbol.to_string());
            eprintln!(
                "[TagManager] Updated group symbol to '{}' via chart {:?}",
                symbol, chart_id
            );
        }
    }

    /// Propagate a timeframe change to all leaves in the same sync color group.
    ///
    /// `source_leaf` is the leaf that already had its timeframe changed.
    fn propagate_timeframe_to_sync_group(
        &mut self,
        source_leaf: zengeld_chart::LeafId,
        tf: zengeld_chart::state::Timeframe,
    ) {
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };
        let sync_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();
        for leaf_id in sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                let _ = window.change_timeframe(tf.clone());
            }
        }

        // Also update the TagManager group's canonical timeframe so the group
        // state stays consistent with the displayed windows.
        if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            self.panel_app.tag_manager.set_timeframe(chart_id, tf.clone());
            eprintln!(
                "[TagManager] Updated group timeframe to {:?} via chart {:?}",
                tf, chart_id
            );
        }
    }

    /// DEPRECATED: Legacy clone-based primitive propagation for non-grouped windows.
    /// For grouped windows (TagManager), primitives are shared via pre-render sync
    /// from the group's primitive list. This function is skipped when `group_id.is_some()`.
    /// Will be removed once all windows are guaranteed to use TagManager groups.
    fn propagate_new_primitive_to_sync_group(
        &mut self,
        source_leaf: zengeld_chart::LeafId,
    ) {
        // Determine the source window's color tag.
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Collect peer leaf IDs that share the same color tag.
        let peer_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        if peer_leaves.is_empty() {
            return;
        }

        // Get the chart ID for the source leaf, then read the new primitive's ID.
        let source_chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return,
        };

        // If the source window is in a TagManager group, primitives are shared via the
        // group — no clone-based propagation is needed.
        if self.panel_app.panel_grid
            .windows()
            .get(&source_chart_id)
            .map(|w| w.group_id.is_some())
            .unwrap_or(false)
        {
            return;
        }

        let prim_id = match self.panel_app.panel_grid
            .windows()
            .get(&source_chart_id)
            .map(|w| w.drawing_manager.last_original_id())
            .flatten()
        {
            Some(id) => id,
            None => return,
        };

        // For each peer leaf, clone the primitive into its drawing manager.
        for peer_leaf in peer_leaves {
            let peer_chart_id = match self.panel_app.panel_grid.chart_id_for_leaf(peer_leaf) {
                Some(id) => id,
                None => continue,
            };

            // Clone the primitive from the source window.
            let cloned = match self.panel_app.panel_grid
                .windows()
                .get(&source_chart_id)
                .and_then(|w| w.drawing_manager.clone_primitive_for_sync(prim_id, peer_chart_id.0))
            {
                Some(c) => c,
                None => continue,
            };

            // Insert the clone into the peer window.
            if let Some(peer_window) = self.panel_app.panel_grid.windows_mut().get_mut(&peer_chart_id) {
                peer_window.drawing_manager.add_synced_primitives(vec![cloned]);
                eprintln!("[ChartApp] Synced primitive {} to peer chart {:?}", prim_id, peer_chart_id);
            }
        }
    }

    /// Propagate crosshair position to all leaves in the same sync color group.
    ///
    /// `source_leaf` is the leaf whose crosshair was just updated.
    /// All other leaves sharing the same color tag receive a matching bar
    /// position via [`ChartWindow::set_crosshair_from_bar`].
    fn propagate_crosshair_to_sync_group(
        &mut self,
        source_leaf: zengeld_chart::LeafId,
        bar_f64: f64,
        price: f64,
        visible: bool,
        pane_index: Option<usize>,
    ) {
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };
        let sync_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();
        for leaf_id in sync_leaves {
            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                window.set_crosshair_from_bar(bar_f64, price, visible, pane_index);
            }
        }
    }

    /// DEPRECATED: Legacy drawing state propagation for non-grouped windows.
    /// For grouped windows, drawing state sync should go through TagManager.
    /// Will be removed once all windows use TagManager groups.
    fn propagate_drawing_state_to_sync_group(
        &mut self,
        source_leaf: zengeld_chart::LeafId,
    ) {
        // Determine the source window's color tag.
        let source_color = match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
            Some(c) => c,
            None => return,
        };

        // Collect peer leaf IDs that share the same color tag.
        let peer_leaves: Vec<zengeld_chart::LeafId> = self.panel_app.leaf_color_tags.iter()
            .filter(|(&lid, &c)| lid != source_leaf && sync_colors_match(c, source_color))
            .map(|(&lid, _)| lid)
            .collect();

        if peer_leaves.is_empty() {
            return;
        }

        // Read the current drawing state from the source window.
        // We extract tool_id and points so we can release the borrow before
        // mutating peer windows.
        let (tool_id, points) = match self.panel_app.panel_grid.window_for_leaf(source_leaf) {
            Some(w) => match w.drawing_manager.drawing_state() {
                zengeld_chart::drawing::DrawingState::Creating { tool_id, points } => {
                    (Some(tool_id.clone()), points.clone())
                }
                zengeld_chart::drawing::DrawingState::Idle => (None, Vec::new()),
            },
            None => return,
        };

        // Apply to every peer leaf.
        for peer_leaf in peer_leaves {
            if let Some(peer_window) = self.panel_app.panel_grid.window_for_leaf_mut(peer_leaf) {
                peer_window.drawing_manager.set_synced_drawing_state(tool_id.clone(), points.clone());
            }
        }
    }

    /// Apply the current mouse position to the active color picker L2 drag.
    ///
    /// Called from `on_drag_start` (initial value) and `on_drag_move`.
    fn apply_color_picker_drag(&mut self, x: f64, y: f64) {
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

    /// If the active window belongs to a TagManager group, move the most recently
    /// completed primitive out of its `drawing_manager` and into the group's
    /// authoritative primitive list.
    ///
    /// This is called immediately after any drawing-completion event (DrawingClick,
    /// freehand drag-end, finish_multipoint) on a grouped window.  The per-frame
    /// render-cache sync then distributes the primitive to all member windows.
    ///
    /// Returns `true` when a primitive was transferred to the group.
    fn intercept_completed_primitive_to_group(&mut self) -> bool {
        // Check if the active window is in a TagManager group.
        let (chart_id, group_id) = match self.panel_app.panel_grid.active_window() {
            Some(w) if w.group_id.is_some() => {
                let cid = match self.panel_app.panel_grid.active_chart_id() {
                    Some(id) => id,
                    None => return false,
                };
                (cid, w.group_id.unwrap())
            }
            _ => return false,
        };

        // Pop the last primitive from the window's drawing_manager.
        let prim = {
            let window = match self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                Some(w) => w,
                None => return false,
            };
            let idx = match window.drawing_manager.last_index() {
                Some(i) => i,
                None => return false,
            };
            match window.drawing_manager.remove(idx) {
                Some(p) => p,
                None => return false,
            }
        };

        // Stamp the symbol on the primitive so per-symbol tracking works.
        let mut prim = prim;
        let prim_symbol = self.panel_app.panel_grid.windows().get(&chart_id)
            .map(|w| w.symbol.clone())
            .unwrap_or_default();
        prim.data_mut().symbol = prim_symbol;

        // Add the primitive to the TagManager group so all members share it.
        if let Some(group) = self.panel_app.tag_manager.group_mut(group_id) {
            group.primitives.push(prim);
            eprintln!(
                "[TagManager] Moved completed primitive into group {:?} (now {} primitives)",
                group_id,
                group.primitives.len()
            );
            true
        } else {
            // Group disappeared — put the primitive back into drawing_manager.
            if let Some(window) = self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                window.drawing_manager.add_synced_primitives(vec![prim]);
            }
            false
        }
    }

    /// After split joins an existing group, create indicator instances on the
    /// new windows for each indicator config already in the group.
    fn sync_group_indicators_to_new_members(
        &mut self,
        group_id: zengeld_chart::tag_manager::SyncGroupId,
        new_leaf_chart_ids: &[(zengeld_chart::LeafId, zengeld_chart::ChartId)],
    ) {
        // Collect indicator configs from the group.
        let configs: Vec<(String, String)> = self.panel_app.tag_manager
            .group(group_id)
            .map(|g| {
                g.indicator_configs.iter()
                    .map(|c| (c.type_id.clone(), c.name.clone()))
                    .collect()
            })
            .unwrap_or_default();

        if configs.is_empty() {
            return;
        }

        for &(leaf_id, chart_id) in new_leaf_chart_ids {
            let symbol = self.panel_app.panel_grid
                .window_for_leaf(leaf_id)
                .map(|w| w.symbol.clone())
                .unwrap_or_default();

            for (type_id, name) in &configs {
                // Check if this window already has this indicator type.
                let already_has = self.indicator_manager.instances_iter()
                    .any(|i| i.window_id == Some(chart_id.0) && i.type_id == *type_id);
                if already_has {
                    continue;
                }

                if let Some(new_id) = self.indicator_manager.create_instance(type_id, &symbol) {
                    if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                        inst.window_id = Some(chart_id.0);
                    }
                    eprintln!(
                        "[TagManager] Split sync: created indicator '{}' (id={}) for chart {:?}",
                        name, new_id, chart_id
                    );
                }
            }

            // Calculate indicators with this window's bars.
            let bars: Option<Vec<zengeld_chart::Bar>> = self.panel_app.panel_grid
                .window_for_leaf(leaf_id)
                .map(|w| w.bars.clone());
            if let Some(bars) = bars {
                self.indicator_manager.calculate_all_for_symbol(&symbol, &bars);
            }
        }
        self.sync_sub_panes_from_manager();
        eprintln!(
            "[TagManager] Synced {} indicator configs to {} new members",
            configs.len(), new_leaf_chart_ids.len()
        );
    }

    /// When an indicator is created on a grouped window, create matching instances
    /// on all peer windows in the same group.
    fn sync_group_indicator_to_peers(
        &mut self,
        group_id: zengeld_chart::tag_manager::SyncGroupId,
        type_id: &str,
        source_leaf: zengeld_chart::LeafId,
    ) {
        // Collect peer chart_ids and their symbols (excluding source).
        let source_chart_id = self.panel_app.panel_grid.chart_id_for_leaf(source_leaf);
        let peer_info: Vec<(zengeld_chart::ChartId, String)> = self.panel_app.tag_manager
            .members(group_id)
            .map(|members| {
                members.iter()
                    .filter(|&&cid| Some(cid) != source_chart_id)
                    .filter_map(|&cid| {
                        self.panel_app.panel_grid.windows().get(&cid)
                            .map(|w| (cid, w.symbol.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (peer_cid, peer_symbol) in peer_info {
            if let Some(new_id) = self.indicator_manager.create_instance(type_id, &peer_symbol) {
                if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                    inst.window_id = Some(peer_cid.0);
                    inst.origin_id = None; // It's a group-managed indicator, not a clone.
                }
                // Calculate with peer's bars.
                let bars: Option<Vec<zengeld_chart::Bar>> = self.panel_app.panel_grid
                    .windows().get(&peer_cid)
                    .map(|w| w.bars.clone());
                if let Some(bars) = bars {
                    self.indicator_manager.calculate_all_for_symbol(&peer_symbol, &bars);
                }
                eprintln!(
                    "[TagManager] Synced indicator '{}' (id={}) to peer chart {:?}",
                    type_id, new_id, peer_cid
                );
            }
        }
        self.sync_sub_panes_from_manager();
    }

    /// Apply a new minimum value for the timeframe range at `tf_idx` on primitive `prim_idx`.
    fn apply_tf_min_value(&mut self, prim_idx: usize, tf_idx: usize, val: u32) {
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
    fn apply_tf_max_value(&mut self, prim_idx: usize, tf_idx: usize, val: u32) {
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

// =============================================================================
// Sync group color palette
// =============================================================================

/// Compare two RGBA colors for sync group membership.
///
/// Two leaves belong to the same sync group when their RGB components match
/// within a tolerance of 0.01.  The alpha channel is ignored so that picking
/// a color with a different opacity still links the leaves.
pub(crate) fn sync_colors_match(a: [f32; 4], b: [f32; 4]) -> bool {
    (a[0] - b[0]).abs() < 0.01
        && (a[1] - b[1]).abs() < 0.01
        && (a[2] - b[2]).abs() < 0.01
}

/// Convert a CSS hex color string (`#RRGGBB` or `#RRGGBBAA`) to an RGBA `[f32; 4]` array.
///
/// All channels are in the 0.0–1.0 range.  Returns a default opaque black on
/// parse failure.
fn hex_str_to_rgba(hex: &str) -> [f32; 4] {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0) as f32 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0) as f32 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0) as f32 / 255.0;
    let a = if h.len() >= 8 {
        u8::from_str_radix(&h[6..8], 16).unwrap_or(255) as f32 / 255.0
    } else {
        1.0
    };
    [r, g, b, a]
}

/// Build context menu items for empty chart background (right-click on chart).
///
/// This is a standalone version of `MenuCatalog::chart_context_menu()` from the
/// core crate, avoiding a dependency on core from chart-app.
fn build_chart_background_menu() -> Vec<ContextMenuItemState> {
    vec![
        ContextMenuItemState::action_with_icon("settings", "chart_settings", "Настройки"),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("zoom_reset", "reset_zoom", "Сбросить масштаб"),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("camera", "screenshot", "Скриншот"),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("search", "symbol_search", "Найти символ"),
    ]
}
