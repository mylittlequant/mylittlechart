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

mod sliders;
mod color_picker;
mod overlay;
mod chart_out_events;
mod panel_click;
mod sync_group;

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
    /// Move cursor / scroll up (used by PTY to send \x1b[A)
    ArrowUp,
    /// Move cursor / scroll down (used by PTY to send \x1b[B)
    ArrowDown,
    /// Enter / Return key
    Enter,
    /// Escape key (\x1b) — PTY-only for now
    Escape,
    /// Tab key (\x09)
    Tab,
    /// Backspace key (\x7f for PTY, handled differently for text fields)
    Backspace,
    /// Ctrl+C interrupt — sends \x03 to PTY, copies text for text fields
    CtrlC,
    /// Page Up (\x1b[5~)
    PageUp,
    /// Page Down (\x1b[6~)
    PageDown,
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
use uzor::input::TextAction;
use uzor::WidgetId;

/// Convert chart-app's `KeyPress` to the uzor `KeyPress` understood by `TextFieldStore`.
///
/// Only the variants that `TextFieldStore` acts on are mapped; everything
/// else becomes `None` and the caller skips forwarding to the store.
fn to_uzor_key(key: &KeyPress) -> Option<uzor::input::KeyPress> {
    use uzor::input::KeyPress as UK;
    Some(match key {
        KeyPress::Delete      => UK::Delete,
        KeyPress::ArrowLeft   => UK::ArrowLeft,
        KeyPress::ArrowRight  => UK::ArrowRight,
        KeyPress::ArrowUp     => UK::ArrowUp,
        KeyPress::ArrowDown   => UK::ArrowDown,
        KeyPress::Enter       => UK::Enter,
        KeyPress::Escape      => UK::Escape,
        KeyPress::Tab         => UK::Tab,
        KeyPress::Backspace   => UK::Backspace,
        KeyPress::CtrlC       => UK::CtrlC,
        KeyPress::PageUp      => UK::PageUp,
        KeyPress::PageDown    => UK::PageDown,
        KeyPress::Home        => UK::Home,
        KeyPress::End         => UK::End,
        KeyPress::SelectAll   => UK::SelectAll,
        KeyPress::ShiftLeft   => UK::ShiftLeft,
        KeyPress::ShiftRight  => UK::ShiftRight,
        KeyPress::ShiftHome   => UK::ShiftHome,
        KeyPress::ShiftEnd    => UK::ShiftEnd,
        KeyPress::Copy        => UK::Copy,
        KeyPress::Paste(s)    => UK::Paste(s.clone()),
        KeyPress::Undo        => UK::Undo,
        KeyPress::Redo        => UK::Redo,
    })
}

// =============================================================================
// Helpers
// =============================================================================


// =============================================================================
// ChartApp input methods
// =============================================================================

impl ChartApp {
    // -------------------------------------------------------------------------
    // Click
    // -------------------------------------------------------------------------

    /// Ensure an agent session is running for the currently selected mode and CLI.
    ///
    /// With multi-CLI support sessions autostart at app launch, so this is
    /// mostly a no-op unless a session died and needs restarting.
    /// True when the Agent PTY field currently owns keyboard focus — used by the
    /// platform runner to short-circuit named-key and char routing straight to the PTY.
    pub fn is_agent_pty_focused(&self) -> bool {
        let pty_id = WidgetId::new(crate::text_input::AGENT_PTY);
        self.input_coordinator.borrow().text_fields().is_focused(&pty_id)
    }

    /// Clear any active host-side PTY selection for the focused leaf.
    pub fn clear_pty_selection(&mut self) {
        if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
            if self.sidebar_state.agent_pty_selections.remove(&leaf_id).is_some() {
                self.sidebar_data_dirty = true;
            }
        }
    }

    /// Paste the given text into the focused PTY leaf's PTY session.
    pub fn paste_to_pty(&mut self, text: &str) {
        if text.is_empty() { return; }
        if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
            if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                if desc.mode == gate4agent::InstanceMode::Pty {
                    let id = desc.instance_id;
                    let _ = self.bridge.runtime().block_on(self.agent.write_pty_instance(id, text));
                }
            }
        }
    }

    /// Convert a screen (x, y) point to a PTY cell (row, col) if the point lies
    /// inside the focused leaf's agent_terminal_rect. Uses 7x19 cell metrics.
    fn pty_cell_at(&self, x: f64, y: f64) -> Option<(u16, u16)> {
        let (rx, ry, rw, rh) = self.sidebar_state.agent_terminal_rect?;
        let (rx, ry, rw, rh) = (rx as f64, ry as f64, rw as f64, rh as f64);
        if x < rx || x >= rx + rw || y < ry || y >= ry + rh {
            return None;
        }
        let (cols, rows) = self.sidebar_state.agent_terminal_size.unwrap_or((80, 24));
        let leaf_id = self.sidebar_state.focused_agent_leaf?;
        let scroll_offset = self.sidebar_state.agent_pty_scrolls.get(&leaf_id).map(|s| s.offset).unwrap_or(0.0);
        let col = ((x - rx) / 7.0).floor() as i32;
        let row = ((y - ry + scroll_offset) / 19.0).floor() as i32;
        let col = col.clamp(0, cols as i32 - 1) as u16;
        let row = row.clamp(0, rows as i32 - 1) as u16;
        Some((row, col))
    }

    /// Extract the currently selected PTY text from the focused leaf's snapshot.
    /// Returns an empty string if there is no selection or no grid.
    pub fn pty_selection_text(&self) -> String {
        use sidebar_content::agent_types::AgentSnapshotMode;
        let leaf_id = match self.sidebar_state.focused_agent_leaf {
            Some(id) => id,
            None => return String::new(),
        };
        let sel = match self.sidebar_state.agent_pty_selections.get(&leaf_id) {
            Some(s) if !s.is_empty() => *s,
            _ => return String::new(),
        };
        let snap = match self.sidebar_state.agent_leaf_snapshots.get(&leaf_id) {
            Some(s) => s,
            None => return String::new(),
        };
        let grid = match &snap.mode {
            AgentSnapshotMode::Pty(g) => g,
            _ => return String::new(),
        };
        let ((lo_row, lo_col), (hi_row, hi_col)) = sel.ordered();
        let lo_row = lo_row as usize;
        let hi_row = hi_row as usize;
        let lo_col = lo_col as usize;
        let hi_col = hi_col as usize;
        let total_cols = grid.cols as usize;
        let mut out = String::new();
        for row in lo_row..=hi_row {
            if row >= grid.cells.len() { break; }
            let row_cells = &grid.cells[row];
            let (c0, c1) = if lo_row == hi_row {
                (lo_col, hi_col)
            } else if row == lo_row {
                (lo_col, total_cols)
            } else if row == hi_row {
                (0, hi_col)
            } else {
                (0, total_cols)
            };
            let c1 = c1.min(row_cells.len());
            if c1 <= c0 { continue; }
            // Append cell chars then trim trailing spaces per row for cleanliness.
            let mut line = String::new();
            for cell in &row_cells[c0..c1] {
                line.push_str(&cell.ch);
            }
            let trimmed = line.trim_end();
            out.push_str(trimmed);
            if row < hi_row {
                out.push('\n');
            }
        }
        out
    }

    /// Extract the currently selected chat text from the focused leaf.
    ///
    /// Uses the word-wrapped line layout stored in `last_sidebar_result` so that
    /// only the exact visible lines within the selection range are copied — not the
    /// full raw `msg.content`.  Returns `None` when there is no active selection.
    ///
    /// After returning `Some(text)` the caller is responsible for clearing the
    /// selection via `agent_chat_selections.remove(&leaf_id)`.
    pub fn chat_selection_text(&self) -> Option<(uzor::panels::LeafId, String)> {
        let leaf_id = self.sidebar_state.focused_agent_leaf?;
        let sel = self.sidebar_state.agent_chat_selections.get(&leaf_id)?;
        if sel.is_empty() { return None; }
        let ((lo_msg, lo_line, lo_char), (hi_msg, hi_line, hi_char)) = sel.ordered();
        let rects = self.last_sidebar_result.as_ref().map(|r| &r.agent_chat_line_rects)?;
        let mut text = String::new();
        for entry in rects.iter() {
            let (msg_i, line_i, _, _, lid, line_text, _, _) = entry;
            if *lid != leaf_id { continue; }
            let pos = (*msg_i, *line_i);
            if pos < (lo_msg, lo_line) || pos > (hi_msg, hi_line) { continue; }
            if !text.is_empty() { text.push('\n'); }
            let is_first = pos == (lo_msg, lo_line);
            let is_last = pos == (hi_msg, hi_line);
            if is_first && is_last {
                // Single-line selection: extract char range.
                let slice: String = line_text.chars()
                    .skip(lo_char as usize)
                    .take((hi_char as usize).saturating_sub(lo_char as usize))
                    .collect();
                text.push_str(&slice);
            } else if is_first {
                // First line: from start_char to end.
                let slice: String = line_text.chars().skip(lo_char as usize).collect();
                text.push_str(&slice);
            } else if is_last {
                // Last line: from start to end_char.
                let slice: String = line_text.chars().take(hi_char as usize).collect();
                text.push_str(&slice);
            } else {
                // Middle line: full text.
                text.push_str(line_text);
            }
        }
        if text.is_empty() { None } else { Some((leaf_id, text)) }
    }

    /// Handle a left-click at screen coordinates `(x, y)`.
    ///
    /// Dispatch order:
    /// 1. `input_coordinator.process_click()` — modals, toolbars
    /// 2. Chart-canvas hit testing (crosshair, primitives, zoom)
    pub fn on_click(&mut self, x: f64, y: f64) {
        // 1. Check the input coordinator (modals, toolbars, dropdowns, panel overlays).
        // This MUST come before the drawing tool guard so toolbar clicks still work.
        // Drop the RefMut borrow before calling dispatch_panel_click (which needs &mut self).
        let clicked_widget_id = self.input_coordinator.borrow_mut().process_click(x, y)
            .map(|w| w.0.clone());
        if let Some(id) = clicked_widget_id {
            eprintln!("[ChartApp] click dispatched to: {}", id);
            self.dispatch_panel_click(&id, x, y);
            return;
        }

        // 1c. Click inside a free-slot order-flow panel — route to the active panel.
        //     Panels currently receive the click but have no built-in behavior.
        //     Infrastructure is wired so behavior can be added per-panel later.
        {
            use sidebar_content::state::RightSidebarPanel;
            let slot_idx_opt = match self.sidebar_state.right_panel {
                RightSidebarPanel::Slot1 => Some(0usize),
                RightSidebarPanel::Slot2 => Some(1),
                RightSidebarPanel::Slot3 => Some(2),
                RightSidebarPanel::Slot4 => Some(3),
                _ => None,
            };
            if let Some(idx) = slot_idx_opt {
                if let Some(ref sr) = self.last_sidebar_result {
                    for (wid, wr) in &sr.item_rects {
                        if wid.starts_with(&format!("slot:{}:leaf:", idx))
                            && wid.ends_with(":focus_content")
                            && x >= wr.x && x < wr.x + wr.width
                            && y >= wr.y && y < wr.y + wr.height
                        {
                            let parts: Vec<&str> = wid.split(':').collect();
                            if parts.len() >= 4 {
                                if let Ok(raw) = parts[3].parse::<u64>() {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use sidebar_content::free_slot::FreeItem;
                                    match item_opt {
                                        Some(FreeItem::Dom(_))
                                        | Some(FreeItem::L2Tape(_))
                                        | Some(FreeItem::TradeTape(_))
                                        | Some(FreeItem::Footprint(_))
                                        | Some(FreeItem::LiquidityHeatmap(_))
                                        | Some(FreeItem::BigTrades(_))
                                        | Some(FreeItem::VolumeProfile(_)) => {
                                            // Click acknowledged — no panel action yet.
                                            // Return without closing dropdowns so the click
                                            // doesn't reset UI state the user didn't touch.
                                            return;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            break;
                        }
                    }
                }
            }
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
        // Close slot spawn dropdown when clicking on canvas.
        self.sidebar_state.slot_spawn_dropdown = None;
        // Close agent popups when clicking on canvas.
        self.sidebar_state.agent_model_dropdown = None;
        self.sidebar_state.agent_perm_dropdown = None;
        self.sidebar_state.agent_sessions_dropdown = None;

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
                                    window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                                }
                                if next_mode.is_auto_y() {
                                    window.calc_auto_scale();
                                }
                                let is_auto = next_mode.is_auto_y();
                                for sp in &mut window.sub_panes {
                                    // update_sub_pane_ranges() already bakes in symmetrization
                                    // and 5% padding, so price_min/price_max already match
                                    // what is displayed.  Just flip the flag.
                                    sp.auto_scale = is_auto;
                                }
                            }
                            // Propagate scale_mode change to sync-group peers.
                            let viewport_state = self.panel_app.panel_grid
                                .window_for_leaf(leaf_id)
                                .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                            if let Some((view_start, bar_spacing)) = viewport_state {
                                self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, Some(next_mode));
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

        // Hit-test primitives and indicators, then dispatch to context menu.
        // Steps 1–5 (geometry + hit-test) live in ChartPanelGrid::handle_right_click.
        // Step 6 (opening context menu / settings) stays here — it touches panel_app state.
        let extended = self.build_extended_layout();

        // Pre-extract active-window data needed by the indicator closures.
        // Done before the mutable borrow of panel_grid inside handle_right_click.
        let (symbol, viewport, price_scale, sub_pane_ranges) = {
            if let Some(window) = self.panel_app.panel_grid.active_window() {
                let ranges: Vec<(u64, f64, f64)> = window.sub_panes.iter()
                    .map(|sp| (sp.instance_id, sp.price_min, sp.price_max))
                    .collect();
                (
                    window.symbol.clone(),
                    window.viewport.clone(),
                    window.price_scale.clone(),
                    ranges,
                )
            } else {
                (
                    String::new(),
                    zengeld_chart::Viewport::default(),
                    zengeld_chart::PriceScale::new(0.0, 1.0),
                    Vec::new(),
                )
            }
        };

        let indicator_overlay_hit = |lx: f64, ly: f64, chart_height: f64| {
            self.indicator_manager.hit_test_overlay(
                lx, ly, &symbol, &viewport, &price_scale, chart_height, 8.0,
            )
        };
        let indicator_subpane_hit = |instance_id: u64, plx: f64, ply: f64, pane_height: f64| {
            let (price_min, price_max) = sub_pane_ranges
                .iter()
                .find(|(id, _, _)| *id == instance_id)
                .map(|&(_, mn, mx)| (mn, mx))
                .unwrap_or((0.0, 100.0));
            self.indicator_manager.hit_test_sub_pane(
                instance_id, plx, ply, &viewport, price_min, price_max, pane_height, 8.0,
            )
        };

        let hit = self.panel_app.panel_grid.handle_right_click(
            x as f64, y as f64, &extended,
            indicator_overlay_hit,
            indicator_subpane_hit,
        );

        match hit {
            zengeld_chart::ChartRightClickHit::Primitive { prim_idx } => {
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
            }
            zengeld_chart::ChartRightClickHit::Indicator { indicator_id } => {
                // Right-clicked on an indicator line — select it and open its settings.
                self.selected_indicator_id = Some(indicator_id);
                self.panel_app.indicator_settings_state.open(indicator_id);
                eprintln!("[ChartApp] Indicator right-clicked: id={}, settings opened", indicator_id);
            }
            zengeld_chart::ChartRightClickHit::Background => {
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
            zengeld_chart::ChartRightClickHit::Miss => {}
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

        let double_click_wid: Option<String> = self.input_coordinator.borrow_mut().process_double_click(x, y).map(|w| w.0.clone());
        if let Some(wid) = double_click_wid {
            let id_str = &wid;
            if id_str == "watchlist:column_header" {
                self.watchlist_actions.push(crate::WatchlistAction::ResetSeparatorOffsets);
                self.watchlists_dirty = true;
                self.persist_watchlists();
                return;
            }
            if let Some(rest) = id_str.strip_prefix("agent:leaf:") {
                if let Some(id_part) = rest.strip_suffix(":focus_content") {
                    if let Ok(raw) = id_part.parse::<u64>() {
                        let leaf_id = uzor::panels::LeafId(raw);
                        let is_chat = self.sidebar_state.agent_leaves.get(&leaf_id)
                            .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                            .unwrap_or(false);
                        if is_chat {
                            self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_CHAT);
                            self.sidebar_data_dirty = true;
                        }
                    }
                }
                return;
            }
            // slot:{idx}:leaf:{leaf_id}:focus_content
            if id_str.starts_with("slot:") && id_str.ends_with(":focus_content") {
                let parts: Vec<&str> = id_str.split(':').collect();
                // parts: ["slot", idx, "leaf", leaf_id, "focus_content"]
                if parts.len() >= 5 {
                    if let (Ok(idx), Ok(raw)) = (parts[1].parse::<usize>(), parts[3].parse::<u64>()) {
                        let leaf_id = uzor::panels::LeafId(raw);
                        if idx < self.sidebar_state.slot_dockings.len() {
                            let item_opt = self.sidebar_state.slot_dockings[idx]
                                .inner()
                                .tree()
                                .leaf(leaf_id)
                                .and_then(|l| l.active_panel().cloned());
                            use sidebar_content::free_slot::FreeItem;
                            match item_opt {
                                Some(FreeItem::Dom(pid)) => {
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        state.auto_center = true;
                                        state.center_price = state.market_price;
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                                Some(item @ FreeItem::L2Tape(_))
                                | Some(item @ FreeItem::Footprint(_))
                                | Some(item @ FreeItem::BigTrades(_))
                                | Some(item @ FreeItem::LiquidityHeatmap(_))
                                | Some(item @ FreeItem::VolumeProfile(_)) => {
                                    let local_id = match &item {
                                        FreeItem::L2Tape(_) => "l2tape:body",
                                        FreeItem::Footprint(_) => "footprint:body",
                                        FreeItem::BigTrades(_) => "bigtrades:body",
                                        FreeItem::LiquidityHeatmap(_) => "heatmap:body",
                                        FreeItem::VolumeProfile(_) => "volprofile:body",
                                        _ => unreachable!(),
                                    };
                                    if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                        panel.handle_double_click(local_id, x, y);
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                                Some(FreeItem::TradeTape(pid)) => {
                                    if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                                        state.handle_double_click();
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                return;
            }
        }

        let extended = self.build_extended_layout();
        let overlay_results_dc = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(&extended)
            .with_overlays(&overlay_results_dc);

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
            use zengeld_chart::ScaleMode;
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                if let Some(sub_pane) = window.sub_panes.get_mut(pane_index) {
                    sub_pane.auto_scale = true;
                }
                // Restore the main chart to Auto so the A/M button reflects the true state.
                window.price_scale.scale_mode = ScaleMode::Auto;
            }
            // Propagate Auto mode to sync-group peers.
            let viewport_state = self.panel_app.panel_grid.active_window()
                .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
            let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
            if let (Some((view_start, bar_spacing)), Some(active_leaf)) = (viewport_state, active_leaf_opt) {
                self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, Some(ScaleMode::Auto));
            }
        }
    }

    // -------------------------------------------------------------------------
    // Drag
    // -------------------------------------------------------------------------

    /// Handle drag start at `(x, y)`.
    pub fn on_drag_start(&mut self, x: f64, y: f64) -> bool {
        // Track whether this drag started on a UI element (for crosshair suppression).
        self.ui_drag_active = self.input_coordinator.borrow_mut().is_over_ui();

        // ── Agent-panel separator drag initiation ────────────────────────────
        // MUST run BEFORE PTY/Chat drag so separator hit zones take priority
        // over the leaf content areas that may overlap the separator zone.
        if self.agent_sep_drag.is_none() {
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
            if let Some(ref wid) = hovered_wid {
                if let Some(idx_str) = wid.strip_prefix("agent:sep:") {
                    if let Ok(sep_idx) = idx_str.parse::<usize>() {
                        let sep_info: Option<(uzor::panels::SeparatorOrientation, f32)> = {
                            let docking = self.sidebar_state.agent_docking.inner();
                            docking.separators().get(sep_idx).map(|sep| {
                                use uzor::panels::SeparatorOrientation;
                                let area = docking.layout_area();
                                let total = match sep.orientation {
                                    SeparatorOrientation::Vertical => area.width,
                                    SeparatorOrientation::Horizontal => area.height,
                                };
                                (sep.orientation, total)
                            })
                        };
                        if let Some((orient, total_size)) = sep_info {
                            use uzor::panels::SeparatorOrientation;
                            let start_pos = match orient {
                                SeparatorOrientation::Vertical => x,
                                SeparatorOrientation::Horizontal => y,
                            };
                            self.agent_sep_drag = Some((sep_idx, start_pos, total_size));
                            self.ui_drag_active = true;
                            return false;
                        }
                    }
                }
            }
        }

        // ── Agent chat / PTY scrollbar handle drag + track click ─────────────
        // MUST run BEFORE PTY/Chat drag so scrollbar clicks are not intercepted
        // by the focus_content handler that covers the full leaf area (which
        // overlaps the scrollbar strip).
        if self.sidebar_state.is_right_open() && !self.ui_drag_active {
            // Collect all leaf scrollbar infos from the last render pass before taking mutable
            // borrows on sidebar_state (avoids simultaneous borrow conflicts).
            let leaf_scroll_infos: Vec<(uzor::panels::LeafId, bool, crate::scroll_dispatch::ScrollableInfo)> = {
                if let Some(ref sidebar_result) = self.last_sidebar_result {
                    sidebar_result.agent_leaf_scrollbar_rects.iter()
                        .filter_map(|(&lid, &(handle_rect, track_rect))| {
                            let (content_h, vp_h) = sidebar_result.agent_leaf_content_heights
                                .get(&lid).copied().unwrap_or((0.0, 0.0));
                            if vp_h <= 0.0 { return None; }
                            let is_chat = self.sidebar_state.agent_leaves.get(&lid)
                                .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                                .unwrap_or(true);
                            Some((lid, is_chat, crate::scroll_dispatch::ScrollableInfo {
                                handle_rect,
                                track_rect,
                                content_height: content_h,
                                viewport_height: vp_h,
                                viewport_rect: None,
                            }))
                        })
                        .collect()
                } else {
                    vec![]
                }
            };

            use crate::scroll_dispatch::{try_start_scrollbar_drag, try_handle_track_click};
            for (leaf_id, is_chat, info) in &leaf_scroll_infos {
                if *is_chat {
                    let scroll = self.sidebar_state.agent_chat_scrolls.entry(*leaf_id).or_default();
                    if try_start_scrollbar_drag(x, y, &mut [(&*info, scroll)]) {
                        return false;
                    }
                    let scroll = self.sidebar_state.agent_chat_scrolls.entry(*leaf_id).or_default();
                    if try_handle_track_click(x, y, &mut [(&*info, scroll)]) {
                        return false;
                    }
                } else {
                    let scroll = self.sidebar_state.agent_pty_scrolls.entry(*leaf_id).or_default();
                    if try_start_scrollbar_drag(x, y, &mut [(&*info, scroll)]) {
                        return false;
                    }
                    let scroll = self.sidebar_state.agent_pty_scrolls.entry(*leaf_id).or_default();
                    if try_handle_track_click(x, y, &mut [(&*info, scroll)]) {
                        return false;
                    }
                }
            }
        }

        // ── PTY host-side selection drag ────────────────────────────────────
        // If the drag starts inside the Agent PTY terminal (in PTY mode),
        // begin a host-side cell selection. This MUST run before TIM drag so
        // the sidebar scroll-drag fallback doesn't hijack the motion.
        {
            // Auto-focus the PTY leaf on mouse-down so selection works immediately
            // even when clicking a non-focused leaf.
            if let Some((hovered_leaf_id, content_rect)) = self.last_sidebar_result.as_ref().and_then(|sr| {
                sr.item_rects.iter()
                    .filter_map(|(wid, wrect)| {
                        let id_str = wid.strip_prefix("agent:leaf:")?.strip_suffix(":focus_content")?;
                        let raw: u64 = id_str.parse().ok()?;
                        let lid = uzor::panels::LeafId(raw);
                        if x >= wrect.x && x < wrect.x + wrect.width
                            && y >= wrect.y && y < wrect.y + wrect.height
                        {
                            Some((lid, *wrect))
                        } else {
                            None
                        }
                    })
                    .next()
            }) {
                let leaf_mode = self.sidebar_state.agent_leaves.get(&hovered_leaf_id)
                    .map(|d| d.mode);
                if self.sidebar_state.focused_agent_leaf != Some(hovered_leaf_id) {
                    self.sidebar_state.focused_agent_leaf = Some(hovered_leaf_id);
                    self.sidebar_state.agent_docking.inner_mut().set_active_leaf(hovered_leaf_id);
                    // Sync the chat text_input buffer so keystrokes go to the
                    // correct leaf's buffer after focus switches.
                    if leaf_mode == Some(gate4agent::InstanceMode::Chat) {
                        let buf = self.sidebar_state.agent_input_buffers
                            .get(&hovered_leaf_id).map(|s| s.as_str()).unwrap_or("");
                        let chat_id = WidgetId::new(crate::text_input::AGENT_CHAT);
                        self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, buf);
                    }
                    self.sidebar_data_dirty = true;
                }
                if leaf_mode == Some(gate4agent::InstanceMode::Pty) {
                    // Update terminal rect immediately so pty_cell_at() works
                    // on this same mousedown without waiting for a re-render.
                    self.sidebar_state.agent_terminal_rect = Some((
                        content_rect.x as f32,
                        content_rect.y as f32,
                        content_rect.width as f32,
                        content_rect.height as f32,
                    ));
                    let pty_cols = ((content_rect.width / 7.0) as u16).max(1);
                    let pty_rows = ((content_rect.height / 19.0) as u16).max(1);
                    self.sidebar_state.agent_terminal_size = Some((pty_cols, pty_rows));
                }
            }

            let is_pty_leaf = self.sidebar_state.focused_agent_leaf
                .and_then(|id| self.sidebar_state.agent_leaves.get(&id))
                .map(|d| d.mode == gate4agent::InstanceMode::Pty)
                .unwrap_or(false);
            if is_pty_leaf {
                if let Some((row, col)) = self.pty_cell_at(x, y) {
                    eprintln!("[gate4agent::pty] drag_start @ row={} col={}", row, col);
                    if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                        self.sidebar_state.agent_pty_selections.insert(
                            leaf_id,
                            sidebar_content::state::PtySelection::new(row, col),
                        );
                    }
                    self.agent_pty_drag_active = true;
                    // Also focus PTY so keyboard events still route to it.
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_PTY);
                    self.sidebar_data_dirty = true;
                    // Return false — NOT dismissed. We want subsequent drag_move
                    // events and the eventual drag_end. Returning true tells the
                    // platform runner to synthesise drag_end immediately.
                    return false;
                }
            }
        }

        // ── Chat host-side selection drag ────────────────────────────────────
        // If the drag starts inside a Chat leaf's content area, begin a
        // line-level text selection. Runs after PTY so PTY leaves are not affected.
        {
            let is_chat_leaf = self.sidebar_state.focused_agent_leaf
                .and_then(|id| self.sidebar_state.agent_leaves.get(&id))
                .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                .unwrap_or(false);
            if is_chat_leaf {
                // Use content_rect from auto-focus hit-test (not stale agent_content_rect
                // from previous render, which may belong to a different leaf).
                let in_content = self.last_sidebar_result.as_ref()
                    .and_then(|sr| {
                        sr.item_rects.iter()
                            .filter_map(|(wid, wrect)| {
                                let id_str = wid.strip_prefix("agent:leaf:")?.strip_suffix(":focus_content")?;
                                let raw: u64 = id_str.parse().ok()?;
                                let lid = uzor::panels::LeafId(raw);
                                if lid == self.sidebar_state.focused_agent_leaf?
                                    && x >= wrect.x && x < wrect.x + wrect.width
                                    && y >= wrect.y && y < wrect.y + wrect.height
                                {
                                    Some(true)
                                } else {
                                    None
                                }
                            })
                            .next()
                    })
                    .unwrap_or(false);
                if in_content {
                    // Focus chat input immediately on mousedown (like PTY does).
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_CHAT);
                    self.sidebar_data_dirty = true;

                    if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                        let hit = self.last_sidebar_result.as_ref()
                            .and_then(|r| {
                                r.agent_chat_line_rects.iter()
                                    .find(|e| e.4 == leaf_id && y >= e.2 && y < e.3)
                                    .map(|e| (e.0, e.1, &e.5, e.6))
                            });
                        if let Some((msg_idx, line_idx, line_text, text_x)) = hit {
                            // Check if this is a click on the header (line 0) of a
                            // collapsible Thinking or Tool bubble → toggle expand/collapse.
                            if line_idx == 0 {
                                let is_collapsible = self.sidebar_state.agent_leaf_snapshots.get(&leaf_id)
                                    .and_then(|snap| {
                                        if let sidebar_content::agent_types::AgentSnapshotMode::Chat(ref msgs) = snap.mode {
                                            msgs.get(msg_idx as usize)
                                        } else {
                                            None
                                        }
                                    })
                                    .map(|msg| msg.role == sidebar_content::agent_types::ChatRole::Thinking || msg.role == sidebar_content::agent_types::ChatRole::Tool)
                                    .unwrap_or(false);
                                if is_collapsible {
                                    let key = (leaf_id, msg_idx);
                                    if self.sidebar_state.agent_chat_expanded.contains(&key) {
                                        self.sidebar_state.agent_chat_expanded.remove(&key);
                                    } else {
                                        self.sidebar_state.agent_chat_expanded.insert(key);
                                    }
                                    self.sidebar_data_dirty = true;
                                    return false;
                                }
                            }
                            let char_idx = chat_char_idx_from_x(line_text, x, text_x);
                            self.sidebar_state.agent_chat_selections.insert(
                                leaf_id,
                                sidebar_content::state::ChatSelection::new_at(msg_idx, line_idx, char_idx),
                            );
                            self.sidebar_state.agent_chat_drag_active = true;
                            return false;
                        }
                    }
                    // No chat line was hit — check if drag started inside the
                    // input field rect so TextFieldStore can begin a text selection.
                    if let Some(ref sr) = self.last_sidebar_result {
                        if let Some(ref rect) = sr.agent_input_rect {
                            if x >= rect.x && x < rect.x + rect.width
                                && y >= rect.y && y < rect.y + rect.height
                            {
                                self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                                return false;
                            }
                        }
                    }
                    return false;
                }
            }
        }

        // Let the text-field store claim the drag if (x, y) falls inside a registered
        // text field (e.g. the HexColor field when the L2 color picker is visible).
        // This must run BEFORE the sidebar-separator check so early returns don't miss it.
        self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);

        // Dismiss color picker on drag-start outside the popup.
        // This treats on_drag_start as a click for popup dismissal purposes,
        // preventing the UX issue where dragging outside doesn't close the picker.
        {
            fn outside_popup(picker: &zengeld_chart::ui::color_picker_state::ColorPickerState, x: f64, y: f64) -> bool {
                if !picker.is_open() { return false; }
                let (ox, oy) = picker.origin;
                let (pw, ph) = match picker.level {
                    zengeld_chart::ui::color_picker_state::ColorPickerLevel::L1 => picker.l1_config().calculate_size(),
                    zengeld_chart::ui::color_picker_state::ColorPickerLevel::L2 => picker.l2_config().calculate_size(),
                    _ => return false,
                };
                x < ox || x > ox + pw || y < oy || y > oy + ph
            }

            let mut dismissed = false;
            if outside_popup(&self.panel_app.primitive_settings_state.color_picker, x, y) {
                self.panel_app.primitive_settings_state.close_color_picker();
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                dismissed = true;
            }
            if outside_popup(&self.panel_app.indicator_settings_state.color_picker, x, y) {
                self.panel_app.indicator_settings_state.close_color_picker();
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                dismissed = true;
            }
            if outside_popup(&self.panel_app.chart_settings_state.color_picker, x, y) {
                self.panel_app.chart_settings_state.close_color_picker();
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                dismissed = true;
            }
            if outside_popup(&self.panel_app.compare_settings_state.color_picker, x, y) {
                self.panel_app.compare_settings_state.close_color_picker();
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                dismissed = true;
            }
            if outside_popup(&self.panel_app.panel_color_picker, x, y) {
                self.panel_app.panel_color_picker.close();
                self.panel_app.sync_color_grid.adding_custom_color = false;
                self.input_coordinator.borrow_mut().text_fields_mut().blur();
                dismissed = true;
            }
            if dismissed {
                self.drag_dismissed_popup = true;
                return true;
            }
        }

        // Check if drag starts on the sidebar separator — if so, begin sidebar resize.
        // This must be checked BEFORE the modal guard so the separator is reachable
        // even when a sidebar panel is open (which registers the sidebar as a UI widget).
        let on_sidebar_separator = self.input_coordinator.borrow_mut().hovered_widget()
            .map(|h| h.0 == "right_sidebar_separator")
            .unwrap_or(false);
        if on_sidebar_separator {
            self.sidebar_separator_drag_active = true;
            return false;
        }

        // Check if drag starts on a watchlist column separator — begin separator drag.
        // Must be checked before the watchlist row drag so separators win the hit-test.
        if self.sidebar_state.right_panel == sidebar_content::state::RightSidebarPanel::Watchlist {
            // Widget ids use 1-based indexing; strip prefix and subtract 1 for 0-based sep index.
            let on_sep = self.input_coordinator.borrow_mut().hovered_widget()
                .and_then(|h| h.0.strip_prefix("watchlist_sep_").and_then(|s| s.parse::<usize>().ok()))
                .map(|one_based| one_based.saturating_sub(1));
            // Skip separator drag when columns are in equal-width (aligned) mode.
            let align_cols = self.sidebar_state.watchlist_manager
                .active_list()
                .map(|l| l.column_config.align_columns)
                .unwrap_or(true);
            if let Some(sep_idx) = on_sep.filter(|_| !align_cols) {
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
                        if c.show_exchange     { n += 1; }
                        if c.show_account_type { n += 1; }
                        if c.show_last_price   { n += 1; }
                        if c.show_change_pct   { n += 1; }
                        if c.show_change_abs   { n += 1; }
                        if c.show_high_low     { n += 2; }
                        if c.show_volume       { n += 1; }
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
                return false;
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
                    return false;
                }
            }
        }

        // Signal group scrollbar track click (handle drag is dispatched via InputCoordinator above).
        if self.sidebar_state.is_right_open() && !self.ui_drag_active {
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                for &(instance_id, ref _handle_rect, ref track_rect, content_h, viewport_h)
                    in &sidebar_result.signal_group_scrollbar_rects
                {
                    // Track click — jump scroll to clicked position.
                    let track_hit = x >= track_rect.x
                        && x <= track_rect.x + track_rect.width
                        && y >= track_rect.y
                        && y <= track_rect.y + track_rect.height;
                    if track_hit {
                        self.sidebar_state
                            .signal_group_scroll
                            .entry(instance_id)
                            .or_default()
                            .handle_track_click(
                                y,
                                track_rect.y,
                                track_rect.height,
                                content_h,
                                viewport_h,
                            );
                        return false;
                    }
                }
            }
        }

        // Right sidebar scrollbar track click (handle drag is dispatched via InputCoordinator above).
        if self.sidebar_state.is_right_open() && !self.ui_drag_active {
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                // Scrollbar track click — jump to position
                if let Some(ref track_rect) = sidebar_result.scrollbar_track_rect {
                    let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                        && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                    if hit {
                        let content_h = sidebar_result.content_height;
                        let viewport_h = sidebar_result.content_rect.height;
                        self.sidebar_state.current_right_scroll_mut().handle_track_click(
                            y, track_rect.y, track_rect.height, content_h, viewport_h,
                        );
                        return false;
                    }
                }
            }
        }

        // ── Free-slot separator drag initiation ──────────────────────────────
        // Detect `"slot:{slot_idx}:sep:{sep_idx}"` widget hover → begin separator resize drag.
        if self.slot_sep_drag.is_none() {
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
            if let Some(ref wid) = hovered_wid {
                if let Some(rest) = wid.strip_prefix("slot:") {
                    if let Some((slot_str, sep_str)) = rest.split_once(":sep:") {
                        if let (Ok(slot_idx), Ok(sep_idx)) =
                            (slot_str.parse::<usize>(), sep_str.parse::<usize>())
                        {
                            if slot_idx < 4 {
                                let sep_info: Option<(uzor::panels::SeparatorOrientation, f32)> = {
                                    let docking = self.sidebar_state.slot_dockings[slot_idx].inner();
                                    docking.separators().get(sep_idx).map(|sep| {
                                        use uzor::panels::SeparatorOrientation;
                                        let area = docking.layout_area();
                                        let total = match sep.orientation {
                                            SeparatorOrientation::Vertical => area.width,
                                            SeparatorOrientation::Horizontal => area.height,
                                        };
                                        (sep.orientation, total)
                                    })
                                };
                                if let Some((orient, total_size)) = sep_info {
                                    use uzor::panels::SeparatorOrientation;
                                    let start_pos = match orient {
                                        SeparatorOrientation::Vertical => x,
                                        SeparatorOrientation::Horizontal => y,
                                    };
                                    self.slot_sep_drag = Some((slot_idx, sep_idx, start_pos, total_size));
                                    self.ui_drag_active = true;
                                    return false;
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── DOM panel drag-to-scroll initiation ─────────────────────────────
        // When drag starts over a `slot:{idx}:leaf:{leaf_id}:focus_content`
        // widget that hosts a DOM panel, record state for smooth price scroll.
        if self.slot_dom_drag.is_none() {
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
            if let Some(ref wid) = hovered_wid {
                if let Some(rest) = wid.strip_prefix("slot:") {
                    if let Some((slot_str, leaf_rest)) = rest.split_once(":leaf:") {
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":focus_content") {
                            if let (Ok(slot_idx), Ok(raw)) =
                                (slot_str.parse::<usize>(), leaf_id_str.parse::<u64>())
                            {
                                if slot_idx < 4 {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use sidebar_content::free_slot::FreeItem;
                                    match item_opt {
                                        Some(FreeItem::Dom(pid)) => {
                                            self.slot_dom_drag = Some((slot_idx, leaf_id, pid, y, 20.0));
                                            self.ui_drag_active = true;
                                            return false;
                                        }
                                        Some(item @ FreeItem::L2Tape(_))
                                        | Some(item @ FreeItem::Footprint(_))
                                        | Some(item @ FreeItem::BigTrades(_))
                                        | Some(item @ FreeItem::LiquidityHeatmap(_))
                                        | Some(item @ FreeItem::VolumeProfile(_)) => {
                                            let local_id = match &item {
                                                FreeItem::L2Tape(_) => "l2tape:body",
                                                FreeItem::Footprint(_) => "footprint:body",
                                                FreeItem::BigTrades(_) => "bigtrades:body",
                                                FreeItem::LiquidityHeatmap(_) => "heatmap:body",
                                                FreeItem::VolumeProfile(_) => "volprofile:body",
                                                _ => unreachable!(),
                                            };
                                            if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                                if panel.handle_drag_start(local_id, x, y) {
                                                    self.active_drag_panel = Some((item, local_id.to_string(), x, y));
                                                    self.ui_drag_active = true;
                                                    return false;
                                                }
                                            }
                                        }
                                        Some(FreeItem::TradeTape(pid)) => {
                                            self.slot_tradetape_drag = Some((pid, x, y));
                                            self.ui_drag_active = true;
                                            return false;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
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

        // Modal header drags — dispatched via InputCoordinator registration.
        // Each modal registers its header rect with Sense::DRAG during render.
        if let Some(wid) = self.input_coordinator.borrow_mut().process_drag_start(x, y) {
            let id = wid.0.clone();
            if id.ends_with(":header") {
                let prefix = &id[..id.len() - 7];
                let started = match prefix {
                    "prim_settings" => {
                        if let Some(ref ps) = self.frame_result.as_ref().and_then(|r| r.primitive_settings.as_ref()) {
                            self.panel_app.primitive_settings_state.start_drag(x, y, ps.header_rect.x, ps.header_rect.y);
                            true
                        } else { false }
                    }
                    "chart_settings" => {
                        if let Some(ref cs) = self.frame_result.as_ref().and_then(|r| r.chart_settings.as_ref()) {
                            self.panel_app.chart_settings_state.start_drag(x, y, cs.header_rect.x, cs.header_rect.y);
                            true
                        } else { false }
                    }
                    "user_settings" => {
                        if let Some(ref us) = self.frame_result.as_ref().and_then(|r| r.user_settings.as_ref()) {
                            self.panel_app.user_settings_state.start_drag(x, y, us.header_rect.x, us.header_rect.y);
                            true
                        } else { false }
                    }
                    "overlay_settings" => {
                        if let Some(ref os) = self.frame_result.as_ref().and_then(|r| r.overlay_settings.as_ref()) {
                            self.panel_app.overlay_settings_state.start_drag(x, y, os.header_rect.x, os.header_rect.y);
                            true
                        } else { false }
                    }
                    "tags_tabs" => {
                        if let Some(ref tt) = self.frame_result.as_ref().and_then(|r| r.tags_tabs.as_ref()) {
                            self.panel_app.tags_tabs_state.start_drag(x, y, tt.header_rect.x, tt.header_rect.y);
                            true
                        } else { false }
                    }
                    "ind_settings" => {
                        if let Some(ref is) = self.frame_result.as_ref().and_then(|r| r.indicator_settings.as_ref()) {
                            self.panel_app.indicator_settings_state.start_drag(x, y, is.header_rect.x, is.header_rect.y);
                            true
                        } else { false }
                    }
                    "alert_settings" | "alert_set" => {
                        if let Some(ref ar) = self.frame_result.as_ref().and_then(|r| r.alert_settings.as_ref()) {
                            self.panel_app.alert_settings_state.start_drag(x, y, ar.header_rect.x, ar.header_rect.y);
                            true
                        } else { false }
                    }
                    "compare_settings" => {
                        if let Some(ref cs) = self.frame_result.as_ref().and_then(|r| r.compare_settings.as_ref()) {
                            self.panel_app.compare_settings_state.start_drag(x, y, cs.header_rect.x, cs.header_rect.y);
                            true
                        } else { false }
                    }
                    "preset_name" => {
                        if let Some(ref pni) = self.frame_result.as_ref().and_then(|r| r.preset_name_input.as_ref()) {
                            self.panel_app.preset_name_input.start_drag(x, y, pni.modal_rect.x, pni.modal_rect.y);
                            true
                        } else { false }
                    }
                    "chart_browser" => {
                        if let Some(ref br) = self.frame_result.as_ref().and_then(|r| r.chart_browser.as_ref()) {
                            self.panel_app.chart_browser.start_drag(x, y, br.modal_rect.x, br.modal_rect.y);
                            true
                        } else { false }
                    }
                    "wl_group_name" => {
                        if let Some(ref gni) = self.last_wl_group_name_result {
                            self.wl_group_name_input.start_drag(x, y, gni.modal_rect.x, gni.modal_rect.y);
                            true
                        } else { false }
                    }
                    "watchlist_modal" => {
                        if let Some(ref wl) = self.last_watchlist_modal_result {
                            self.watchlist_modal.start_drag(x, y, wl.modal_rect.x, wl.modal_rect.y);
                            true
                        } else { false }
                    }
                    "search_modal" => {
                        if let Some(ref smr) = self.search_modal_result {
                            self.modal_state.start_drag(x, y, smr.modal_rect.x, smr.modal_rect.y);
                            true
                        } else { false }
                    }
                    _ => false,
                };
                if started {
                    return false;
                }
            }

            // Scrollbar handle drags — dispatched via InputCoordinator DRAG registration.
            if id.ends_with(":scrollbar_handle") {
                let prefix = &id[..id.len() - ":scrollbar_handle".len()];
                let started = if prefix == "sidebar" {
                    self.sidebar_state.current_right_scroll_mut().start_drag(y);
                    true
                } else if let Some(id_str) = prefix.strip_prefix("signal_group:") {
                    if let Ok(instance_id) = id_str.parse::<u64>() {
                        self.sidebar_state
                            .signal_group_scroll
                            .entry(instance_id)
                            .or_default()
                            .start_drag(y);
                        true
                    } else {
                        false
                    }
                } else if let Some(id_str) = prefix.strip_prefix("agent:leaf:") {
                    if let Ok(raw) = id_str.parse::<u64>() {
                        let leaf_id = uzor::panels::LeafId(raw);
                        let is_chat = self.sidebar_state.agent_leaves.get(&leaf_id)
                            .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                            .unwrap_or(true);
                        if is_chat {
                            self.sidebar_state.agent_chat_scrolls
                                .entry(leaf_id)
                                .or_default()
                                .start_drag(y);
                        } else {
                            self.sidebar_state.agent_pty_scrolls
                                .entry(leaf_id)
                                .or_default()
                                .start_drag(y);
                        }
                        true
                    } else {
                        false
                    }
                } else if prefix == "chart_settings" {
                    self.panel_app.chart_settings_state.scroll.start_drag(y);
                    true
                } else if prefix == "tags_tabs" {
                    let scroll_state = tags_tabs_active_scroll(&mut self.panel_app.tags_tabs_state);
                    scroll_state.start_drag(y);
                    true
                } else if prefix == "user_settings" {
                    use zengeld_chart::ui::modal_settings::UserSettingsTab;
                    let scroll = match self.panel_app.user_settings_state.active_tab {
                        UserSettingsTab::General => &mut self.panel_app.user_settings_state.general_tab_scroll,
                        UserSettingsTab::Sync => &mut self.panel_app.user_settings_state.sync_tab_scroll,
                        UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_keys_scroll,
                        UserSettingsTab::Performance => &mut self.panel_app.user_settings_state.performance_tab_scroll,
                    };
                    scroll.start_drag(y);
                    true
                } else if prefix == "user_settings:profile_list" {
                    self.panel_app.user_settings_state.profile_list_scroll.start_drag(y);
                    true
                } else if prefix == "ind_settings" {
                    self.panel_app.indicator_settings_state.scroll.start_drag(y);
                    true
                } else if prefix == "alert_settings" {
                    self.panel_app.alert_settings_state.list_scroll.start_drag(y);
                    true
                } else if prefix == "search_modal" {
                    self.modal_state.scroll.start_drag(y);
                    true
                } else if prefix == "watchlist_modal" {
                    self.watchlist_modal.scroll.start_drag(y);
                    true
                } else if prefix == "chart_browser" {
                    self.panel_app.chart_browser.scroll.start_drag(y);
                    true
                } else {
                    false
                };
                if started {
                    self.ui_drag_active = true;
                    return false;
                }
            }

            // Compare settings TF dual-handle sliders (Visibility tab) and line_width slider (Style tab).
            if id.starts_with("cmp_settings:item:") {
                let field_id = &id["cmp_settings:item:".len()..];
                if field_id.starts_with("tf_") && field_id.ends_with("_slider")
                    && self.panel_app.compare_settings_state.is_open()
                {
                    let frame_ref = self.frame_result.as_ref();
                    let track_data = frame_ref
                        .and_then(|r| r.compare_settings.as_ref())
                        .and_then(|cs| cs.tf_slider_tracks.iter().find(|t| t.field_id == field_id))
                        .map(|t| (t.field_id.clone(), t.track_x, t.track_width, t.min_val, t.max_val));
                    if let Some((fid, track_x, track_width, min_val, max_val)) = track_data {
                        let t = ((x - track_x) / track_width).clamp(0.0, 1.0);
                        let tf_config = self.panel_app.compare_settings_state
                            .cached_timeframe_visibility.clone()
                            .unwrap_or_else(TimeframeVisibilityConfig::all);
                        let handle = if let Some(tf_idx) = fid.strip_prefix("tf_")
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
                                _ => if t <= 0.5 { (min_val as u32, min_val as u32) } else { (max_val as u32, max_val as u32) },
                            };
                            let min_pos = (cur_min as f64 - min_val) / (max_val - min_val);
                            let max_pos = (cur_max as f64 - min_val) / (max_val - min_val);
                            if (t - min_pos).abs() <= (t - max_pos).abs() { DualSliderHandle::Min } else { DualSliderHandle::Max }
                        } else if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max };
                        self.panel_app.compare_settings_state.start_dual_slider_drag(
                            &fid, track_x, track_width, min_val, max_val, handle, x,
                        );
                        return false;
                    }
                }
            }

            if id == "cmp_settings:line_width_slider" && self.panel_app.compare_settings_state.is_open() {
                let track_data = self.frame_result.as_ref()
                    .and_then(|r| r.compare_settings.as_ref())
                    .and_then(|cs| cs.line_width_slider.as_ref())
                    .map(|t| (t.field_id.clone(), t.track_x, t.track_width, t.min_val, t.max_val));
                if let Some((fid, track_x, track_width, min_val, max_val)) = track_data {
                    self.panel_app.compare_settings_state.start_slider_drag(
                        &fid, track_x, track_width, min_val, max_val,
                    );
                    self.panel_app.compare_settings_state.update_slider_drag(x);
                    return false;
                }
            }

            // Primitive settings sliders — routed via prim_settings:item:{field_id} DRAG registration.
            if id.starts_with("prim_settings:item:")
                && self.panel_app.primitive_settings_state.is_open()
                && !self.panel_app.primitive_settings_state.is_color_picker_open()
            {
                let field_id = id["prim_settings:item:".len()..].to_string();
                let track_data = self.frame_result.as_ref()
                    .and_then(|r| r.primitive_settings.as_ref())
                    .and_then(|ps| ps.slider_tracks.iter().find(|s| s.field_id == field_id))
                    .map(|s| (s.field_id.clone(), s.track_x, s.track_width, s.min_val, s.max_val));
                if let Some((fid, track_x, track_width, min_val, max_val)) = track_data {
                    if fid.starts_with("tf_") && fid.ends_with("_slider") {
                        let t = ((x - track_x) / track_width).clamp(0.0, 1.0);
                        let handle = if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
                            if let Some(data) = self.panel_app.panel_grid.active_window()
                                .and_then(|w| w.drawing_manager.get_data_at(idx))
                            {
                                if let Some(tf_idx) = fid.strip_prefix("tf_")
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
                                        _ => if t <= 0.5 { (min_val as u32, min_val as u32) } else { (max_val as u32, max_val as u32) },
                                    };
                                    let min_pos = (current_min as f64 - min_val) / (max_val - min_val);
                                    let max_pos = (current_max as f64 - min_val) / (max_val - min_val);
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
                                    if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                                }
                            } else {
                                if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                            }
                        } else {
                            if t <= 0.5 { DualSliderHandle::Min } else { DualSliderHandle::Max }
                        };
                        self.panel_app.primitive_settings_state.start_dual_slider_drag_from_track(
                            &fid, track_x, track_width, min_val, max_val, handle, x,
                        );
                        return false;
                    }
                    self.panel_app.primitive_settings_state.start_slider_drag_from_track(
                        &fid, track_x, track_width, min_val, max_val,
                    );
                    self.panel_app.primitive_settings_state.update_slider_drag_float(x);
                    return false;
                }
            }
        }

        if let Some(result) = &self.frame_result {
            // Profile manager / wizard inputs — text select drag on any active profile manager or wizard input.
            if self.panel_app.user_settings_state.show_profile_manager || self.panel_app.user_settings_state.show_welcome_wizard {
                if let Some(ref us) = result.user_settings {
                    // Iterate all registered input char-position entries and check if
                    // the drag started inside that input's rect.
                    let mut drag_field: Option<String> = None;
                    let mut drag_cursor: usize = 0;
                    for (field_id, char_positions) in &us.input_char_positions {
                        // Find the matching content_item rect.
                        if let Some((_, input_rect)) = us.content_items.iter().find(|(k, _)| k == field_id) {
                            if input_rect.contains(x, y) && !char_positions.is_empty() {
                                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(char_positions, x);
                                drag_field = Some(field_id.clone());
                                drag_cursor = new_cursor;
                                break;
                            }
                        }
                    }
                    if let Some(field_id) = drag_field {
                        // Set cursor and selection anchor on the appropriate editing state.
                        // Also activate focus (drag = click, should focus the field).
                        // Clear selection on ALL other fields to prevent multiple highlights.
                        match field_id.as_str() {
                            "e2e_passphrase_input" | "wizard_passphrase_input" => {
                                self.panel_app.user_settings_state.e2e_passphrase_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.e2e_passphrase_focused = true;
                                self.panel_app.user_settings_state.new_profile_name_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.recovery_key_display_focused = false;
                                self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                            }
                            "wizard_profile_name_input" | "profile_mgr:name_input" => {
                                self.panel_app.user_settings_state.new_profile_name_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.new_profile_name_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.new_profile_name_focused = true;
                                self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.recovery_key_display_focused = false;
                                self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                            }
                            "profile_mgr:recovery_key_input" => {
                                self.panel_app.user_settings_state.recovery_key_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.recovery_key_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.recovery_key_focused = true;
                                self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_profile_name_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                            }
                            "profile_mgr:new_passphrase_input" => {
                                self.panel_app.user_settings_state.new_passphrase_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.new_passphrase_focused = true;
                                self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                            }
                            "profile_mgr:confirm_passphrase_input"
                            | "wizard_confirm_passphrase_input"
                            | "profile_mgr:create_confirm_passphrase_input" => {
                                self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                                self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_profile_name_focused = false;
                                self.panel_app.user_settings_state.recovery_key_display_focused = false;
                                self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                            }
                            "profile_mgr:recovery_key_display" => {
                                self.panel_app.user_settings_state.recovery_key_display_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.recovery_key_display_focused = true;
                                self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_profile_name_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                            }
                            _ => {}
                        }
                        self.panel_app.user_settings_state.profile_mgr_text_select_dragging = Some(field_id.clone());
                        eprintln!("[ChartApp] profile_mgr text select drag started: {} at char {}", field_id, drag_cursor);
                        return false;
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
                    return false;
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
                    return false;
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
                        return false;
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
                        return false;
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
                        return false;
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
                        return false;
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
                    return false;
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
                        return false;
                    }
                }
            }
        }

        // Watchlist modal item row drag — begin drag-to-reorder.
        if self.watchlist_modal.is_open() {
            if let Some(ref wl) = self.last_watchlist_modal_result {
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
                    return false;
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
                    return false;
                }
            }
        }

        // === Slider drag start — must come BEFORE the modal guard ===
        // Scrollbar handle drags are now handled via InputCoordinator registration.
        // Track clicks remain manual (need geometry from frame_result).
        if let Some(result) = &self.frame_result {
            // Chart settings slider drag start
            if self.panel_app.chart_settings_state.is_open {
                if let Some(ref cs) = result.chart_settings {
                    // Track click — click on empty scrollbar track to jump position
                    // Handle drag via InputCoordinator (chart_settings:scrollbar_handle).
                    if let Some(ref track_rect) = cs.scrollbar_track_rect {
                        let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                            && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                        if hit {
                            self.panel_app.chart_settings_state.scroll.handle_track_click(
                                y,
                                track_rect.y,
                                track_rect.height,
                                cs.total_content_height,
                                cs.viewport_height,
                            );
                            return false;
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
                                return false;
                            }
                        }
                    }
                }
            }
            // Tags & Tabs scrollbar track click (handle drag via InputCoordinator)
            if self.panel_app.tags_tabs_state.is_open {
                if let Some(ref tt) = result.tags_tabs {
                    use crate::scroll_dispatch::{ScrollableInfo, try_handle_track_click};
                    let info = ScrollableInfo {
                        handle_rect:     tt.scrollbar_handle_rect,
                        track_rect:      tt.scrollbar_track_rect,
                        content_height:  tt.scroll_content_height,
                        viewport_height: tt.scroll_viewport_height,
                        viewport_rect:   tt.scroll_viewport_rect,
                    };
                    let scroll_state = tags_tabs_active_scroll(&mut self.panel_app.tags_tabs_state);
                    if try_handle_track_click(x, y, &mut [(&info, scroll_state)]) {
                        return false;
                    }
                }
            }
            // User settings scrollbar track click (handle drag via InputCoordinator)
            if self.panel_app.user_settings_state.is_open {
                if let Some(ref us) = result.user_settings {
                    use crate::scroll_dispatch::{ScrollableInfo, try_handle_track_click};
                    use zengeld_chart::ui::modal_settings::UserSettingsTab;

                    let info = ScrollableInfo {
                        handle_rect: us.scrollbar_handle_rect,
                        track_rect: us.scrollbar_track_rect,
                        content_height: us.scroll_content_height,
                        viewport_height: us.scroll_viewport_height,
                        viewport_rect: us.scroll_viewport_rect,
                    };

                    let scroll_state = match self.panel_app.user_settings_state.active_tab {
                        UserSettingsTab::General => &mut self.panel_app.user_settings_state.general_tab_scroll,
                        UserSettingsTab::Sync => &mut self.panel_app.user_settings_state.sync_tab_scroll,
                        UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_keys_scroll,
                        UserSettingsTab::Performance => &mut self.panel_app.user_settings_state.performance_tab_scroll,
                    };

                    // Track click (jump to position)
                    if try_handle_track_click(x, y, &mut [(&info, scroll_state)]) {
                        return false;
                    }
                }
            }
            // User settings DATA & CACHE slider drag start
            if self.panel_app.user_settings_state.is_open {
                use zengeld_chart::ui::modal_settings::UserSettingsTab;
                if self.panel_app.user_settings_state.active_tab == UserSettingsTab::Performance {
                    if let Some(ref us) = result.user_settings {
                        for track in &us.slider_tracks {
                            if let Some((_, item_rect)) = us.content_items.iter().find(|(id, _)| id == &track.field_id) {
                                let hit = x >= item_rect.x - 2.0 && x <= item_rect.x + item_rect.width + 2.0
                                    && y >= item_rect.y && y <= item_rect.y + item_rect.height;
                                if hit {
                                    let field_id = track.field_id.clone();
                                    let track_x = track.track_x;
                                    let track_width = track.track_width;
                                    let min_val = track.min_val;
                                    let max_val = track.max_val;
                                    self.panel_app.user_settings_state.start_data_slider_drag(
                                        &field_id, track_x, track_width, min_val, max_val,
                                    );
                                    self.panel_app.user_settings_state.update_data_slider_drag(x);
                                    return false;
                                }
                            }
                        }
                    }
                }
            }
            // Profile list scrollbar track click (handle drag via InputCoordinator)
            if self.panel_app.user_settings_state.show_profile_manager {
                if let Some(ref us) = result.user_settings {
                    // Track click — click on empty scrollbar track to jump position
                    if let Some(ref track_rect) = us.profile_list_track_rect {
                        let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                            && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                        if hit {
                            self.panel_app.user_settings_state.profile_list_scroll.handle_track_click(
                                y,
                                track_rect.y,
                                track_rect.height,
                                us.profile_list_total_content_h,
                                us.profile_list_viewport_rect.height,
                            );
                            return false;
                        }
                    }
                }
            }
            // Indicator settings scrollbar / slider drag start (skip when color picker is open above)
            if self.panel_app.indicator_settings_state.is_open()
                && !self.panel_app.indicator_settings_state.is_color_picker_open()
            {
                if let Some(ref is) = result.indicator_settings {
                    // Track click — click on empty scrollbar track to jump position
                    // Handle drag via InputCoordinator (ind_settings:scrollbar_handle).
                    if let Some(ref track_rect) = is.scrollbar_track_rect {
                        let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                            && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                        if hit {
                            self.panel_app.indicator_settings_state.scroll.handle_track_click(
                                y,
                                track_rect.y,
                                track_rect.height,
                                is.total_content_height,
                                is.viewport_height,
                            );
                            return false;
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
                                return false;
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
                        return false;
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
                        return false;
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
                        return false;
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
                            return false;
                        }
                    }
                }
            }
        }

        // Alert settings scrollbar track click (handle drag via InputCoordinator)
        if self.panel_app.alert_settings_state.is_open() {
            if let Some(ref result) = &self.frame_result {
                if let Some(ref asr) = result.alert_settings {
                    if let Some(ref track_rect) = asr.scrollbar_track_rect {
                        let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                            && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                        if hit {
                            if let Some(ref vp) = asr.list_viewport_rect {
                                self.panel_app.alert_settings_state.list_scroll.handle_track_click(
                                    y,
                                    track_rect.y,
                                    track_rect.height,
                                    asr.list_total_content_height,
                                    vp.height,
                                );
                            }
                            return false;
                        }
                    }
                }
            }
        }

        // Search / compare / indicator-search modal scrollbar track click (handle drag via InputCoordinator)
        if self.modal_state.is_open() {
            if let Some(ref smr) = self.search_modal_result {
                // Track click — click on empty scrollbar track to jump position
                if let Some(ref track_rect) = smr.scrollbar_track_rect {
                    let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                        && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                    if hit {
                        self.modal_state.scroll.handle_track_click(
                            y,
                            track_rect.y,
                            track_rect.height,
                            smr.total_content_height,
                            smr.viewport_height,
                        );
                        return false;
                    }
                }
            }
        }

        // Watchlist modal scrollbar track click (handle drag via InputCoordinator)
        if self.watchlist_modal.is_open() {
            if let Some(ref wmr) = self.last_watchlist_modal_result {
                if let Some(ref track_rect) = wmr.scrollbar_track_rect {
                    let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                        && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                    if hit {
                        self.watchlist_modal.scroll.handle_track_click(
                            y, track_rect.y, track_rect.height,
                            wmr.total_content_height,
                            wmr.list_viewport_rect.height,
                        );
                        return false;
                    }
                }
            }
        }

        // Chart browser scrollbar track click (handle drag via InputCoordinator)
        if self.panel_app.chart_browser.is_open {
            if let Some(ref frame_r) = self.frame_result {
                if let Some(ref cbr) = frame_r.chart_browser {
                    if let Some(ref track_rect) = cbr.scrollbar_track_rect {
                        let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                            && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                        if hit {
                            self.panel_app.chart_browser.scroll.handle_track_click(
                                y,
                                track_rect.y,
                                track_rect.height,
                                cbr.total_content_height,
                                cbr.list_viewport_rect.height,
                            );
                            return false;
                        }
                    }
                }
            }
        }

        // Block drags when a modal is open (and we didn't start a modal drag above).
        if self.input_coordinator.borrow_mut().is_blocked_by_modal(x, y) {
            return false;
        }

        // Split panel: route drag to the correct leaf FIRST, before any drawing
        // tool check, so the click lands on the correct leaf.
        // Skip when over UI (dropdown/toolbar) to avoid activating wrong leaf.
        if self.panel_app.panel_grid.is_split() && !self.ui_drag_active {
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
                    return false;
                }
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => {
                    self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    // Fall through to chart engine drag handling.
                }
                ChartInputTarget::None => return false,
            }
        }

        // Click-based drawing tool on canvas: mouse-press = click immediately.
        // on_click from runner (mouse-release) is ignored via guard in on_click().
        // AFTER split routing so handle_canvas_click operates on the correct leaf.
        let _has_ct = self.has_click_drawing_tool();
        if !self.ui_drag_active && _has_ct {
            self.handle_canvas_click(x, y);
            return false;
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
        let overlay_results_ds = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(&extended)
            .with_overlays(&overlay_results_ds);

        // Delegate freehand-start and primitive/control-point hit-test to ChartPanelGrid.
        // Freehand: drawing_manager.start_freehand() runs inside handle_drag_start.
        // Background: viewport_before_drag already captured above.
        use zengeld_chart::state::ChartDragStartHit;
        let drag_start_hit = self.panel_app.panel_grid.handle_drag_start(x, y, &extended);

        let (drag_start_mode, extra_actions) = match drag_start_hit {
            ChartDragStartHit::FreehandStarted => {
                return false;
            }
            ChartDragStartHit::ControlPoint { primitive_id: id, control_point: cp_type } => {
                let mode = DragMode::ControlPoint { primitive_id: id, point_index: 0 };
                let action = ChartOutputAction::StartControlPointDrag {
                    primitive_id: id,
                    control_point: cp_type,
                    bar: x,
                    price: y,
                };
                (mode, vec![action])
            }
            ChartDragStartHit::Primitive { primitive_id: id } => {
                let mode = DragMode::Primitive { id };
                let action = ChartOutputAction::StartPrimitiveDrag {
                    id,
                    bar: x,
                    price: y,
                };
                (mode, vec![action])
            }
            ChartDragStartHit::Background | ChartDragStartHit::Miss => {
                (DragMode::None, Vec::new())
            }
        };

        let mut actions = self.input_handler.process_action(
            ChartInputAction::DragStart { mode: drag_start_mode, x, y },
            &hit_tester,
        );
        // Append Start*Drag actions so process_output_actions initialises
        // drawing_manager.start_drag() with the correct coordinates.
        actions.extend(extra_actions);
        self.process_output_actions(actions);
        false
    }

    /// Handle drag move to `(x, y)` with deltas `(dx, dy)`.
    pub fn on_drag_move(&mut self, x: f64, y: f64, dx: f64, dy: f64) {
        // ── Agent-panel separator resize drag ────────────────────────────────
        if let Some((sep_idx, start_pos, _total_size)) = self.agent_sep_drag {
            // Read orientation and area with a scoped borrow, then drop before mutable call.
            let (is_vertical, content_width, content_height) = {
                let d = self.sidebar_state.agent_docking.inner();
                let area = d.layout_area();
                let vert = d.separators().get(sep_idx)
                    .map(|s| s.orientation == uzor::panels::SeparatorOrientation::Vertical)
                    .unwrap_or(true);
                (vert, area.width, area.height)
            };
            let cur_pos = if is_vertical { x } else { y };
            let delta = (cur_pos - start_pos) as f32;
            // Update start_pos so subsequent moves are incremental, not cumulative.
            self.agent_sep_drag = Some((sep_idx, cur_pos, content_width));
            self.sidebar_state.agent_docking.inner_mut().drag_separator(sep_idx, delta, content_width, content_height);
            self.sidebar_data_dirty = true;
            return;
        }

        // ── DOM drag scroll ──────────────────────────────────────────────────
        if let Some((_, _, pid, ref mut last_y, row_h)) = self.slot_dom_drag {
            let delta_y = y - *last_y;
            if delta_y.abs() > 1.0 {
                if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                    // Drag down = content follows mouse down = higher prices at center
                    let ticks_moved = delta_y / row_h;
                    state.center_price += ticks_moved * state.tick_size;
                    state.auto_center = false;
                    self.sidebar_data_dirty = true;
                }
                *last_y = y;
            }
            return;
        }

        // ── Coordinator-routed panel drag (L2Tape, Footprint, BigTrades, LiquidityHeatmap, VolumeProfile) ─────
        if let Some((ref item, ref local_id, ref mut last_x, ref mut last_y)) = self.active_drag_panel {
            let dx = x - *last_x;
            let dy = y - *last_y;
            if dx.abs() > 0.5 || dy.abs() > 0.5 {
                let local_id = local_id.clone();
                let item = item.clone();
                if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                    panel.handle_drag_move(&local_id, dx, dy);
                    self.sidebar_data_dirty = true;
                }
                if let Some((_, _, ref mut lx, ref mut ly)) = self.active_drag_panel {
                    *lx = x;
                    *ly = y;
                }
            }
            return;
        }

        // ── TradeTape drag-to-scroll ─────────────────────────────────────────
        if let Some((pid, ref mut last_x, ref mut last_y)) = self.slot_tradetape_drag {
            let dx = x - *last_x;
            let dy = y - *last_y;
            if dx.abs() > 0.5 || dy.abs() > 0.5 {
                if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                    state.handle_drag(dx, dy);
                    self.sidebar_data_dirty = true;
                }
                *last_x = x;
                *last_y = y;
            }
            return;
        }

        // ── Free-slot separator resize drag ─────────────────────────────────
        if let Some((slot_idx, sep_idx, start_pos, _total_size)) = self.slot_sep_drag {
            let (is_vertical, content_width, content_height) = {
                let d = self.sidebar_state.slot_dockings[slot_idx].inner();
                let area = d.layout_area();
                let vert = d.separators().get(sep_idx)
                    .map(|s| s.orientation == uzor::panels::SeparatorOrientation::Vertical)
                    .unwrap_or(true);
                (vert, area.width, area.height)
            };
            let cur_pos = if is_vertical { x } else { y };
            let delta = (cur_pos - start_pos) as f32;
            self.slot_sep_drag = Some((slot_idx, sep_idx, cur_pos, content_width));
            self.sidebar_state.slot_dockings[slot_idx].inner_mut().drag_separator(sep_idx, delta, content_width, content_height);
            self.sidebar_data_dirty = true;
            return;
        }

        // ── PTY host-side selection drag extension ────────────────────────
        if self.agent_pty_drag_active {
            // Clamp to the PTY rect for graceful out-of-bounds behavior.
            let (rx, ry, rw, rh) = self
                .sidebar_state
                .agent_terminal_rect
                .map(|(a, b, c, d)| (a as f64, b as f64, c as f64, d as f64))
                .unwrap_or((0.0, 0.0, 0.0, 0.0));
            let cx = x.clamp(rx, rx + rw - 1.0);
            let cy = y.clamp(ry, ry + rh - 1.0);
            if let Some((row, col)) = self.pty_cell_at(cx, cy) {
                if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                    if let Some(sel) = self.sidebar_state.agent_pty_selections.get_mut(&leaf_id) {
                        sel.end_row = row;
                        sel.end_col = col;
                        self.sidebar_data_dirty = true;
                    }
                }
            }
            return;
        }

        // ── Chat host-side selection drag extension ───────────────────────
        if self.sidebar_state.agent_chat_drag_active {
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                let hit = self.last_sidebar_result.as_ref()
                    .and_then(|r| {
                        r.agent_chat_line_rects.iter()
                            .find(|e| e.4 == leaf_id && y >= e.2 && y < e.3)
                            .map(|e| (e.0, e.1, e.5.clone(), e.6))
                    });
                if let Some((msg_idx, line_idx, line_text, text_x)) = hit {
                    let char_idx = chat_char_idx_from_x(&line_text, x, text_x);
                    if let Some(sel) = self.sidebar_state.agent_chat_selections.get_mut(&leaf_id) {
                        sel.end_msg = msg_idx;
                        sel.end_line = line_idx;
                        sel.end_char = char_idx;
                        self.sidebar_data_dirty = true;
                    }
                }
            }
            return;
        }

        // Forward to text-field store for text-selection drag (e.g. HexColor field).
        self.input_coordinator.borrow_mut().text_fields_mut().on_drag_move(x);

        if self.drag_dismissed_popup {
            return;
        }

        // If the sidebar separator drag is active, resize the sidebar.
        if self.sidebar_separator_drag_active {
            // Sidebar width = distance from mouse X to the left edge of the right toolbar.
            // Preventive gate: if there is not enough horizontal room for the minimum
            // sidebar AND the minimum chart area simultaneously, auto-close the sidebar
            // rather than allowing a degenerate layout that leads to negative rects.
            let min_chart_w = self.panel_app.panel_grid.min_sidebar_chart_width() as f64;
            let chart_if_min_sidebar =
                self.right_toolbar_left_x - sidebar_content::state::MIN_SIDEBAR_WIDTH;
            if chart_if_min_sidebar < min_chart_w {
                // Not enough room — close the sidebar entirely.
                self.sidebar_state
                    .set_right_panel(sidebar_content::state::RightSidebarPanel::None);
                return;
            }
            // Enforce [MIN_SIDEBAR_WIDTH, right_toolbar_left_x - min_chart_w] clamp.
            let max_w = self.right_toolbar_left_x - min_chart_w;
            let new_width = (self.right_toolbar_left_x - x)
                .clamp(sidebar_content::state::MIN_SIDEBAR_WIDTH, max_w);
            self.sidebar_state.set_right_width(new_width);
            return;
        }

        // Watchlist column-separator drag: update absolute separator offset (clip curtain model).
        if let Some((sep_idx, start_x, sep_offset_at_start)) = self.sidebar_state.watchlist_sep_drag {
            // Skip drag when columns are aligned (equal-width mode).
            let align = self.sidebar_state.watchlist_manager
                .active_list()
                .map(|l| l.column_config.align_columns)
                .unwrap_or(true);
            if align {
                return;
            }

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
                    if c.show_exchange     { n += 1; }
                    if c.show_account_type { n += 1; }
                    if c.show_last_price   { n += 1; }
                    if c.show_change_pct   { n += 1; }
                    if c.show_change_abs   { n += 1; }
                    if c.show_high_low     { n += 2; }
                    if c.show_volume       { n += 1; }
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
                // Do NOT push an action or set watchlists_dirty during drag:
                // that would cause main.rs to clone app_state back over sidebar_state
                // next frame (1-frame stutter). Persist happens in on_drag_end.
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
                let scroll = self.watchlist_modal.scroll.offset;
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

        // Signal group scrollbar drag move.
        // Find which group is dragging first (avoids borrow conflict with self).
        let dragging_group_id = self
            .sidebar_state
            .signal_group_scroll
            .iter()
            .find(|(_, s)| s.is_dragging)
            .map(|(&id, _)| id);
        if let Some(group_id) = dragging_group_id {
            // Look up the track geometry from the last sidebar result.
            let track_info = self.last_sidebar_result.as_ref().and_then(|sr| {
                sr.signal_group_scrollbar_rects
                    .iter()
                    .find(|r| r.0 == group_id)
                    .map(|r| (r.2.height, r.3, r.4)) // (track_height, content_h, viewport_h)
            });
            if let Some((track_h, content_h, viewport_h)) = track_info {
                if let Some(scroll) = self.sidebar_state.signal_group_scroll.get_mut(&group_id) {
                    scroll.handle_drag(y, track_h, content_h, viewport_h);
                }
                return;
            }
        }

        // Sidebar scrollbar handle drag move
        if self.sidebar_state.current_right_scroll_mut().is_dragging {
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                if let Some(ref track_rect) = sidebar_result.scrollbar_track_rect {
                    let content_h = sidebar_result.content_height;
                    let viewport_h = sidebar_result.content_rect.height;
                    self.sidebar_state.current_right_scroll_mut().handle_drag(
                        y, track_rect.height, content_h, viewport_h,
                    );
                    return;
                }
            }
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
            let delta = match sep_drag.orientation {
                SeparatorOrientation::Horizontal => (y - sep_drag.start_y) as f32,
                SeparatorOrientation::Vertical => (x - sep_drag.start_x) as f32,
            };
            // Hard-coded per-leaf minimum width: no leaf can ever shrink below
            // LEAF_MIN_WIDTH regardless of which separator is being dragged.
            {
                let leaf_ids: Vec<_> = self.panel_app.panel_grid
                    .iter_windows()
                    .map(|(leaf_id, _w)| leaf_id)
                    .collect();
                for leaf_id in leaf_ids {
                    self.panel_app.panel_grid.set_leaf_min_width(
                        leaf_id,
                        zengeld_chart::state::panel_grid::ChartPanelGrid::LEAF_MIN_WIDTH,
                    );
                }
            }
            self.panel_app.panel_grid.apply_separator_drag(
                sep_drag.separator_idx,
                delta,
                self.content_rect.width as f32,
                self.content_rect.height as f32,
            );
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
        // Profile manager text select drag move.
        if self.panel_app.user_settings_state.profile_mgr_text_select_dragging.is_some() {
            let field_id = self.panel_app.user_settings_state.profile_mgr_text_select_dragging.clone().unwrap_or_default();
            let char_positions: Vec<f64> = self.frame_result.as_ref()
                .and_then(|r| r.user_settings.as_ref())
                .and_then(|us| us.input_char_positions.iter().find(|(k, _)| k == &field_id))
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            if !char_positions.is_empty() {
                let new_cursor = zengeld_chart::ui::widgets::cursor_from_char_positions(&char_positions, x);
                match field_id.as_str() {
                    "e2e_passphrase_input" | "wizard_passphrase_input" => {
                        self.panel_app.user_settings_state.e2e_passphrase_editing.cursor = new_cursor;
                    }
                    "wizard_profile_name_input" | "profile_mgr:name_input" => {
                        self.panel_app.user_settings_state.new_profile_name_editing.cursor = new_cursor;
                    }
                    "profile_mgr:recovery_key_input" => {
                        self.panel_app.user_settings_state.recovery_key_editing.cursor = new_cursor;
                    }
                    "profile_mgr:new_passphrase_input" => {
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = new_cursor;
                    }
                    "profile_mgr:confirm_passphrase_input"
                    | "wizard_confirm_passphrase_input"
                    | "profile_mgr:create_confirm_passphrase_input" => {
                        self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = new_cursor;
                    }
                    "profile_mgr:recovery_key_display" => {
                        self.panel_app.user_settings_state.recovery_key_display_editing.cursor = new_cursor;
                    }
                    _ => {}
                }
                // selection_start stays as the anchor set during drag_start
            }
            return;
        }
        if self.panel_app.preset_name_input.text_select_dragging {
            // Update text selection cursor from mouse X position.
            if let Some(pni) = self.frame_result.as_ref().and_then(|r| r.preset_name_input.as_ref()) {
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
            if let Some(br) = self.frame_result.as_ref().and_then(|r| r.chart_browser.as_ref()) {
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
        // Chart browser scrollbar drag move
        if self.panel_app.chart_browser.scroll.is_dragging {
            if let Some(ref frame_r) = self.frame_result {
                if let Some(ref cbr) = frame_r.chart_browser {
                    if let Some(ref track_rect) = cbr.scrollbar_track_rect {
                        self.panel_app.chart_browser.scroll.handle_drag(
                            y,
                            track_rect.height,
                            cbr.total_content_height,
                            cbr.list_viewport_rect.height,
                        );
                    }
                }
            }
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
        // Alert settings scrollbar drag move
        if self.panel_app.alert_settings_state.list_scroll.is_dragging {
            if let Some(ref result) = self.frame_result {
                if let Some(ref asr) = result.alert_settings {
                    if let Some(ref track_rect) = asr.scrollbar_track_rect {
                        if let Some(ref vp) = asr.list_viewport_rect {
                            self.panel_app.alert_settings_state.list_scroll.handle_drag(
                                y,
                                track_rect.height,
                                asr.list_total_content_height,
                                vp.height,
                            );
                        }
                    }
                }
            }
            return;
        }
        // Tags & Tabs scrollbar drag move
        {
            use crate::scroll_dispatch::{ScrollableInfo, try_handle_scrollbar_drag};
            if let Some(ref result) = self.frame_result {
                if let Some(ref tt) = result.tags_tabs {
                    let info = ScrollableInfo {
                        handle_rect:     tt.scrollbar_handle_rect,
                        track_rect:      tt.scrollbar_track_rect,
                        content_height:  tt.scroll_content_height,
                        viewport_height: tt.scroll_viewport_height,
                        viewport_rect:   tt.scroll_viewport_rect,
                    };
                    let scroll_state = tags_tabs_active_scroll(&mut self.panel_app.tags_tabs_state);
                    if try_handle_scrollbar_drag(y, &mut [(&info, scroll_state)]) {
                        return;
                    }
                }
            }
        }
        // User settings scrollbar drag move
        {
            use crate::scroll_dispatch::{ScrollableInfo, try_handle_scrollbar_drag};
            use zengeld_chart::ui::modal_settings::UserSettingsTab;

            if let Some(ref result) = self.frame_result {
                if let Some(ref us) = result.user_settings {
                    let info = ScrollableInfo {
                        handle_rect: us.scrollbar_handle_rect,
                        track_rect: us.scrollbar_track_rect,
                        content_height: us.scroll_content_height,
                        viewport_height: us.scroll_viewport_height,
                        viewport_rect: us.scroll_viewport_rect,
                    };

                    let scroll_state = match self.panel_app.user_settings_state.active_tab {
                        UserSettingsTab::General => &mut self.panel_app.user_settings_state.general_tab_scroll,
                        UserSettingsTab::Sync => &mut self.panel_app.user_settings_state.sync_tab_scroll,
                        UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_keys_scroll,
                        UserSettingsTab::Performance => &mut self.panel_app.user_settings_state.performance_tab_scroll,
                    };

                    if try_handle_scrollbar_drag(y, &mut [(&info, scroll_state)]) {
                        return;
                    }
                }
            }
        }

        // Profile list scrollbar drag move
        if self.panel_app.user_settings_state.profile_list_scroll.is_dragging {
            if let Some(ref result) = self.frame_result {
                if let Some(ref us) = result.user_settings {
                    if let Some(ref track_rect) = us.profile_list_track_rect {
                        let content_h = us.profile_list_total_content_h;
                        let viewport_h = us.profile_list_viewport_rect.height;
                        self.panel_app.user_settings_state.profile_list_scroll.handle_drag(
                            y, track_rect.height, content_h, viewport_h,
                        );
                    }
                }
            }
            return;
        }

        // Agent chat / PTY scrollbar drag move — route to whichever leaf is dragging.
        {
            use crate::scroll_dispatch::{ScrollableInfo, try_handle_scrollbar_drag};
            let leaf_scroll_infos: Vec<(uzor::panels::LeafId, bool, ScrollableInfo)> = {
                if let Some(ref sidebar_result) = self.last_sidebar_result {
                    sidebar_result.agent_leaf_scrollbar_rects.iter()
                        .filter_map(|(&lid, &(handle_rect, track_rect))| {
                            let (content_h, vp_h) = sidebar_result.agent_leaf_content_heights
                                .get(&lid).copied().unwrap_or((0.0, 0.0));
                            if vp_h <= 0.0 { return None; }
                            let is_chat = self.sidebar_state.agent_leaves.get(&lid)
                                .map(|d| d.mode == gate4agent::InstanceMode::Chat)
                                .unwrap_or(true);
                            Some((lid, is_chat, ScrollableInfo {
                                handle_rect,
                                track_rect,
                                content_height: content_h,
                                viewport_height: vp_h,
                                viewport_rect: None,
                            }))
                        })
                        .collect()
                } else {
                    vec![]
                }
            };

            for (leaf_id, is_chat, info) in &leaf_scroll_infos {
                if *is_chat {
                    let scroll = self.sidebar_state.agent_chat_scrolls.entry(*leaf_id).or_default();
                    if try_handle_scrollbar_drag(y, &mut [(&*info, scroll)]) {
                        return;
                    }
                } else {
                    let scroll = self.sidebar_state.agent_pty_scrolls.entry(*leaf_id).or_default();
                    if try_handle_scrollbar_drag(y, &mut [(&*info, scroll)]) {
                        return;
                    }
                }
            }
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
        // === User settings DATA & CACHE slider drag move ===
        if self.panel_app.user_settings_state.is_data_slider_dragging() {
            self.panel_app.user_settings_state.update_data_slider_drag(x);
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

        // === Watchlist modal scrollbar drag move ===
        if self.watchlist_modal.scroll.is_dragging {
            if let Some(ref wmr) = self.last_watchlist_modal_result {
                if let Some(ref track_rect) = wmr.scrollbar_track_rect {
                    self.watchlist_modal.scroll.handle_drag(
                        y,
                        track_rect.height,
                        wmr.total_content_height,
                        wmr.list_viewport_rect.height,
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
        {
            let extended = self.build_extended_layout();
            let chart_rect = extended.main_chart.chart;
            if self.panel_app.panel_grid.extend_freehand(x, y, chart_rect) {
                return;
            }
        }

        let drag_mode = self.input_handler.state.drag_mode;

        // Handle pane separator drag: resize the sub-pane above and below the
        // separator by the vertical delta.  This updates height_ratio on the
        // sub-pane so that compute_from_chart_panel picks it up next frame.
        if let zengeld_chart::engine::input::DragMode::PaneSeparator { instance_id } = drag_mode {
            // Pre-compute layout data that requires &self methods before taking &mut self.
            let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
            if let Some(leaf) = active_leaf {
                if let Some(leaf_rect) = self.get_leaf_absolute_rect(leaf) {
                    let available_h = leaf_rect.height.max(1.0);
                    let rendered_heights: std::collections::HashMap<u64, f64> =
                        match self.build_extended_layout_for_leaf(leaf, &leaf_rect) {
                            Some(ext) => ext.sub_panes.iter()
                                .map(|sp| (sp.instance_id, sp.content.height))
                                .collect(),
                            None => std::collections::HashMap::new(),
                        };
                    self.panel_app.panel_grid.drag_pane_separator(
                        instance_id,
                        dy,
                        available_h,
                        &rendered_heights,
                    );
                }
            }
            return;
        }

        let extended = self.build_extended_layout();
        let overlay_results_dm = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(&extended)
            .with_overlays(&overlay_results_dm);
        let actions = self.input_handler.process_action(
            ChartInputAction::DragMove { mode: drag_mode, x, y, delta_x: dx, delta_y: dy },
            &hit_tester,
        );
        self.process_output_actions(actions);

        // Update crosshair during drag via ChartPanelGrid.
        let extended2 = self.build_extended_layout();
        let drag_mode = self.input_handler.state.drag_mode;
        if let Some((timestamp, price, crosshair_visible, pane_index)) = self.panel_app.panel_grid
            .update_crosshair(x, y, drag_mode, false, &extended2)
        {
            let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
            if let Some(active_leaf) = active_leaf_opt {
                self.propagate_crosshair_to_sync_group(active_leaf, timestamp, price, crosshair_visible, pane_index);
            }
        }
    }

    /// Handle drag end at `(x, y)`.
    pub fn on_drag_end(&mut self, x: f64, y: f64) {
        self.ui_drag_active = false;

        // ── End agent-panel separator drag ───────────────────────────────────
        if self.agent_sep_drag.take().is_some() {
            self.sidebar_data_dirty = true;
            self.autosave_snapshot();
            return;
        }

        // ── End free-slot separator drag ─────────────────────────────────────
        if self.slot_sep_drag.take().is_some() {
            self.sidebar_data_dirty = true;
            self.autosave_snapshot();
            return;
        }

        // ── End DOM drag scroll ───────────────────────────────────────────────
        if self.slot_dom_drag.take().is_some() {
            self.sidebar_data_dirty = true;
            return;
        }

        // ── End coordinator-routed panel drag (L2Tape, Footprint, BigTrades, LiquidityHeatmap, VolumeProfile) ─
        if let Some((item, local_id, _, _)) = self.active_drag_panel.take() {
            if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                panel.handle_drag_end(&local_id);
            }
            self.sidebar_data_dirty = true;
            return;
        }

        // ── End TradeTape drag-to-scroll ─────────────────────────────────────
        if self.slot_tradetape_drag.take().is_some() {
            self.sidebar_data_dirty = true;
            return;
        }

        // ── End host-side PTY selection drag ────────────────────────────────
        if self.agent_pty_drag_active {
            self.agent_pty_drag_active = false;
            // If start==end the selection is empty — clear it so overlay vanishes.
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                let is_empty = self.sidebar_state.agent_pty_selections.get(&leaf_id)
                    .map(|s| s.is_empty())
                    .unwrap_or(true);
                if is_empty {
                    self.sidebar_state.agent_pty_selections.remove(&leaf_id);
                }
            }
            self.sidebar_data_dirty = true;
            return;
        }

        // ── End host-side chat selection drag ────────────────────────────────
        if self.sidebar_state.agent_chat_drag_active {
            self.sidebar_state.agent_chat_drag_active = false;
            // If start==end the selection is empty — clear it so overlay vanishes.
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                let is_empty = self.sidebar_state.agent_chat_selections.get(&leaf_id)
                    .map(|s| s.is_empty())
                    .unwrap_or(true);
                if is_empty {
                    self.sidebar_state.agent_chat_selections.remove(&leaf_id);
                }
            }
            self.sidebar_data_dirty = true;
            return;
        }

        // Notify text-field store that drag selection has ended.
        self.input_coordinator.borrow_mut().text_fields_mut().on_drag_end();

        if self.drag_dismissed_popup {
            self.drag_dismissed_popup = false;
            return;
        }

        // End pane separator drag: persist the new height ratios.
        if let zengeld_chart::engine::input::DragMode::PaneSeparator { .. } = self.input_handler.state.drag_mode {
            self.input_handler.state.drag_mode = zengeld_chart::engine::input::DragMode::None;
            self.persist_profile();
            eprintln!("[ChartApp] PaneSeparator drag ended — sub-pane heights persisted");
            return;
        }

        // End sidebar separator drag.
        if self.sidebar_separator_drag_active {
            self.sidebar_separator_drag_active = false;
            self.persist_profile();
            eprintln!("[ChartApp] Sidebar width: {:.0}", self.sidebar_state.right_width());
            return;
        }

        // End watchlist column-separator drag: push the final offsets once to sync
        // app_state for persistence, then clear the drag handle.
        if self.sidebar_state.watchlist_sep_drag.is_some() {
            if let Some(list) = self.sidebar_state.watchlist_manager.active_list() {
                if let Some(offsets) = &list.column_config.separator_offsets {
                    self.watchlist_actions.push(crate::WatchlistAction::SetSeparatorOffsets { offsets: offsets.clone() });
                }
            }
            self.watchlists_dirty = true;
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

        // End signal group scrollbar drag.
        for (_, scroll) in self.sidebar_state.signal_group_scroll.iter_mut() {
            if scroll.is_dragging {
                scroll.end_drag();
                break;
            }
        }

        // End sidebar scrollbar handle drag.
        if self.sidebar_state.current_right_scroll_mut().is_dragging {
            self.sidebar_state.current_right_scroll_mut().end_drag();
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
        // Profile manager text select drag end.
        if self.panel_app.user_settings_state.profile_mgr_text_select_dragging.is_some() {
            let field_id = self.panel_app.user_settings_state.profile_mgr_text_select_dragging.take().unwrap_or_default();
            // Finalize: if anchor == cursor (plain click jitter), clear the selection.
            let (anchor, cursor) = match field_id.as_str() {
                "e2e_passphrase_input" | "wizard_passphrase_input" => (
                    self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start,
                    self.panel_app.user_settings_state.e2e_passphrase_editing.cursor,
                ),
                "wizard_profile_name_input" | "profile_mgr:name_input" => (
                    self.panel_app.user_settings_state.new_profile_name_editing.selection_start,
                    self.panel_app.user_settings_state.new_profile_name_editing.cursor,
                ),
                "profile_mgr:recovery_key_input" => (
                    self.panel_app.user_settings_state.recovery_key_editing.selection_start,
                    self.panel_app.user_settings_state.recovery_key_editing.cursor,
                ),
                "profile_mgr:new_passphrase_input" => (
                    self.panel_app.user_settings_state.new_passphrase_editing.selection_start,
                    self.panel_app.user_settings_state.new_passphrase_editing.cursor,
                ),
                "profile_mgr:confirm_passphrase_input"
                | "wizard_confirm_passphrase_input"
                | "profile_mgr:create_confirm_passphrase_input" => (
                    self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start,
                    self.panel_app.user_settings_state.confirm_passphrase_editing.cursor,
                ),
                "profile_mgr:recovery_key_display" => (
                    self.panel_app.user_settings_state.recovery_key_display_editing.selection_start,
                    self.panel_app.user_settings_state.recovery_key_display_editing.cursor,
                ),
                _ => (None, 0),
            };
            if anchor == Some(cursor) {
                match field_id.as_str() {
                    "e2e_passphrase_input" | "wizard_passphrase_input" => {
                        self.panel_app.user_settings_state.e2e_passphrase_editing.selection_start = None;
                    }
                    "wizard_profile_name_input" | "profile_mgr:name_input" => {
                        self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                    }
                    "profile_mgr:recovery_key_input" => {
                        self.panel_app.user_settings_state.recovery_key_editing.selection_start = None;
                    }
                    "profile_mgr:new_passphrase_input" => {
                        self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                    }
                    "profile_mgr:confirm_passphrase_input"
                    | "wizard_confirm_passphrase_input"
                    | "profile_mgr:create_confirm_passphrase_input" => {
                        self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                    }
                    "profile_mgr:recovery_key_display" => {
                        self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                    }
                    _ => {}
                }
            }
            eprintln!("[ChartApp] profile_mgr text select drag ended: {}", field_id);
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
        if self.panel_app.chart_browser.scroll.is_dragging {
            self.panel_app.chart_browser.scroll.end_drag();
            eprintln!("[ChartApp] chart_browser scrollbar drag ended");
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
        if self.panel_app.alert_settings_state.list_scroll.is_dragging {
            self.panel_app.alert_settings_state.list_scroll.end_drag();
            eprintln!("[ChartApp] alert_settings scrollbar drag ended");
            return;
        }
        // Tags & Tabs scrollbar drag end
        {
            use crate::scroll_dispatch::try_end_scrollbar_drag;
            let ended = try_end_scrollbar_drag(&mut [
                &mut self.panel_app.tags_tabs_state.tabs_scroll,
                &mut self.panel_app.tags_tabs_state.tags_groups_scroll,
                &mut self.panel_app.tags_tabs_state.tags_details_scroll,
            ]);
            if ended {
                return;
            }
        }
        // User settings scrollbar drag end
        {
            use crate::scroll_dispatch::try_end_scrollbar_drag;
            let ended = try_end_scrollbar_drag(&mut [
                &mut self.panel_app.user_settings_state.general_tab_scroll,
                &mut self.panel_app.user_settings_state.sync_tab_scroll,
                &mut self.panel_app.user_settings_state.server_keys_scroll,
                &mut self.panel_app.user_settings_state.performance_tab_scroll,
            ]);
            if ended {
                return;
            }
        }
        // Profile list scrollbar drag end
        if self.panel_app.user_settings_state.profile_list_scroll.is_dragging {
            self.panel_app.user_settings_state.profile_list_scroll.end_drag();
            return;
        }
        // Agent chat / PTY scrollbar drag end — end drag on whichever leaf was dragging.
        {
            use crate::scroll_dispatch::try_end_scrollbar_drag;
            // Collect all leaf IDs from both chat and PTY scroll maps to avoid borrow conflicts.
            let all_leaf_ids: Vec<uzor::panels::LeafId> = self.sidebar_state.agent_chat_scrolls.keys()
                .chain(self.sidebar_state.agent_pty_scrolls.keys())
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            let mut any_ended = false;
            for leaf_id in all_leaf_ids {
                if let Some(s) = self.sidebar_state.agent_chat_scrolls.get_mut(&leaf_id) {
                    if try_end_scrollbar_drag(&mut [s]) { any_ended = true; }
                }
                if let Some(s) = self.sidebar_state.agent_pty_scrolls.get_mut(&leaf_id) {
                    if try_end_scrollbar_drag(&mut [s]) { any_ended = true; }
                }
            }
            if any_ended {
                return;
            }
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
        // === User settings DATA & CACHE slider drag end ===
        if self.panel_app.user_settings_state.is_data_slider_dragging() {
            if let Some((field_id, value)) = self.panel_app.user_settings_state.take_data_slider_value() {
                self.apply_data_cache_slider_value(&field_id, value);
            } else {
                self.panel_app.user_settings_state.end_data_slider_drag();
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

        // === Watchlist modal scrollbar drag end ===
        if self.watchlist_modal.scroll.is_dragging {
            self.watchlist_modal.scroll.end_drag();
            return;
        }

        // Freehand drawing (brush/highlighter) — complete stroke on drag end
        if let Some(freehand) = self.panel_app.panel_grid.complete_freehand() {
            // For grouped windows: move the completed freehand primitive to TagManager.
            // For standalone windows: record undo and propagate to color-tag peers.
            if !self.intercept_completed_primitive_to_group() {
                // Standalone path.
                self.push_undo_command(zengeld_chart::Command::CreatePrimitive {
                    index: freehand.index,
                    type_id: freehand.type_id,
                    points: freehand.points,
                    data: freehand.data,
                });
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
            self.sidebar_data_dirty = true;
            return;
        }

        let drag_mode = self.input_handler.state.drag_mode;
        let extended = self.build_extended_layout();
        let overlay_results_de = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(&extended)
            .with_overlays(&overlay_results_de);
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

        // --- Overlay tab hover state is derived from InputCoordinator below ---
        self.leaf_tab_hover = zengeld_chart::LeafTabHoverZone::None;
        self.leaf_tab_hovered_leaf = None;

        // --- Update panel overlay tab hover state (free-slot leaves) ---
        {
            self.sidebar_state.free_leaf_overlay_hover.clear();
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                for (_panel_id, leaf_id, zones) in &sidebar_result.panel_overlay_zones {
                    let [tx, ty, tw, th] = zones.tab_rect;
                    let hover_zone = if x >= tx && x < tx + tw && y >= ty && y < ty + th {
                        let [dx, dy, dw, dh] = zones.dots_rect;
                        let [cx, cy, cw, ch] = zones.color_tag_rect;
                        if x >= dx && x < dx + dw && y >= dy && y < dy + dh {
                            zengeld_chart::LeafTabHoverZone::GearMenu
                        } else if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                            zengeld_chart::LeafTabHoverZone::ColorTag
                        } else {
                            zengeld_chart::LeafTabHoverZone::Body
                        }
                    } else {
                        zengeld_chart::LeafTabHoverZone::None
                    };
                    // Find which slot contains this leaf_id.
                    let slot_idx_opt = self.sidebar_state.slot_dockings.iter().enumerate()
                        .find(|(_, sd)| sd.inner().tree().leaf(*leaf_id).is_some())
                        .map(|(i, _)| i);
                    if let Some(slot_idx) = slot_idx_opt {
                        self.sidebar_state.free_leaf_overlay_hover.insert((slot_idx, *leaf_id), hover_zone);
                    }
                }
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
        // Reset agent/slot hover highlights each frame; re-set below if still hovering.
        self.sidebar_state.hovered_agent_leaf = None;
        self.sidebar_state.hovered_free_leaf = None;
        // Clear DOM hovered_price each frame; re-set below if a row widget is still hovered.
        for dom_state in self.panels_store.dom.values_mut() {
            dom_state.hovered_price = None;
        }

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
                } else if item_id == "inline_dropdown:__bg__" || item_id == "__bg__" {
                    // background absorbers — no highlight
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
            } else if let Some(rest) = id_str.strip_prefix("leaf_tab:") {
                if let Some(colon) = rest.find(':') {
                    let leaf_id_str = &rest[..colon];
                    let sub_zone = &rest[colon + 1..];
                    if let Ok(lid) = leaf_id_str.parse::<u64>() {
                        self.leaf_tab_hovered_leaf = Some(zengeld_chart::LeafId(lid));
                        self.leaf_tab_hover = match sub_zone {
                            "gear" => zengeld_chart::LeafTabHoverZone::GearMenu,
                            "color_tag" => zengeld_chart::LeafTabHoverZone::ColorTag,
                            _ => zengeld_chart::LeafTabHoverZone::Body,
                        };
                    }
                }
            } else if id_str.starts_with("wl_modal:") || id_str.starts_with("wl_group_name:") {
                self.watchlist_modal.hovered_widget = Some(id_str.to_string());
            } else if let Some(rest) = id_str.strip_prefix("ind_search:") {
                // Indicator search sets view — "set_create" or "set:{id}"
                self.modal_state.hovered_item_id = Some(rest.to_string());
            } else if let Some(rest) = id_str.strip_prefix("modal_search:item:") {
                // Search overlay result items
                self.modal_state.hovered_item_id = Some(rest.to_string());
            } else if let Some(rest) = id_str.strip_prefix("agent:leaf:") {
                // Hover over an agent leaf — update visual hover highlight ONLY.
                // Keyboard focus (focused_agent_leaf) is set exclusively on click,
                // so that typing into one pane is not interrupted by mouse movement.
                if let Some(id_str2) = rest.strip_suffix(":focus") {
                    if let Ok(raw) = id_str2.parse::<u64>() {
                        let leaf_id = uzor::panels::LeafId(raw);
                        self.sidebar_state.hovered_agent_leaf = Some(leaf_id);
                        self.sidebar_data_dirty = true;
                    }
                }
            } else if let Some(slot_rest) = id_str.strip_prefix("slot:") {
                // Hover over a slot leaf — update visual hover highlight ONLY.
                // Pattern: "slot:{idx}:leaf:{leaf_id}:focus"
                if let Some((idx_str, leaf_rest)) = slot_rest.split_once(":leaf:") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":focus") {
                            if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                let leaf_id = uzor::panels::LeafId(raw);
                                self.sidebar_state.hovered_free_leaf = Some((idx, leaf_id));
                                self.sidebar_data_dirty = true;
                            }
                        } else if let Some((leaf_id_str, local_id)) = leaf_rest.split_once(':') {
                            // Pattern: "slot:{idx}:leaf:{leaf_id}:{local_id}"
                            // Dispatch panel hover for registered panel widgets (e.g. dom:row:{tick}).
                            if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                let leaf_id = uzor::panels::LeafId(raw);
                                let item_opt = if idx < self.sidebar_state.slot_dockings.len() {
                                    self.sidebar_state.slot_dockings[idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned())
                                } else {
                                    None
                                };
                                if let Some(item) = item_opt {
                                    let local_id = local_id.to_string();
                                    if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                        panel.handle_hover(&local_id);
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // No widget hovered — clear search overlay hover.
            if self.modal_state.current.is_search_overlay() {
                self.modal_state.hovered_item_id = None;
            }
        }

        // --- Update modal hover states via InputCoordinator (Phase 4.1) ---
        // Clear all modal hovered_item_id fields; re-set below if coordinator reports a hover.
        self.panel_app.primitive_settings_state.hovered_item_id = None;
        self.panel_app.chart_settings_state.hovered_item_id = None;
        self.panel_app.chart_settings_state.hovered_footer_button = None;
        self.panel_app.indicator_settings_state.hovered_item_id = None;
        self.panel_app.indicator_settings_state.hovered_footer_button = None;
        self.panel_app.user_settings_state.hovered_item_id = None;
        self.panel_app.chart_browser.hovered_preset_id = None;
        self.watchlist_modal.hovered_item_id = None;
        self.panel_app.overlay_settings_state.hovered_item_id = None;

        {
            let hovered = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
            if let Some(ref id_str) = hovered {
                let id = id_str.as_str();

                if let Some(local) = id.strip_prefix("prim_settings:item:") {
                    self.panel_app.primitive_settings_state.hovered_item_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("chart_settings:item:") {
                    // dropdown_option:{field}:{item_id} → extract item_id (3rd segment)
                    let actual = if let Some(rest) = local.strip_prefix("dropdown_option:") {
                        rest.split_once(':').map(|(_, r)| r.to_string()).unwrap_or_else(|| local.to_string())
                    } else {
                        local.to_string()
                    };
                    self.panel_app.chart_settings_state.hovered_item_id = Some(actual);
                } else if let Some(local) = id.strip_prefix("chart_settings:tab:") {
                    self.panel_app.chart_settings_state.hovered_item_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("chart_settings:footer:") {
                    self.panel_app.chart_settings_state.hovered_footer_button = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("ind_settings:item:") {
                    let actual = if let Some(r) = local.strip_prefix("input:") {
                        r.to_string()
                    } else if let Some(r) = local.strip_prefix("color:") {
                        r.to_string()
                    } else {
                        local.to_string()
                    };
                    self.panel_app.indicator_settings_state.hovered_item_id = Some(actual);
                } else if let Some(local) = id.strip_prefix("ind_settings:tab:") {
                    self.panel_app.indicator_settings_state.hovered_item_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("ind_settings:footer:") {
                    self.panel_app.indicator_settings_state.hovered_footer_button = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("user_settings:") {
                    self.panel_app.user_settings_state.hovered_item_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("chart_browser:item:") {
                    self.panel_app.chart_browser.hovered_preset_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("wl_modal:item:") {
                    self.watchlist_modal.hovered_item_id = Some(local.to_string());
                } else if id.starts_with("tags_tabs:") || id.starts_with("overlay_settings:") {
                    self.panel_app.overlay_settings_state.hovered_item_id = Some(id_str.clone());
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

        // Phase 7.3: crosshair gate — show only when hovered widget is chart:pane:*.
        // Hovering price/time scale, sub-pane separator, toolbar, modal, etc. all
        // suppress the crosshair.  drawing_active bypasses the gate so the preview
        // line stays visible when the cursor drifts outside the chart pane.
        {
            let hovered_chart_pane = self.input_coordinator.borrow().hovered_widget()
                .map(|w| w.0.starts_with("chart:pane:"))
                .unwrap_or(false);
            let is_drawing = self.panel_app.panel_grid.active_window()
                .map(|w| w.drawing_manager.is_drawing())
                .unwrap_or(false);
            if !hovered_chart_pane && !is_drawing {
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
                    // Build layout for the hovered leaf and delegate crosshair
                    // update to ChartPanelGrid::update_crosshair_split.
                    let leaf_rect_opt = self.get_leaf_absolute_rect(leaf_id);
                    if let Some(leaf_rect) = leaf_rect_opt {
                        let extended_opt = self.build_extended_layout_for_leaf(leaf_id, &leaf_rect);
                        if let Some(extended) = extended_opt {
                            if let Some((ts, price, vis, pane_idx)) = self.panel_app.panel_grid
                                .update_crosshair_split(x, y, leaf_id, &extended)
                            {
                                // Propagate to sync-group peers (handles order-flow panels too).
                                self.propagate_crosshair_to_sync_group(leaf_id, ts, price, vis, pane_idx);
                            }
                        }
                    }
                    // Hide crosshair on leaves outside the sync group.
                    self.hide_crosshairs_outside_sync_group(leaf_id);
                }
                ChartInputTarget::Separator { .. } | ChartInputTarget::None => {
                    self.hide_all_split_crosshairs();
                }
            }

            // --- Update sub-pane overlay visibility for split mode ---
            // Determine which leaf the cursor is in and update its overlay states.
            {
                let hovered_leaf = match self.panel_app.panel_grid.resolve_input(
                    x, y, self.content_rect.x, self.content_rect.y,
                ) {
                    ChartInputTarget::Chart { leaf_id }
                    | ChartInputTarget::PriceScale { leaf_id }
                    | ChartInputTarget::TimeScale { leaf_id }
                    | ChartInputTarget::ScaleCorner { leaf_id, .. } => Some(leaf_id),
                    _ => None,
                };
                if let Some(leaf_id) = hovered_leaf {
                    let leaf_rect_opt = self.get_leaf_absolute_rect(leaf_id);
                    let extended_opt = leaf_rect_opt.and_then(|lr| self.build_extended_layout_for_leaf(leaf_id, &lr));
                    let overlay_results_split = self.panel_app.panel_grid
                        .window_for_leaf(leaf_id)
                        .map(|w| w.sub_pane_overlay_results.clone())
                        .unwrap_or_default();
                    let mut any_button_hovered_split = false;
                    if let (Some(extended), Some(window)) = (extended_opt, self.panel_app.panel_grid.window_for_leaf_mut(leaf_id)) {
                        for state in window.sub_pane_overlay_states.iter_mut() {
                            state.visible = false;
                            state.hovered_button = None;
                            state.hovered_left_button = None;
                        }
                        for (layout_idx, pane_layout) in extended.sub_panes.iter().enumerate() {
                            if pane_layout.content.contains(x, y) {
                                // Find the real position in window.sub_panes by instance_id.
                                // During maximized mode, extended.sub_panes has only 1 entry
                                // but window.sub_panes may have the pane at a different index.
                                let real_idx = window.sub_panes
                                    .iter()
                                    .position(|p| p.instance_id == pane_layout.instance_id)
                                    .unwrap_or(layout_idx);
                                while window.sub_pane_overlay_states.len() <= real_idx {
                                    window.sub_pane_overlay_states.push(Default::default());
                                }
                                window.sub_pane_overlay_states[real_idx].visible = true;
                                if layout_idx < overlay_results_split.len() {
                                    use zengeld_chart::ui::modal_settings::SubPaneButton;
                                    let overlay = &overlay_results_split[layout_idx];
                                    let hovered = if overlay.delete_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::Delete)
                                    } else if overlay.hide_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::Hide)
                                    } else if overlay.move_up_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::MoveUp)
                                    } else if overlay.move_down_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::MoveDown)
                                    } else if overlay.expand_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::Expand)
                                    } else if overlay.restore_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::Restore)
                                    } else {
                                        None
                                    };
                                    let hovered_left = if overlay.left_eye_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::IndicatorEye)
                                    } else if overlay.left_alert_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::IndicatorAlert)
                                    } else if overlay.left_settings_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::IndicatorSettings)
                                    } else if overlay.left_delete_rect.as_ref().is_some_and(|r| r.contains(x, y)) {
                                        Some(SubPaneButton::IndicatorDelete)
                                    } else {
                                        None
                                    };
                                    any_button_hovered_split = hovered.is_some() || hovered_left.is_some();
                                    window.sub_pane_overlay_states[real_idx].hovered_button = hovered;
                                    window.sub_pane_overlay_states[real_idx].hovered_left_button = hovered_left;
                                }
                                break;
                            }
                        }
                    }
                    // Suppress crosshair when hovering over an overlay button (split mode).
                    if any_button_hovered_split {
                        self.hide_crosshair();
                    }
                } else {
                    // Cursor not over any leaf — hide all overlays on all windows.
                    for leaf_id in self.panel_app.panel_grid.panel_rects().keys().copied().collect::<Vec<_>>() {
                        if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                            for state in window.sub_pane_overlay_states.iter_mut() {
                                state.visible = false;
                                state.hovered_button = None;
                                state.hovered_left_button = None;
                            }
                        }
                    }
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

        // Hide crosshair during separator drag — resize cursor is sufficient feedback.
        if matches!(self.input_handler.state.drag_mode, DragMode::PaneSeparator { .. }) {
            self.hide_crosshair();
            return;
        }
        let drag_mode = self.input_handler.state.drag_mode;
        if let Some((timestamp, price, crosshair_visible, pane_index)) = self.panel_app.panel_grid
            .update_crosshair(x, y, drag_mode, is_drawing, &extended)
        {
            let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
            if let Some(active_leaf) = active_leaf_opt {
                self.propagate_crosshair_to_sync_group(active_leaf, timestamp, price, crosshair_visible, pane_index);
            }
        }

        // Hover-to-activate: when a drawing tool is globally active and the
        // panel is split, switch active_leaf to whichever leaf the mouse is
        // hovering over. This avoids wasting a click on window activation when
        // the user moves the cursor to a different split pane while drawing.
        if self.panel_app.toolbar_state.active_tool_id.is_some()
            && self.panel_app.panel_grid.is_split()
        {
            use zengeld_chart::state::ChartInputTarget;
            let target = self.panel_app.panel_grid.resolve_input(
                x, y, self.content_rect.x, self.content_rect.y,
            );
            match target {
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => {
                    let current_active = self.panel_app.panel_grid.docking().active_leaf();
                    if current_active != Some(leaf_id) {
                        self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    }
                }
                _ => {}
            }
        }

        // --- Update sub-pane overlay visibility based on cursor position ---
        // Clone overlay results to avoid conflicting borrows on panel_grid.
        let overlay_results_mm = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hide_crosshair_for_overlay = self.panel_app.panel_grid
            .update_sub_pane_overlay_hover(x as f64, y as f64, &extended, &overlay_results_mm);
        // Suppress crosshair when the cursor is over an overlay button.
        if hide_crosshair_for_overlay {
            self.hide_crosshair();
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
            self.propagate_crosshair_to_sync_group(active_leaf, 0, 0.0, false, None);
        }
    }

    /// Check if the sidebar separator is being dragged.
    pub fn is_sidebar_separator_dragging(&self) -> bool {
        self.sidebar_separator_drag_active
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

        // ── Active agent separator drag: show resize cursor for entire drag duration ──
        if let Some((sep_idx, _, _)) = self.agent_sep_drag {
            use uzor::panels::SeparatorOrientation;
            let orientation = self.sidebar_state.agent_docking
                .inner()
                .separators()
                .get(sep_idx)
                .map(|s| s.orientation);
            return match orientation {
                Some(SeparatorOrientation::Vertical) => CursorStyle::EwResize,
                _ => CursorStyle::NsResize,
            };
        }

        // ── Active slot separator drag: show resize cursor for entire drag duration ──
        if let Some((slot_idx, sep_idx, _, _)) = self.slot_sep_drag {
            use uzor::panels::SeparatorOrientation;
            let orientation = self.sidebar_state.slot_dockings
                .get(slot_idx)
                .and_then(|d| d.inner().separators().get(sep_idx))
                .map(|s| s.orientation);
            return match orientation {
                Some(SeparatorOrientation::Vertical) => CursorStyle::EwResize,
                _ => CursorStyle::NsResize,
            };
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

        // ── Agent panel separator hover ────────────────────────────────────────
        // Must be checked before the generic is_over_ui() → Default fallback,
        // because separator widgets ARE registered UI elements (so is_over_ui()
        // would fire first and swallow the resize cursor).
        {
            use uzor::panels::SeparatorOrientation;
            let hovered = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
            if let Some(ref wid) = hovered {
                if let Some(idx_str) = wid.strip_prefix("agent:sep:") {
                    if let Ok(sep_idx) = idx_str.parse::<usize>() {
                        let orientation = self.sidebar_state.agent_docking
                            .inner()
                            .separators()
                            .get(sep_idx)
                            .map(|s| s.orientation);
                        return match orientation {
                            Some(SeparatorOrientation::Vertical) => CursorStyle::EwResize,
                            _ => CursorStyle::NsResize,
                        };
                    }
                }

                // slot:{slot_idx}:sep:{sep_idx}
                if let Some(rest) = wid.strip_prefix("slot:") {
                    if let Some((slot_str, sep_str)) = rest.split_once(":sep:") {
                        if let (Ok(slot_idx), Ok(sep_idx)) =
                            (slot_str.parse::<usize>(), sep_str.parse::<usize>())
                        {
                            let orientation = self.sidebar_state.slot_dockings
                                .get(slot_idx)
                                .and_then(|d| d.inner().separators().get(sep_idx))
                                .map(|s| s.orientation);
                            return match orientation {
                                Some(SeparatorOrientation::Vertical) => CursorStyle::EwResize,
                                _ => CursorStyle::NsResize,
                            };
                        }
                    }
                }
            }
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
    /// - `ctrl`: whether the Ctrl modifier is held (used for DOM tick_size zoom).
    ///
    /// When a modal is open, scroll is routed to the modal's scroll state
    /// instead of the chart canvas.
    pub fn on_scroll(&mut self, x: f64, y: f64, dx: f64, dy: f64, ctrl: bool) {
        // Route scroll to open modal content instead of blocking entirely.
        if self.input_coordinator.borrow_mut().is_blocked_by_modal(x, y) {
            // Normalise dy: wheel delta is typically negative when scrolling down
            // (content moves up), so negate to convert to "scroll offset increase".
            let scroll_step = -dy;

            // Phase 6.1c-color: route opacity slider wheel via coordinator.
            // Each scroll notch changes opacity by 1% (0.01). Scroll up = more opaque.
            {
                let opacity_step = 0.01 * -dy.signum();
                let source = self.input_coordinator.borrow().hovered_widget().and_then(|wid| {
                    wid.0.strip_prefix("color_picker_")
                        .and_then(|s| s.strip_suffix(":opacity_slider"))
                        .map(|s| s.to_string())
                });
                if let Some(src) = source {
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
                        "compare" => {
                            let current = self.panel_app.compare_settings_state.color_picker.get_opacity();
                            let new_opacity = (current + opacity_step).clamp(0.0, 1.0);
                            self.panel_app.compare_settings_state.color_picker.set_opacity(new_opacity);
                            let color = self.panel_app.compare_settings_state.color_picker.get_final_color();
                            self.apply_compare_color(&color);
                        }
                        "panel" => {
                            let current = self.panel_app.panel_color_picker.get_opacity();
                            let new_opacity = (current + opacity_step).clamp(0.0, 1.0);
                            self.panel_app.panel_color_picker.set_opacity(new_opacity);
                        }
                        _ => {}
                    }
                    return;
                }
            }

            // Phase 6.1a: route 6 simple modals via coordinator scroll surface registrations.
            // hovered_widget() returns the topmost registered widget under (x, y) within the
            // active modal layer — meaning hit detection is already done by the coordinator.
            {
                let hovered_id = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
                if let Some(ref id) = hovered_id {
                    match id.as_str() {
                        "chart_settings:scroll_viewport" => {
                            if let Some(ref result) = self.frame_result {
                                if let Some(ref cs) = result.chart_settings {
                                    self.panel_app.chart_settings_state.scroll.handle_wheel(
                                        scroll_step,
                                        cs.total_content_height,
                                        cs.viewport_height,
                                    );
                                }
                            }
                            return;
                        }
                        "ind_settings:scroll_viewport" => {
                            if let Some(ref result) = self.frame_result {
                                if let Some(ref is) = result.indicator_settings {
                                    self.panel_app.indicator_settings_state.scroll.handle_wheel(
                                        scroll_step,
                                        is.total_content_height,
                                        is.viewport_height,
                                    );
                                }
                            }
                            return;
                        }
                        "alert_settings:list_viewport" => {
                            if let Some(ref result) = self.frame_result {
                                if let Some(ref asr) = result.alert_settings {
                                    if let Some(ref vp) = asr.list_viewport_rect {
                                        self.panel_app.alert_settings_state.list_scroll.handle_wheel(
                                            scroll_step,
                                            asr.list_total_content_height,
                                            vp.height,
                                        );
                                    }
                                }
                            }
                            return;
                        }
                        "user_settings:profile_list_viewport" => {
                            if let Some(ref result) = self.frame_result {
                                if let Some(ref us) = result.user_settings {
                                    let viewport_h = us.profile_list_viewport_rect.height;
                                    let total_h = us.profile_list_total_content_h;
                                    self.panel_app.user_settings_state.profile_list_scroll
                                        .handle_wheel(scroll_step, total_h, viewport_h);
                                }
                            }
                            return;
                        }
                        "watchlist:list_viewport" => {
                            if let Some(ref wl) = self.last_watchlist_modal_result {
                                self.watchlist_modal.scroll.handle_wheel(
                                    -dy,
                                    wl.total_content_height,
                                    wl.list_viewport_rect.height,
                                );
                            }
                            return;
                        }
                        "chart_browser:list_viewport" => {
                            if let Some(ref result) = self.frame_result {
                                if let Some(ref br) = result.chart_browser {
                                    self.panel_app.chart_browser.scroll.handle_wheel(
                                        dy,
                                        br.total_content_height,
                                        br.list_viewport_rect.height,
                                    );
                                }
                            }
                            return;
                        }
                        "tags_tabs:scroll_viewport" => {
                            if let Some(ref tt) = self.frame_result
                                .as_ref()
                                .and_then(|r| r.tags_tabs.as_ref())
                                .cloned()
                            {
                                use crate::scroll_dispatch::{ScrollableInfo, try_handle_wheel};
                                let info = ScrollableInfo {
                                    handle_rect:     tt.scrollbar_handle_rect,
                                    track_rect:      tt.scrollbar_track_rect,
                                    content_height:  tt.scroll_content_height,
                                    viewport_height: tt.scroll_viewport_height,
                                    viewport_rect:   tt.scroll_viewport_rect,
                                };
                                let scroll_state = tags_tabs_active_scroll(&mut self.panel_app.tags_tabs_state);
                                let _ = try_handle_wheel(x, y, scroll_step, &mut [(&info, scroll_state)]);
                            }
                            return;
                        }
                        "user_settings:scroll_viewport" => {
                            if let Some(ref us) = self.frame_result
                                .as_ref()
                                .and_then(|r| r.user_settings.as_ref())
                                .cloned()
                            {
                                use crate::scroll_dispatch::{ScrollableInfo, try_handle_wheel};
                                use zengeld_chart::ui::modal_settings::UserSettingsTab;
                                let info = ScrollableInfo {
                                    handle_rect: us.scrollbar_handle_rect,
                                    track_rect: us.scrollbar_track_rect,
                                    content_height: us.scroll_content_height,
                                    viewport_height: us.scroll_viewport_height,
                                    viewport_rect: us.scroll_viewport_rect,
                                };
                                let scroll_state = match self.panel_app.user_settings_state.active_tab {
                                    UserSettingsTab::General => &mut self.panel_app.user_settings_state.general_tab_scroll,
                                    UserSettingsTab::Sync => &mut self.panel_app.user_settings_state.sync_tab_scroll,
                                    UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_keys_scroll,
                                    UserSettingsTab::Performance => &mut self.panel_app.user_settings_state.performance_tab_scroll,
                                };
                                let _ = try_handle_wheel(x, y, scroll_step, &mut [(&info, scroll_state)]);
                            }
                            return;
                        }
                        _ => {}
                    }
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

            // Phase 6.1c-sliders: route prim_settings and compare_settings slider
            // wheel events via coordinator. Sense::SCROLL was added to their registrations
            // so hovered_widget() resolves them by the same hit-test path used for clicks.
            {
                let hovered_wid = self.input_coordinator.borrow().hovered_widget().map(|w| w.0.clone());

                // prim_settings sliders — guard: modal must be open
                if self.panel_app.primitive_settings_state.is_open() {
                    if let Some(ref wid) = hovered_wid {
                        if let Some(field_id) = wid.strip_prefix("prim_settings:item:").map(|s| s.to_string()) {
                            let delta = dy.signum();
                            // Locate matching track for min/max bounds
                            let track_opt = self.frame_result.as_ref()
                                .and_then(|r| r.primitive_settings.as_ref())
                                .and_then(|ps| ps.slider_tracks.iter().find(|t| t.field_id == field_id).cloned());
                            let item_rect_opt = self.frame_result.as_ref()
                                .and_then(|r| r.primitive_settings.as_ref())
                                .and_then(|ps| ps.content_items.iter().find(|(id, _)| id == &field_id).map(|(_, r)| *r));

                            if let Some(track) = track_opt {
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
                                                    _ => { return; }
                                                };
                                                let (current_min, current_max) = match tf_idx {
                                                    1 => tf_config.seconds.unwrap_or((1, 59)),
                                                    2 => tf_config.minutes.unwrap_or((1, 59)),
                                                    3 => tf_config.hours.unwrap_or((1, 24)),
                                                    4 => tf_config.days.unwrap_or((1, 366)),
                                                    5 => tf_config.weeks.unwrap_or((1, 52)),
                                                    6 => tf_config.months.unwrap_or((1, 12)),
                                                    _ => { return; }
                                                };
                                                let item_rect = item_rect_opt.unwrap_or_default();
                                                let t = if item_rect.width > 0.0 {
                                                    ((x - item_rect.x) / item_rect.width).clamp(0.0, 1.0)
                                                } else {
                                                    0.5
                                                };
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
                                                self.sync_drawing_back_to_group();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // prim_settings always swallows wheel (modal eats scroll even when no slider hit)
                    return;
                }

                // compare_settings sliders — guard: modal must be open
                if self.panel_app.compare_settings_state.is_open() {
                    use zengeld_chart::ui::modal_settings::CompareSettingsTab;
                    let delta = dy.signum();

                    if let Some(ref wid) = hovered_wid {
                        // tf_*_slider — dual-handle, only on Visibility tab
                        if let Some(field_id) = wid.strip_prefix("cmp_settings:item:").map(|s| s.to_string()) {
                            if field_id.starts_with("tf_") && field_id.ends_with("_slider")
                                && self.panel_app.compare_settings_state.active_tab == CompareSettingsTab::Visibility
                            {
                                if let Some(tf_idx) = field_id.strip_prefix("tf_")
                                    .and_then(|s| s.strip_suffix("_slider"))
                                    .and_then(|s| s.parse::<usize>().ok())
                                {
                                    let item_rect_opt = self.frame_result.as_ref()
                                        .and_then(|r| r.compare_settings.as_ref())
                                        .and_then(|cs| cs.tf_content_items.iter().find(|(id, _)| id == &field_id).map(|(_, r)| *r));

                                    use zengeld_chart::drawing::TimeframeVisibilityConfig;
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
                                    let item_rect = item_rect_opt.unwrap_or_default();
                                    let t = if item_rect.width > 0.0 {
                                        ((x - item_rect.x) / item_rect.width).clamp(0.0, 1.0)
                                    } else {
                                        0.5
                                    };
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
                                    eprintln!("[ChartApp] cmp_settings scroll on tf_{}_slider", tf_idx);
                                }
                            }
                        }

                        // line_width_slider — single-handle, step=0.5, only on Style tab
                        if wid.as_str() == "cmp_settings:line_width_slider"
                            && self.panel_app.compare_settings_state.active_tab == CompareSettingsTab::Style
                        {
                            let (min_val, max_val) = self.frame_result.as_ref()
                                .and_then(|r| r.compare_settings.as_ref())
                                .and_then(|cs| cs.line_width_slider.as_ref())
                                .map(|t| (t.min_val, t.max_val))
                                .unwrap_or((0.5, 10.0));
                            let current = self.panel_app.compare_settings_state.cached_line_width as f64;
                            let new_val = (current + delta * 0.5).clamp(min_val, max_val) as f32;
                            let series_idx = self.panel_app.compare_settings_state.series_index;
                            self.panel_app.compare_settings_state.cached_line_width = new_val;
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                window.compare_overlay.set_series_line_width_by_index(series_idx, new_val);
                            }
                            eprintln!("[ChartApp] cmp_settings scroll on line_width: {}", new_val);
                        }
                    }
                    // compare_settings always swallows wheel
                    return;
                }
            }

            // Any other modal or widget layer — swallow the event.
            return;
        }

        // Check if mouse is over the right sidebar — route scroll there.
        if self.sidebar_state.is_right_open() {
            if let Some(ref sidebar_result) = self.last_sidebar_result {
                let sr = &sidebar_result.sidebar_rect;
                if x >= sr.x && x <= sr.x + sr.width && y >= sr.y && y <= sr.y + sr.height {
                    // Phase 6.2b: signal group viewport scroll via coordinator dispatch.
                    // Replaces the manual loop above — the coordinator hit-test resolves
                    // which group is under the cursor, so no geometry iteration needed here.
                    {
                        let sg_hovered = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
                        if let Some(ref sg_id) = sg_hovered {
                            if let Some(iid_str) = sg_id.strip_prefix("signal_group:").and_then(|r| r.strip_suffix(":viewport")) {
                                if let Ok(iid) = iid_str.parse::<u64>() {
                                    let row_height = 24.0_f64;
                                    let max_visible = 8usize;
                                    let signal_count = self
                                        .sidebar_state
                                        .indicator_signals
                                        .groups
                                        .iter()
                                        .find(|g| g.instance_id == iid)
                                        .map(|g| g.signals.len())
                                        .unwrap_or(0);
                                    let viewport_h = signal_count.min(max_visible) as f64 * row_height;
                                    let total_h = signal_count as f64 * row_height;
                                    let max_offset = (total_h - viewport_h).max(0.0);
                                    let current_offset = self
                                        .sidebar_state
                                        .signal_group_scroll
                                        .get(&iid)
                                        .map(|s| s.offset)
                                        .unwrap_or(0.0);
                                    let new_offset = (current_offset - dy * 30.0).clamp(0.0, max_offset);
                                    self.sidebar_state
                                        .signal_group_scroll
                                        .entry(iid)
                                        .or_default()
                                        .offset = new_offset;
                                    return;
                                }
                            }
                        }
                    }

                    // Phase 6.2a: agent scroll surfaces via coordinator dispatch.
                    if self.sidebar_state.right_panel == sidebar_content::state::RightSidebarPanel::Agents {
                        let hovered_id = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
                        if let Some(ref id) = hovered_id {
                            let scroll_step = -dy;

                            if let Some(rest) = id.strip_suffix(":model_scroll_area") {
                                if let Some(lid_str) = rest.strip_prefix("agent:leaf:") {
                                    if let Ok(raw) = lid_str.parse::<u64>() {
                                        let lid = uzor::panels::LeafId(raw);
                                        let caps2 = match self.sidebar_state.agent_leaves.get(&lid).map(|d| d.cli) {
                                            Some(gate4agent::AgentCli::Claude) => gate4agent::CliTool::ClaudeCode.capabilities(),
                                            Some(gate4agent::AgentCli::Codex) => gate4agent::CliTool::Codex.capabilities(),
                                            Some(gate4agent::AgentCli::Gemini) => gate4agent::CliTool::Gemini.capabilities(),
                                            Some(gate4agent::AgentCli::OpenCode) => gate4agent::CliTool::OpenCode.capabilities(),
                                            None => gate4agent::CliTool::ClaudeCode.capabilities(),
                                        };
                                        let item_h = 24.0;
                                        let total_h = caps2.available_models.len() as f64 * item_h + 6.0;
                                        // viewport_h: sourced from item_rects rect.height (same as original check_popup)
                                        let viewport_h = sidebar_result.item_rects.iter()
                                            .find(|(wid, _)| wid.as_str() == id.as_str())
                                            .map(|(_, r)| r.height)
                                            .unwrap_or(total_h);
                                        self.sidebar_state.agent_model_scroll
                                            .entry(lid).or_default()
                                            .handle_wheel(scroll_step, total_h, viewport_h);
                                        return;
                                    }
                                }
                            } else if let Some(rest) = id.strip_suffix(":perm_scroll_area") {
                                if let Some(lid_str) = rest.strip_prefix("agent:leaf:") {
                                    if let Ok(raw) = lid_str.parse::<u64>() {
                                        let lid = uzor::panels::LeafId(raw);
                                        let caps2 = match self.sidebar_state.agent_leaves.get(&lid).map(|d| d.cli) {
                                            Some(gate4agent::AgentCli::Claude) => gate4agent::CliTool::ClaudeCode.capabilities(),
                                            Some(gate4agent::AgentCli::Codex) => gate4agent::CliTool::Codex.capabilities(),
                                            Some(gate4agent::AgentCli::Gemini) => gate4agent::CliTool::Gemini.capabilities(),
                                            Some(gate4agent::AgentCli::OpenCode) => gate4agent::CliTool::OpenCode.capabilities(),
                                            None => gate4agent::CliTool::ClaudeCode.capabilities(),
                                        };
                                        let item_h = 24.0;
                                        let total_h = caps2.permission_modes.len() as f64 * item_h + 6.0;
                                        let viewport_h = sidebar_result.item_rects.iter()
                                            .find(|(wid, _)| wid.as_str() == id.as_str())
                                            .map(|(_, r)| r.height)
                                            .unwrap_or(total_h);
                                        self.sidebar_state.agent_perm_scroll
                                            .entry(lid).or_default()
                                            .handle_wheel(scroll_step, total_h, viewport_h);
                                        return;
                                    }
                                }
                            } else if let Some(rest) = id.strip_suffix(":sessions_scroll_area") {
                                if let Some(lid_str) = rest.strip_prefix("agent:leaf:") {
                                    if let Ok(raw) = lid_str.parse::<u64>() {
                                        let lid = uzor::panels::LeafId(raw);
                                        let item_h = 22.0;
                                        let n = self.sidebar_state.agent_past_sessions.get(&lid)
                                            .map(|v| v.len()).unwrap_or(0);
                                        let total_h = (n as f64 * item_h + 4.0).max(28.0);
                                        let viewport_h = sidebar_result.item_rects.iter()
                                            .find(|(wid, _)| wid.as_str() == id.as_str())
                                            .map(|(_, r)| r.height.max(28.0))
                                            .unwrap_or(28.0);
                                        self.sidebar_state.agent_sessions_scroll
                                            .entry(lid).or_default()
                                            .handle_wheel(scroll_step, total_h, viewport_h);
                                        return;
                                    }
                                }
                            } else if let Some(rest) = id.strip_suffix(":focus_content") {
                                if let Some(lid_str) = rest.strip_prefix("agent:leaf:") {
                                    if let Ok(raw) = lid_str.parse::<u64>() {
                                        let lid = uzor::panels::LeafId(raw);
                                        let leaf_mode = self.sidebar_state.agent_leaves.get(&lid).map(|d| d.mode);
                                        let rect_h = sidebar_result.item_rects.iter()
                                            .find(|(wid, _)| wid.as_str() == id.as_str())
                                            .map(|(_, r)| r.height)
                                            .unwrap_or(0.0);
                                        let (total_h, vp_h) = sidebar_result.agent_leaf_content_heights
                                            .get(&lid)
                                            .copied()
                                            .unwrap_or((0.0, rect_h));
                                        match leaf_mode {
                                            Some(gate4agent::InstanceMode::Chat) => {
                                                self.sidebar_state.agent_chat_scrolls
                                                    .entry(lid).or_default()
                                                    .handle_wheel(-dy, total_h, vp_h);
                                            }
                                            Some(gate4agent::InstanceMode::Pty) => {
                                                self.sidebar_state.agent_pty_scrolls
                                                    .entry(lid).or_default()
                                                    .handle_wheel(-dy, total_h, vp_h);
                                            }
                                            None => {}
                                        }
                                        // Always swallow wheel events in the Agents panel.
                                        return;
                                    }
                                }
                            }
                        }
                        // Always swallow wheel events in the Agents panel (no outer sidebar scroll).
                        return;
                    }

                    // Slot panels (Slot1..Slot4): route wheel to the hovered leaf body.
                    {
                        use sidebar_content::state::RightSidebarPanel;
                        let slot_idx_opt = match self.sidebar_state.right_panel {
                            RightSidebarPanel::Slot1 => Some(0usize),
                            RightSidebarPanel::Slot2 => Some(1),
                            RightSidebarPanel::Slot3 => Some(2),
                            RightSidebarPanel::Slot4 => Some(3),
                            _ => None,
                        };

                        if let Some(slot_idx) = slot_idx_opt {
                            // Hit-test all `slot:{idx}:leaf:{lid}:focus_content` rects.
                            let hovered_slot_leaf = sidebar_result.item_rects.iter()
                                .filter_map(|(wid, wrect)| {
                                    let rest = wid.strip_prefix("slot:")?;
                                    let (idx_str, leaf_rest) = rest.split_once(":leaf:")?;
                                    let leaf_id_str = leaf_rest.strip_suffix(":focus_content")?;
                                    let panel_idx: usize = idx_str.parse().ok()?;
                                    if panel_idx != slot_idx { return None; }
                                    let raw: u64 = leaf_id_str.parse().ok()?;
                                    if x >= wrect.x && x < wrect.x + wrect.width
                                        && y >= wrect.y && y < wrect.y + wrect.height
                                    {
                                        Some((uzor::panels::LeafId(raw), wrect.height))
                                    } else {
                                        None
                                    }
                                })
                                .next();

                            if let Some((hover_leaf_id, _rect_h)) = hovered_slot_leaf {
                                // Look up the active FreeItem for this leaf.
                                let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                    .inner()
                                    .tree()
                                    .leaf(hover_leaf_id)
                                    .and_then(|l| l.active_panel().cloned());

                                use sidebar_content::free_slot::FreeItem;
                                let scroll_step = dy;

                                match item_opt {
                                    Some(FreeItem::Dom(pid)) => {
                                        if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                            if ctrl {
                                                // Ctrl+scroll: zoom tick_size (depth aggregation).
                                                // Scroll up → multiply by 10 (zoom out / coarser).
                                                // Scroll down → divide by 10 (zoom in / finer).
                                                // `set_tick_size` rebuilds aggregation from raw orderbook
                                                // so no data is lost waiting for the next snapshot.
                                                let new_tick = if scroll_step > 0.0 {
                                                    (state.tick_size * 10.0).clamp(0.0001, 100.0)
                                                } else if scroll_step < 0.0 {
                                                    (state.tick_size / 10.0).clamp(0.0001, 100.0)
                                                } else {
                                                    state.tick_size
                                                };
                                                state.set_tick_size(new_tick);
                                            } else {
                                                // Normal scroll: move center price 1 tick per notch.
                                                state.auto_center = false;
                                                let lines = (scroll_step / 20.0).round(); // normalize: 1 notch = ±20.0 → ±1 line
                                                let delta = lines * state.tick_size;
                                                state.center_price += delta;
                                            }
                                        }
                                    }
                                    Some(item @ FreeItem::BigTrades(_))
                                    | Some(item @ FreeItem::L2Tape(_))
                                    | Some(item @ FreeItem::Footprint(_))
                                    | Some(item @ FreeItem::LiquidityHeatmap(_))
                                    | Some(item @ FreeItem::VolumeProfile(_))
                                    | Some(item @ FreeItem::PositionManager(_))
                                    | Some(item @ FreeItem::TradeLog(_)) => {
                                        let local_id = match &item {
                                            FreeItem::BigTrades(_) => "bigtrades:body",
                                            FreeItem::L2Tape(_) => "l2tape:body",
                                            FreeItem::Footprint(_) => "footprint:body",
                                            FreeItem::LiquidityHeatmap(_) => "heatmap:body",
                                            FreeItem::VolumeProfile(_) => "volprofile:body",
                                            FreeItem::PositionManager(_) => "position_manager:body",
                                            FreeItem::TradeLog(_) => "trade_log:body",
                                            _ => unreachable!(),
                                        };
                                        if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                            panel.handle_scroll(local_id, 0.0, scroll_step);
                                            self.sidebar_data_dirty = true;
                                        }
                                    }
                                    Some(FreeItem::TradeTape(pid)) => {
                                        if let Some(state) = self.panels_store.trade_tape.get_mut(&pid) {
                                            state.handle_scroll(scroll_step * 3.0);
                                        }
                                    }
                                    _ => {}
                                }
                                return;
                            }
                        }
                    }

                    // Phase 6.2b: main right sidebar body scroll via coordinator dispatch.
                    {
                        let sidebar_hovered = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
                        if sidebar_hovered.as_deref() == Some("right_sidebar:viewport") {
                            let content_h = sidebar_result.content_height;
                            let viewport_h = sidebar_result.content_rect.height;
                            let max_offset = (content_h - viewport_h).max(0.0);
                            let scroll = self.sidebar_state.current_right_scroll_mut();
                            scroll.offset = (scroll.offset - dy * 30.0).clamp(0.0, max_offset);
                            return;
                        }
                    }
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
        let overlay_results_sc = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(&extended)
            .with_overlays(&overlay_results_sc);
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
        // While the profile manager is shown, only the passphrase, recovery key,
        // and name inputs may receive keyboard events.  All other char routing is
        // blocked to prevent data leaking into hidden inputs or triggering chart
        // keyboard shortcuts.
        // The recovery key DISPLAY box (read-only) is also excluded from char input.
        if self.panel_app.user_settings_state.show_profile_manager
            && !self.panel_app.user_settings_state.e2e_passphrase_focused
            && !self.panel_app.user_settings_state.new_profile_name_focused
            && !self.panel_app.user_settings_state.recovery_key_focused
            && !self.panel_app.user_settings_state.new_passphrase_focused
            && !self.panel_app.user_settings_state.confirm_passphrase_focused
        {
            return;
        }

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
                    self.panel_app.chart_browser.scroll.reset();
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
                    self.panel_app.chart_browser.scroll.reset();
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
                        self.watchlist_modal.scroll.reset();
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
                        self.watchlist_modal.scroll.reset();
                    }
                    _ => {}
                }
                return;
            }
        }

        // Handle color picker hex input editing.
        // Check all color picker instances; the first one with hex_editing=true consumes the event.
        let hex_id = WidgetId::new(crate::text_input::HEX_COLOR);
        if self.input_coordinator.borrow().text_fields().is_focused(&hex_id) {
            let action = self.input_coordinator.borrow_mut().text_fields_mut().on_char(ch);
            match action {
                TextAction::Commit(_) | TextAction::Cancel => {
                    // Close hex editing on all pickers.
                    self.panel_app.primitive_settings_state.color_picker.hex_editing = false;
                    self.panel_app.indicator_settings_state.color_picker.hex_editing = false;
                    self.panel_app.chart_settings_state.color_picker.hex_editing = false;
                    self.panel_app.compare_settings_state.color_picker.hex_editing = false;
                    self.panel_app.panel_color_picker.hex_editing = false;
                    self.input_coordinator.borrow_mut().text_fields_mut().blur();
                }
                TextAction::TextChanged(ref new_hex) => {
                    // Live-apply: sync the new hex text to the active picker.
                    let pickers: [(&mut zengeld_chart::ui::color_picker_state::ColorPickerState, &str); 5] = [
                        (&mut self.panel_app.primitive_settings_state.color_picker, "primitive"),
                        (&mut self.panel_app.indicator_settings_state.color_picker, "indicator"),
                        (&mut self.panel_app.chart_settings_state.color_picker, "chart"),
                        (&mut self.panel_app.compare_settings_state.color_picker, "compare"),
                        (&mut self.panel_app.panel_color_picker, "panel"),
                    ];
                    for (picker, _src) in pickers {
                        if picker.hex_editing {
                            picker.hex_set_text(new_hex);
                            break;
                        }
                    }
                }
                TextAction::None => {}
                TextAction::RawInput(_) => {
                    // RawInput is for AgentPty, not HexColor — ignore here.
                }
            }
            return;
        }
        // Agent PTY input — route raw characters directly to the focused leaf's PTY.
        let pty_id = WidgetId::new(crate::text_input::AGENT_PTY);
        if self.input_coordinator.borrow().text_fields().is_focused(&pty_id) {
            let action = self.input_coordinator.borrow_mut().text_fields_mut().on_char(ch);
            if let TextAction::RawInput(bytes) = action {
                if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                    if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                        if desc.mode == gate4agent::InstanceMode::Pty {
                            let text = String::from_utf8_lossy(&bytes).to_string();
                            let id = desc.instance_id;
                            let _ = self.bridge.runtime().block_on(self.agent.write_pty_instance(id, &text));
                        }
                    }
                }
            }
            return;
        }
        // Agent chat input — route printable characters and Enter to the focused leaf's chat.
        let chat_id = WidgetId::new(crate::text_input::AGENT_CHAT);
        if self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
            if ch == '\r' || ch == '\n' {
                if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                    let desc = self.sidebar_state.agent_leaves.get(&leaf_id).cloned();
                    if let Some(desc) = desc {
                        let text = {
                            let coord = self.input_coordinator.borrow();
                            coord.text_fields().text(&chat_id).to_string()
                        };
                        eprintln!("[gate4agent::chat] Enter via on_char text_len={}", text.len());
                        if !text.is_empty() {
                            let id = desc.instance_id;
                            match self.bridge.runtime().block_on(self.agent.send_chat_instance(id, &text)) {
                                Ok(()) => {
                                    let buf = self.sidebar_state.agent_input_buffers
                                        .entry(leaf_id).or_default();
                                    buf.clear();
                                    self.input_coordinator.borrow_mut().text_fields_mut().set_text(&chat_id, "");
                                    eprintln!("[gate4agent::chat] Enter via on_char OK");
                                }
                                Err(e) => eprintln!("[gate4agent::chat] Enter via on_char error: {}", e),
                            }
                        }
                    }
                }
            } else {
                let _action = self.input_coordinator.borrow_mut().text_fields_mut().on_char(ch);
                if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                    let new_text = {
                        let coord = self.input_coordinator.borrow();
                        coord.text_fields().text(&chat_id).to_string()
                    };
                    self.sidebar_state.agent_input_buffers.insert(leaf_id, new_text);
                }
            }
            return;
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

        // Handle E2E passphrase input in User Settings Sync tab, Welcome Wizard, or Profile Manager
        if (self.panel_app.user_settings_state.is_open || self.panel_app.user_settings_state.show_welcome_wizard || self.panel_app.user_settings_state.needs_vault_unlock || self.panel_app.user_settings_state.show_profile_manager)
            && self.panel_app.user_settings_state.e2e_passphrase_focused
        {
            let editing = &mut self.panel_app.user_settings_state.e2e_passphrase_editing;
            match ch {
                '\r' | '\n' => {
                    // Enter submits the passphrase — only when on an
                    // appropriate page (unlock / create-passphrase / settings
                    // sync tab / wizard).  On the ProfileList or CreateNew
                    // pages the passphrase field should not be active, but
                    // guard against stale focus just in case.
                    use zengeld_chart::ui::modal_settings::ProfileManagerPage;
                    let on_passphrase_page = if self.panel_app.user_settings_state.show_profile_manager {
                        matches!(
                            self.panel_app.user_settings_state.profile_manager_page,
                            ProfileManagerPage::UnlockPassphrase | ProfileManagerPage::CreatePassphrase
                        )
                    } else {
                        true // settings sync tab, wizard, vault_unlock — always valid
                    };

                    let passphrase = editing.text.trim().to_string();
                    if !passphrase.is_empty() && on_passphrase_page {
                        self.pending_updater_cmd = Some(format!("e2e_setup:{}", passphrase));
                        editing.text.clear();
                        editing.cursor = 0;
                        editing.selection_start = None;
                    }
                    self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                }
                '\x09' => {
                    // Tab moves to next field in wizard.
                    if self.panel_app.user_settings_state.show_welcome_wizard {
                        self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                        self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    }
                }
                '\x1b' => {
                    // Escape unfocuses without submitting
                    self.panel_app.user_settings_state.e2e_passphrase_focused = false;
                }
                '\x08' => {
                    // Backspace — clear the error so the user knows they can retry.
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.char_to_byte_pos(editing.cursor);
                        let byte_start = editing.char_to_byte_pos(editing.cursor - 1);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    // Any character typed — clear the error so the user can retry.
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.char_to_byte_pos(editing.cursor);
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }

        // Handle recovery key input in the UseRecoveryKey profile manager page
        if self.panel_app.user_settings_state.show_profile_manager
            && self.panel_app.user_settings_state.recovery_key_focused
        {
            let editing = &mut self.panel_app.user_settings_state.recovery_key_editing;
            match ch {
                '\r' | '\n' => {
                    // Enter submits the recovery key if long enough.
                    let key_text = editing.text.clone();
                    if key_text.len() >= 40 {
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.pending_updater_cmd = Some(format!("recovery_unlock:{}", key_text));
                        eprintln!("[ChartApp] profile_mgr: recovery unlock submitted via Enter");
                    }
                    self.panel_app.user_settings_state.recovery_key_focused = false;
                }
                '\x1b' => {
                    // Escape unfocuses without submitting
                    self.panel_app.user_settings_state.recovery_key_focused = false;
                }
                '\x08' => {
                    // Backspace — clear any error.
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.char_to_byte_pos(editing.cursor);
                        let byte_start = editing.char_to_byte_pos(editing.cursor - 1);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    // Any character typed — clear error.
                    self.panel_app.user_settings_state.vault_unlock_error = None;
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.char_to_byte_pos(editing.cursor);
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }

        // Handle new passphrase input in the SetNewPassphrase profile manager page
        if self.panel_app.user_settings_state.show_profile_manager
            && self.panel_app.user_settings_state.new_passphrase_focused
        {
            let editing = &mut self.panel_app.user_settings_state.new_passphrase_editing;
            match ch {
                '\r' | '\n' => {
                    // Tab to next field (confirm passphrase).
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                    self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                }
                '\x1b' => {
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                }
                '\x08' => {
                    self.panel_app.user_settings_state.set_passphrase_error.clear();
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.char_to_byte_pos(editing.cursor);
                        let byte_start = editing.char_to_byte_pos(editing.cursor - 1);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    self.panel_app.user_settings_state.set_passphrase_error.clear();
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.char_to_byte_pos(editing.cursor);
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }

        // Handle confirm passphrase input in the SetNewPassphrase profile manager page or wizard page 2
        if (self.panel_app.user_settings_state.show_profile_manager || self.panel_app.user_settings_state.show_welcome_wizard)
            && self.panel_app.user_settings_state.confirm_passphrase_focused
        {
            // For SetNewPassphrase page, compare against new_passphrase_editing; for wizard/CreatePassphrase compare against e2e_passphrase_editing.
            let passphrase_text = if self.panel_app.user_settings_state.show_profile_manager
                && !self.panel_app.user_settings_state.new_passphrase_editing.text.is_empty()
            {
                self.panel_app.user_settings_state.new_passphrase_editing.text.clone()
            } else {
                self.panel_app.user_settings_state.e2e_passphrase_editing.text.clone()
            };
            let editing = &mut self.panel_app.user_settings_state.confirm_passphrase_editing;
            match ch {
                '\r' | '\n' => {
                    // Enter submits if passphrases match and meet min length.
                    let confirm_text = editing.text.clone();
                    use zengeld_chart::user_manager::profile_manager::MIN_PASSPHRASE_LENGTH;
                    if passphrase_text.len() >= MIN_PASSPHRASE_LENGTH
                        && confirm_text == passphrase_text
                    {
                        self.panel_app.user_settings_state.set_passphrase_error.clear();
                        self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                        self.pending_updater_cmd = Some(format!("set_new_passphrase:{}", passphrase_text));
                        eprintln!("[ChartApp] profile_mgr: set_new_passphrase submitted via Enter");
                    } else if confirm_text != passphrase_text {
                        self.panel_app.user_settings_state.set_passphrase_error =
                            "Passphrases do not match".to_string();
                    }
                }
                '\x09' => {
                    // Tab cycles to next/first field.
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                    if self.panel_app.user_settings_state.show_welcome_wizard {
                        self.panel_app.user_settings_state.new_profile_name_focused = true;
                    } else {
                        self.panel_app.user_settings_state.new_passphrase_focused = true;
                    }
                }
                '\x1b' => {
                    self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                }
                '\x08' => {
                    self.panel_app.user_settings_state.set_passphrase_error.clear();
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.char_to_byte_pos(editing.cursor);
                        let byte_start = editing.char_to_byte_pos(editing.cursor - 1);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    self.panel_app.user_settings_state.set_passphrase_error.clear();
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.char_to_byte_pos(editing.cursor);
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }

        // Handle profile rename text input
        if self.panel_app.user_settings_state.is_open
            && self.panel_app.user_settings_state.profile_rename_focused
        {
            let editing = &mut self.panel_app.user_settings_state.profile_rename_editing;
            match ch {
                '\r' | '\n' => {
                    // Enter submits the rename — fire the same action as clicking Save.
                    self.panel_app.user_settings_state.profile_rename_focused = false;
                    let new_name = editing.text.trim().to_string();
                    if !new_name.is_empty() {
                        self.panel_app.user_settings_state.profile_display_name = new_name.clone();
                        self.panel_app.user_settings_state.profile_rename_mode = false;
                        self.panel_app.user_settings_state.profile_rename_target_id = None;
                        self.pending_updater_cmd = Some(format!("profile_rename:{}", new_name));
                    }
                }
                '\x1b' => {
                    // Escape cancels.
                    self.panel_app.user_settings_state.profile_rename_focused = false;
                    self.panel_app.user_settings_state.profile_rename_mode = false;
                    self.panel_app.user_settings_state.profile_rename_target_id = None;
                }
                '\x08' => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.char_to_byte_pos(editing.cursor);
                        let byte_start = editing.char_to_byte_pos(editing.cursor - 1);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.char_to_byte_pos(editing.cursor);
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
                }
                _ => {}
            }
            return;
        }

        // Handle new profile name text input (in settings modal, profile manager, or welcome wizard)
        if (self.panel_app.user_settings_state.is_open || self.panel_app.user_settings_state.show_profile_manager || self.panel_app.user_settings_state.show_welcome_wizard)
            && self.panel_app.user_settings_state.new_profile_name_focused
        {
            let editing = &mut self.panel_app.user_settings_state.new_profile_name_editing;
            match ch {
                '\r' | '\n' => {
                    if self.panel_app.user_settings_state.show_welcome_wizard {
                        // In the wizard, Enter in the profile name field moves focus to passphrase.
                        self.panel_app.user_settings_state.new_profile_name_focused = false;
                        self.panel_app.user_settings_state.e2e_passphrase_focused = true;
                    } else {
                        // Enter submits the new profile creation.
                        self.panel_app.user_settings_state.new_profile_name_focused = false;
                        let name = editing.text.trim().to_string();
                        if !name.is_empty() {
                            self.pending_updater_cmd = Some(format!("profile_create:{}", name));
                            self.panel_app.user_settings_state.show_new_profile_dialog = false;
                        }
                    }
                }
                '\x09' => {
                    // Tab moves to next field.
                    if self.panel_app.user_settings_state.show_welcome_wizard {
                        self.panel_app.user_settings_state.new_profile_name_focused = false;
                        self.panel_app.user_settings_state.e2e_passphrase_focused = true;
                    }
                }
                '\x1b' => {
                    // Escape cancels (wizard handles its own Escape elsewhere).
                    self.panel_app.user_settings_state.new_profile_name_focused = false;
                    self.panel_app.user_settings_state.show_new_profile_dialog = false;
                }
                '\x08' => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    } else if editing.cursor > 0 {
                        let byte_end = editing.char_to_byte_pos(editing.cursor);
                        let byte_start = editing.char_to_byte_pos(editing.cursor - 1);
                        editing.text.drain(byte_start..byte_end);
                        editing.cursor -= 1;
                    }
                }
                c if !c.is_control() => {
                    if editing.has_selection() {
                        editing.delete_selection();
                    }
                    let byte_idx = editing.char_to_byte_pos(editing.cursor);
                    editing.text.insert(byte_idx, c);
                    editing.cursor += 1;
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
                                            PrimitiveText {
                                                content: text.clone(),
                                                ..PrimitiveText::default()
                                            }
                                        });
                                    }
                                    window.drawing_manager.set_data_at(idx, &data);
                                }
                            }
                        }
                        self.sync_drawing_back_to_group();
                        eprintln!("[ChartApp] prim_settings text_content committed: {}", text);
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
                                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                        let bars = window.bars.clone();
                                        window.drawing_manager.update_all_timestamps_from_bars(&bars);
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
                                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                        let bars = window.bars.clone();
                                        window.drawing_manager.update_all_timestamps_from_bars(&bars);
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
                        self.sync_drawing_back_to_group();
                        self.autosave_snapshot();
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
                                .map(|r| format!("{}:{}:{}", r.symbol, r.exchange_id, r.account_type))
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

        // While the profile manager is shown, only allow key events to reach the
        // passphrase, recovery key, or name input fields.  This prevents keyboard
        // shortcuts (Escape, arrow keys, etc.) from operating on the hidden chart UI.
        // Special case: when the read-only recovery key display box is focused, allow
        // SelectAll (Ctrl+A) through so the user can select all text.  Ctrl+C is
        // handled passively via on_copy_selection() without needing a key event.
        if self.panel_app.user_settings_state.show_profile_manager
            && !self.panel_app.user_settings_state.e2e_passphrase_focused
            && !self.panel_app.user_settings_state.new_profile_name_focused
            && !self.panel_app.user_settings_state.recovery_key_focused
            && !self.panel_app.user_settings_state.new_passphrase_focused
            && !self.panel_app.user_settings_state.confirm_passphrase_focused
        {
            // Allow Ctrl+A (SelectAll) to reach the recovery key display box.
            if self.panel_app.user_settings_state.recovery_key_display_focused
                && matches!(key, KeyPress::SelectAll) {
                    self.panel_app.user_settings_state.recovery_key_display_editing.select_all();
                    return;
                }
            return;
        }

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
                // ── PTY-only named keys — not meaningful in text fields ───────
                KeyPress::ArrowUp
                | KeyPress::ArrowDown
                | KeyPress::Enter
                | KeyPress::Escape
                | KeyPress::Tab
                | KeyPress::Backspace
                | KeyPress::CtrlC
                | KeyPress::PageUp
                | KeyPress::PageDown => false,
            }
        }

        // ── Profile rename text input key events ──────────────────────────────
        if self.panel_app.user_settings_state.is_open
            && self.panel_app.user_settings_state.profile_rename_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.profile_rename_editing, key);
            return;
        }

        // ── E2E passphrase text input key events (settings modal, wizard, vault unlock, or profile manager) ──
        if (self.panel_app.user_settings_state.is_open
            || self.panel_app.user_settings_state.show_welcome_wizard
            || self.panel_app.user_settings_state.needs_vault_unlock
            || self.panel_app.user_settings_state.show_profile_manager)
            && self.panel_app.user_settings_state.e2e_passphrase_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.e2e_passphrase_editing, key);
            return;
        }

        // ── New profile name text input key events (settings modal, profile manager, or welcome wizard) ──
        if (self.panel_app.user_settings_state.is_open || self.panel_app.user_settings_state.show_profile_manager || self.panel_app.user_settings_state.show_welcome_wizard)
            && self.panel_app.user_settings_state.new_profile_name_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.new_profile_name_editing, key);
            return;
        }

        // ── Recovery key text input key events (profile manager UseRecoveryKey page) ──
        if self.panel_app.user_settings_state.show_profile_manager
            && self.panel_app.user_settings_state.recovery_key_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.recovery_key_editing, key);
            return;
        }

        // ── New passphrase text input key events (profile manager SetNewPassphrase page) ──
        if self.panel_app.user_settings_state.show_profile_manager
            && self.panel_app.user_settings_state.new_passphrase_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.new_passphrase_editing, key);
            return;
        }

        // ── Confirm passphrase text input key events (profile manager or wizard) ──
        if (self.panel_app.user_settings_state.show_profile_manager || self.panel_app.user_settings_state.show_welcome_wizard)
            && self.panel_app.user_settings_state.confirm_passphrase_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.confirm_passphrase_editing, key);
            return;
        }

        // ── Hex color picker key routing ──────────────────────────────────────
        let hex_id = WidgetId::new(crate::text_input::HEX_COLOR);
        if self.input_coordinator.borrow().text_fields().is_focused(&hex_id) {
            if let Some(uzor_key) = to_uzor_key(&key) {
                let action = self.input_coordinator.borrow_mut().text_fields_mut().on_key(uzor_key);
                match action {
                    TextAction::Commit(_) | TextAction::Cancel => {
                        self.panel_app.primitive_settings_state.color_picker.hex_editing = false;
                        self.panel_app.indicator_settings_state.color_picker.hex_editing = false;
                        self.panel_app.chart_settings_state.color_picker.hex_editing = false;
                        self.panel_app.compare_settings_state.color_picker.hex_editing = false;
                        self.panel_app.panel_color_picker.hex_editing = false;
                        self.input_coordinator.borrow_mut().text_fields_mut().blur();
                    }
                    TextAction::TextChanged(ref new_hex) => {
                        let pickers: [(&mut zengeld_chart::ui::color_picker_state::ColorPickerState, &str); 5] = [
                            (&mut self.panel_app.primitive_settings_state.color_picker, "primitive"),
                            (&mut self.panel_app.indicator_settings_state.color_picker, "indicator"),
                            (&mut self.panel_app.chart_settings_state.color_picker, "chart"),
                            (&mut self.panel_app.compare_settings_state.color_picker, "compare"),
                            (&mut self.panel_app.panel_color_picker, "panel"),
                        ];
                        for (picker, _src) in pickers {
                            if picker.hex_editing {
                                picker.hex_set_text(new_hex);
                                break;
                            }
                        }
                    }
                    TextAction::None => {}
                    TextAction::RawInput(_) => {
                        // RawInput is for AgentPty, not HexColor — ignore here.
                    }
                }
            }
            return;
        }

        // ── Agent PTY key routing — sends to focused leaf's PTY instance ─────────
        if self.is_agent_pty_focused() {
            if let Some(uzor_key) = to_uzor_key(&key) {
                let action = self.input_coordinator.borrow_mut().text_fields_mut().on_key(uzor_key);
                if let TextAction::RawInput(bytes) = action {
                    if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                        if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                            if desc.mode == gate4agent::InstanceMode::Pty {
                                let text = String::from_utf8_lossy(&bytes).to_string();
                                let id = desc.instance_id;
                                let _ = self.bridge.runtime().block_on(self.agent.write_pty_instance(id, &text));
                            }
                        }
                    }
                }
            }
            return;
        }

        // ── Agent chat key routing — syncs focused leaf's input buffer ────────
        let chat_id = WidgetId::new(crate::text_input::AGENT_CHAT);
        if self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
            if let Some(uzor_key) = to_uzor_key(&key) {
                let _action = self.input_coordinator.borrow_mut().text_fields_mut().on_key(uzor_key);
            }
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                let new_text = {
                    let coord = self.input_coordinator.borrow();
                    coord.text_fields().text(&chat_id).to_string()
                };
                self.sidebar_state.agent_input_buffers.insert(leaf_id, new_text);
            }
            return;
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
            self.panel_app.chart_browser.scroll.reset();
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
                self.watchlist_modal.scroll.reset();
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

        // ── Trading panel keyboard routing ───────────────────────────────────
        // Route key events to whichever order-flow panel is currently hovered
        // (identified by the `slot:{idx}:leaf:{id}:focus_content` widget).
        // Panels return `false` until they implement actual hotkeys, at which
        // point returning `true` stops propagation here.
        {
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.0.clone());
            if let Some(ref wid) = hovered_wid {
                if let Some(rest) = wid.strip_prefix("slot:") {
                    if let Some((slot_str, leaf_rest)) = rest.split_once(":leaf:") {
                        if let Some(leaf_id_str) = leaf_rest.strip_suffix(":focus_content") {
                            if let (Ok(slot_idx), Ok(raw)) =
                                (slot_str.parse::<usize>(), leaf_id_str.parse::<u64>())
                            {
                                if slot_idx < 4 {
                                    let leaf_id = uzor::panels::LeafId(raw);
                                    let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                        .inner()
                                        .tree()
                                        .leaf(leaf_id)
                                        .and_then(|l| l.active_panel().cloned());
                                    use sidebar_content::free_slot::FreeItem;
                                    // Convert KeyPress to zengeld_chart::input::KeyCode for panels.
                                    // Most KeyPress variants map to specific KeyCode values; the
                                    // ones with no equivalent are passed as KeyCode::Other.
                                    let panel_key = match &key {
                                        KeyPress::ArrowUp    => zengeld_chart::input::KeyCode::ArrowUp,
                                        KeyPress::ArrowDown  => zengeld_chart::input::KeyCode::ArrowDown,
                                        KeyPress::ArrowLeft  => zengeld_chart::input::KeyCode::ArrowLeft,
                                        KeyPress::ArrowRight => zengeld_chart::input::KeyCode::ArrowRight,
                                        KeyPress::PageUp     => zengeld_chart::input::KeyCode::PageUp,
                                        KeyPress::PageDown   => zengeld_chart::input::KeyCode::PageDown,
                                        KeyPress::Home       => zengeld_chart::input::KeyCode::Home,
                                        KeyPress::End        => zengeld_chart::input::KeyCode::End,
                                        KeyPress::Enter      => zengeld_chart::input::KeyCode::Enter,
                                        KeyPress::Escape     => zengeld_chart::input::KeyCode::Escape,
                                        KeyPress::Tab        => zengeld_chart::input::KeyCode::Tab,
                                        KeyPress::Backspace  => zengeld_chart::input::KeyCode::Backspace,
                                        KeyPress::Delete     => zengeld_chart::input::KeyCode::Delete,
                                        _                    => zengeld_chart::input::KeyCode::Unknown,
                                    };
                                    let _consumed = match item_opt {
                                        Some(FreeItem::Dom(pid)) => {
                                            self.panels_store.dom.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(FreeItem::L2Tape(pid)) => {
                                            self.panels_store.l2_tape.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(FreeItem::TradeTape(pid)) => {
                                            self.panels_store.trade_tape.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(FreeItem::Footprint(pid)) => {
                                            self.panels_store.footprint.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(FreeItem::LiquidityHeatmap(pid)) => {
                                            self.panels_store.liquidity_heatmap.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(FreeItem::BigTrades(pid)) => {
                                            self.panels_store.big_trades.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(FreeItem::VolumeProfile(pid)) => {
                                            self.panels_store.volume_profile.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        _ => false,
                                    };
                                }
                            }
                        }
                    }
                }
            }
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

        // Text-field store handles HexColor, AgentPty, AgentChat selections.
        if let Some(text) = self.input_coordinator.borrow().text_fields().copy_selection() {
            return Some(text);
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

        // Profile manager text fields
        {
            let uss = &self.panel_app.user_settings_state;
            if let Some(text) = get_selection(&uss.e2e_passphrase_editing) {
                return Some(text);
            }
            if let Some(text) = get_selection(&uss.new_profile_name_editing) {
                return Some(text);
            }
            if let Some(text) = get_selection(&uss.recovery_key_editing) {
                return Some(text);
            }
            if let Some(text) = get_selection(&uss.new_passphrase_editing) {
                return Some(text);
            }
            if let Some(text) = get_selection(&uss.confirm_passphrase_editing) {
                return Some(text);
            }
            // Recovery key DISPLAY — read-only, but user can select and copy text.
            if let Some(text) = get_selection(&uss.recovery_key_display_editing) {
                return Some(text);
            }
        }

        // User settings profile rename field
        if let Some(text) = get_selection(&self.panel_app.user_settings_state.profile_rename_editing) {
            return Some(text);
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
                } else if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.drawing_manager.insert_at(*index, type_id, points, data);
                }
            }
            Command::DeletePrimitive { index, .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if *index < group.primitives.len() {
                            group.primitives.remove(*index);
                        }
                    }
                } else if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.drawing_manager.delete_at(*index);
                }
            }
            Command::DeleteAllPrimitives { .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        group.primitives.clear();
                    }
                } else if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.drawing_manager.clear();
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
                } else if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.drawing_manager.clear();
                    for (i, (type_id, points, data)) in primitives.iter().enumerate() {
                        w.drawing_manager.insert_at(i, type_id, points, data);
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
                    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                        let bars = w.bars.clone();
                        w.drawing_manager.update_all_timestamps_from_bars(&bars);
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
                } else if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.drawing_manager.set_visibility_at(*index, *visible);
                }
            }
            Command::SetPrimitiveLock { index, locked, .. } => {
                if let Some(gid) = group_id {
                    if let Some(group) = self.panel_app.tag_manager.group_mut(gid) {
                        if let Some(prim) = group.primitives.get_mut(*index) {
                            prim.data_mut().locked = *locked;
                        }
                    }
                } else if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.drawing_manager.set_lock_at(*index, *locked);
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
                    self.sync_drawing_back_to_group();
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
                } else if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
                    w.drawing_manager.move_to_index(*old_index, *new_index);
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
                    // Re-add to pre_tag_indicator_ids for Object Tree visibility.
                    // indicator_manager borrows are released before this block.
                    if let Some(window) = self.panel_app.panel_grid.windows_mut().values_mut()
                        .find(|w| w.id == chart_id_val)
                    {
                        if !window.pre_tag_indicator_ids.contains(instance_id) {
                            window.pre_tag_indicator_ids.push(*instance_id);
                        }
                    }
                    self.sync_sub_panes_from_manager();
                    eprintln!("[Undo/Redo] Re-created indicator {} (id={}) window_id={}", type_id, instance_id, chart_id_val.0);
                } else {
                    eprintln!("[Undo/Redo] Failed to re-create indicator {} (id={})", type_id, instance_id);
                }
            }
            Command::RemoveIndicator { instance_id, .. } => {
                if self.indicator_manager.remove_instance(*instance_id).is_some() {
                    // Remove stale entries so the Object Tree does not show phantom items.
                    let removed_id = *instance_id;
                    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
                        window.pre_tag_indicator_ids.retain(|iid| *iid != removed_id);
                    }
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

        // Build the set of visible sub-pane instance IDs from indicator_manager
        // (single source of truth for visibility).  Collect before borrowing
        // active_window() again so the borrow checker stays happy.
        let visible_set: std::collections::HashSet<u64> = {
            let symbol = self.panel_app.panel_grid.active_window()
                .map(|w| w.symbol.clone())
                .unwrap_or_default();
            let chart_id = self.panel_app.panel_grid.active_chart_id()
                .map(|cid| cid.0);
            if let Some(cid) = chart_id {
                self.indicator_manager
                    .get_instances_for_symbol_in_window(&symbol, cid)
                    .into_iter()
                    .filter(|i| i.visible && i.pane > 0)
                    .map(|i| i.id)
                    .collect()
            } else {
                self.indicator_manager
                    .get_instances_for_symbol(&symbol)
                    .into_iter()
                    .filter(|i| i.visible && i.pane > 0)
                    .map(|i| i.id)
                    .collect()
            }
        };

        // Collect sub-pane instance IDs from the window's SubPane list,
        // filtering by indicator_manager visibility (the single source of truth).
        let sub_pane_ids: Vec<u64> = self.panel_app.panel_grid
            .active_window()
            .map(|win| {
                if let Some(maximized) = win.sub_panes.iter().find(|p| p.maximized && visible_set.contains(&p.instance_id)) {
                    vec![maximized.instance_id]
                } else {
                    win.sub_panes.iter()
                        .filter(|p| visible_set.contains(&p.instance_id))
                        .map(|p| p.instance_id)
                        .collect()
                }
            })
            .unwrap_or_default();

        let maximized_instance_id: Option<u64> = self.panel_app.panel_grid
            .active_window()
            .and_then(|win| win.sub_panes.iter().find(|p| p.maximized && sub_pane_ids.contains(&p.instance_id)))
            .map(|p| p.instance_id);

        // Build per-pane heights from the active window's SubPane list so that
        // hit-testing coordinates exactly match what render_full_chart_panel produced.
        // When maximized, compute_from_chart_panel ignores these and uses full available height.
        let sub_pane_heights: Vec<f64> = self.panel_app.panel_grid
            .active_window()
            .map(|win| {
                sub_pane_ids.iter().map(|&id| {
                    let ratio = win.sub_panes.iter()
                        .find(|p| p.instance_id == id)
                        .map(|p| p.height_ratio)
                        .unwrap_or(0.0);
                    if ratio <= 0.0 { 100.0 } else { (ratio as f64 * content_rect.height).max(30.0) }
                }).collect()
            })
            .unwrap_or_else(|| {
                zengeld_chart::default_sub_pane_heights(sub_pane_ids.len(), 100.0)
            });
        let above_main_flags: Vec<bool> = self.panel_app.panel_grid
            .active_window()
            .map(|win| {
                sub_pane_ids.iter().map(|&id| {
                    win.sub_panes.iter()
                        .find(|p| p.instance_id == id)
                        .map(|p| p.above_main)
                        .unwrap_or(false)
                }).collect()
            })
            .unwrap_or_else(|| vec![false; sub_pane_ids.len()]);
        ExtendedFrameLayout::compute_from_chart_panel(
            &content_rect,
            &sub_pane_ids,
            &scale_settings,
            &sub_pane_heights,
            1.0, // separator_height
            maximized_instance_id,
            &above_main_flags,
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

        // Build visible set from indicator_manager (single source of truth).
        let visible_set: std::collections::HashSet<u64> = {
            let chart_id = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id);
            if let Some(cid) = chart_id {
                self.indicator_manager
                    .get_instances_for_symbol_in_window(&window.symbol, cid.0)
                    .into_iter()
                    .filter(|i| i.visible && i.pane > 0)
                    .map(|i| i.id)
                    .collect()
            } else {
                self.indicator_manager
                    .get_instances_for_symbol(&window.symbol)
                    .into_iter()
                    .filter(|i| i.visible && i.pane > 0)
                    .map(|i| i.id)
                    .collect()
            }
        };

        // Collect sub-pane IDs filtered by indicator_manager visibility.
        let sub_pane_ids: Vec<u64> = if let Some(maximized) = window.sub_panes.iter().find(|p| p.maximized && visible_set.contains(&p.instance_id)) {
            vec![maximized.instance_id]
        } else {
            window.sub_panes.iter()
                .filter(|p| visible_set.contains(&p.instance_id))
                .map(|p| p.instance_id)
                .collect()
        };

        let maximized_instance_id: Option<u64> = window.sub_panes.iter()
            .find(|p| p.maximized && sub_pane_ids.contains(&p.instance_id))
            .map(|p| p.instance_id);

        // Build per-pane heights matching sub_pane_ids order (filtered by hidden/maximized).
        // When maximized, compute_from_chart_panel ignores these and uses full available height.
        let sub_pane_heights: Vec<f64> = sub_pane_ids.iter().map(|&id| {
            let ratio = window.sub_panes.iter()
                .find(|p| p.instance_id == id)
                .map(|p| p.height_ratio)
                .unwrap_or(0.0);
            if ratio <= 0.0 { 100.0 } else { (ratio as f64 * leaf_rect.height).max(30.0) }
        }).collect();
        let above_main_flags: Vec<bool> = sub_pane_ids.iter().map(|&id| {
            window.sub_panes.iter()
                .find(|p| p.instance_id == id)
                .map(|p| p.above_main)
                .unwrap_or(false)
        }).collect();
        Some(ExtendedFrameLayout::compute_from_chart_panel(
            leaf_rect,
            &sub_pane_ids,
            scale_settings,
            &sub_pane_heights,
            1.0, // separator_height
            maximized_instance_id,
            &above_main_flags,
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
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let bars = window.bars.clone();
                            window.drawing_manager.update_all_timestamps_from_bars(&bars);
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
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            let bars = window.bars.clone();
                            window.drawing_manager.update_all_timestamps_from_bars(&bars);
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
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
                    let rect = self.input_coordinator.borrow_mut().widget_rect(&uzor::input::WidgetId(widget_id_str));
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
                let widget_id_str = "ilb:inline:color".to_string();
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

                let widget_id_str = "ilb:inline:text_color".to_string();
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
                self.sync_drawing_back_to_group();
                if let Some(pid) = prim_id {
                    self.alert_manager.remove_alerts_for_drawing(pid);
                }
                self.autosave_snapshot();
            }

            // ── Lock toggle ──────────────────────────────────────────────────
            "inline:lock" => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.toggle_selected_lock();
                    eprintln!("[ChartApp] inline: toggled lock on selected primitive");
                }
                self.sync_drawing_back_to_group();
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
                    self.sync_drawing_back_to_group();
                    eprintln!("[ChartApp] inline: cycled line style to {}", style_str);
                }
                self.snapshot_primitive_settings_to_user_manager(idx);
            }

            // ── Width cycle ──────────────────────────────────────────────────
            "inline:width" => {
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    window.drawing_manager.increase_selected_width();
                    eprintln!("[ChartApp] inline: increased line width");
                }
                self.sync_drawing_back_to_group();
                self.snapshot_primitive_settings_to_user_manager(idx);
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
                        self.sync_drawing_back_to_group();
                        eprintln!("[ChartApp] inline: set line style to {}", style_str);
                    }
                }
                self.snapshot_primitive_settings_to_user_manager(idx);
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
                        self.sync_drawing_back_to_group();
                        eprintln!("[ChartApp] inline: set line width to {}", new_width);
                    }
                }
                self.snapshot_primitive_settings_to_user_manager(idx);
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
                // Parse composite key "SYMBOL:exchange:account_type" — split from the
                // right twice so that symbols containing ':' are handled correctly.
                let (sym_part, exchange_part, wl_at_label) =
                    if let Some(at_pos) = composite.rfind(':') {
                        let at_part = &composite[at_pos + 1..];
                        let remainder = &composite[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (
                                &remainder[..ex_pos],
                                &remainder[ex_pos + 1..],
                                at_part.to_string(),
                            )
                        } else {
                            // Only one colon — treat as "SYMBOL:exchange", look up account_type.
                            let at = self.sidebar_state.watchlist_manager.active_list()
                                .and_then(|list| {
                                    list.all_symbols().iter()
                                        .find(|ws| ws.symbol == remainder && ws.exchange == at_part)
                                        .map(|ws| ws.account_type.clone())
                                })
                                .unwrap_or_else(|| "S".to_string());
                            (remainder, at_part, at)
                        }
                    } else {
                        (composite, self.active_exchange.as_str(), "S".to_string())
                    };
                // Resolve ExchangeId from string.
                let resolved_exchange = self.exchange_symbols
                    .keys()
                    .find(|eid| eid.as_str() == exchange_part)
                    .copied()
                    .unwrap_or(self.active_exchange);
                // Switch the active chart to this symbol+exchange.
                // Capture old trade-stream identity BEFORE mutating window state.
                let old_trade_exchange = self.active_exchange;
                let old_trade_symbol = self.panel_app.panel_grid.active_window()
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let old_trade_at = self.panel_app.panel_grid.active_window()
                    .map(|w| crate::account_type_from_label(&w.account_type))
                    .unwrap_or(digdigdig3::AccountType::Spot);
                let timeframe = self.panel_app.panel_grid.active_window()
                    .map(|w| w.timeframe.clone())
                    .unwrap_or_default();
                if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                    let old_sym = window.symbol.clone();
                    let old_exchange = window.exchange.clone();
                    let old_account_type = window.account_type.clone();
                    window.snapshot_drawings_for_symbol(&old_sym, &old_exchange, &old_account_type);
                    window.symbol = sym_part.to_string();
                    window.exchange = exchange_part.to_string();
                    window.account_type = wl_at_label.clone();
                    window.update_title();
                    window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
                    window.bars.clear();
                    window.viewport.bar_count = 0;
                    window.pending_symbol_load = true;
                    window.drawing_manager.clear_all_primitives();
                    window.restore_drawings_for_symbol(sym_part, exchange_part, &wl_at_label);
                }
                self.active_exchange = resolved_exchange;
                // Unsubscribe only the old symbol's trade stream, leaving other
                // windows' streams intact.
                if !old_trade_symbol.is_empty() {
                    self.bridge.unsubscribe_trades(old_trade_exchange, &old_trade_symbol, old_trade_at);
                }
                let eid_str = resolved_exchange.as_str();
                if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                    eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (watchlist modal item click)", eid_str);
                } else {
                    let wl_at = crate::account_type_from_label(&wl_at_label);
                    self.bridge.ensure_connector(resolved_exchange);
                    self.bridge.request_bars(resolved_exchange, sym_part, &timeframe, wl_at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), false);
                }
                self.autosave_snapshot();
                eprintln!("[WatchlistModal] symbol selected: {} @ {}", sym_part, exchange_part);
                self.watchlist_modal.close();
            }
            _ if rest.starts_with("delete:") => {
                let key = &rest["delete:".len()..];
                // Parse composite key "SYMBOL:exchange:account_type" — split from the
                // right twice so that symbols containing ':' are handled correctly.
                let (symbol, exchange_owned, account_type_owned): (&str, String, String) =
                    if let Some(at_pos) = key.rfind(':') {
                        let at_part = &key[at_pos + 1..];
                        let remainder = &key[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (
                                &remainder[..ex_pos],
                                remainder[ex_pos + 1..].to_string(),
                                at_part.to_string(),
                            )
                        } else {
                            // Only one colon — "SYMBOL:exchange", look up account_type.
                            let at = self.sidebar_state.watchlist_manager.active_list()
                                .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == remainder && ws.exchange == at_part).map(|ws| ws.account_type.clone()))
                                .unwrap_or_default();
                            (remainder, at_part.to_string(), at)
                        }
                    } else {
                        // No colon — plain symbol, look up exchange + account_type.
                        let ex = self.sidebar_state.watchlist_manager.active_list()
                            .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == key).map(|ws| ws.exchange.clone()))
                            .unwrap_or_else(|| self.active_exchange.as_str().to_string());
                        let at = self.sidebar_state.watchlist_manager.active_list()
                            .and_then(|l| l.all_symbols().iter().find(|ws| ws.symbol == key).map(|ws| ws.account_type.clone()))
                            .unwrap_or_default();
                        (key, ex, at)
                    };
                let exchange = exchange_owned.as_str();
                let account_type = account_type_owned.as_str();
                // Remove symbol from snapshot (if active) before removing from list.
                if let Some(list) = self.sidebar_state.watchlist_manager.active_list_mut() {
                    if let Some(ref mut snap) = list.order_snapshot {
                        snap.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == account_type));
                    }
                }
                self.sidebar_state.watchlist_manager.remove_symbol(symbol, exchange, account_type);
                self.watchlist_actions.push(crate::WatchlistAction::Remove { symbol: symbol.to_string(), exchange: exchange.to_string(), account_type: account_type.to_string() });
                self.watchlists_dirty = true;
                self.persist_watchlists();
                eprintln!("[WatchlistModal] symbol removed: {}:{}:{}", symbol, exchange, account_type);
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
        // Clear orphan pre_tag_indicator_ids for every window so the Object Tree
        // does not show phantom items after a bulk indicator replace.
        for window in self.panel_app.panel_grid.windows_mut().values_mut() {
            window.pre_tag_indicator_ids.retain(|iid| !existing_ids.contains(iid));
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
        self.sidebar_data_dirty = true;
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
                // Composite key format: "SYMBOL:exchange_id:account_type" — split from the
                // right twice so that symbols containing ':' (e.g. "tETH2X:USD") are handled
                // correctly.
                let (sym_part, exchange_part, at_label) =
                    if let Some(at_pos) = composite.rfind(':') {
                        let at_part = &composite[at_pos + 1..];
                        let remainder = &composite[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (&remainder[..ex_pos], &remainder[ex_pos + 1..], at_part.to_string())
                        } else {
                            (remainder, self.active_exchange.as_str(), at_part.to_string())
                        }
                    } else {
                        (composite, self.active_exchange.as_str(), "S".to_string())
                    };
                // Queue action for App to apply on AppState (single source of truth).
                self.watchlist_actions.push(crate::WatchlistAction::Toggle {
                    symbol: sym_part.to_string(),
                    exchange: exchange_part.to_string(),
                    account_type: at_label,
                });
                self.watchlists_dirty = true;
                eprintln!("[ChartApp] star toggle queued: {}:{}", sym_part, exchange_part);
            }
            _ if rest.starts_with("item:") => {
                let item_id = &rest["item:".len()..];
                eprintln!("[ChartApp] search modal item selected: {}", item_id);

                // Parse composite key "SYMBOL:exchange_id:account_type" — split from the
                // right twice so that symbols containing ':' (e.g. "tETH2X:USD") are handled
                // correctly.
                let (symbol_part, exchange_id_part, item_at_label) =
                    if let Some(at_pos) = item_id.rfind(':') {
                        let at_part = &item_id[at_pos + 1..];
                        let remainder = &item_id[..at_pos];
                        if let Some(ex_pos) = remainder.rfind(':') {
                            (
                                &remainder[..ex_pos],
                                &remainder[ex_pos + 1..],
                                at_part.to_string(),
                            )
                        } else {
                            // Only one colon — treat as "SYMBOL:exchange", no account_type.
                            (remainder, at_part, "S".to_string())
                        }
                    } else {
                        // No colon — treat the whole string as the symbol and fall back to
                        // the current active exchange.
                        (item_id, "", "S".to_string())
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
                        // Capture previous symbol, exchange, and account_type BEFORE changing.
                        let previous_symbol = self.panel_app.panel_grid.active_window()
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        let old_trade_exchange = self.active_exchange;
                        let old_trade_at = self.panel_app.panel_grid.active_window()
                            .map(|w| crate::account_type_from_label(&w.account_type))
                            .unwrap_or(digdigdig3::AccountType::Spot);
                        let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
                        let new_symbol_str = symbol_part.to_string();
                        // Set symbol, clear bars, and request data asynchronously.
                        let timeframe = self.panel_app.panel_grid.active_window()
                            .map(|w| w.timeframe.clone())
                            .unwrap_or_default();
                        // account_type is embedded in the item key — use it directly.
                        let search_at_label = item_at_label.clone();
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            // Snapshot current drawings before switching symbol
                            let old_sym = window.symbol.clone();
                            let old_exchange = window.exchange.clone();
                            let old_account_type = window.account_type.clone();
                            window.snapshot_drawings_for_symbol(&old_sym, &old_exchange, &old_account_type);
                            window.symbol = symbol_part.to_string();
                            window.exchange = resolved_exchange.as_str().to_string();
                            window.account_type = search_at_label.clone();
                            window.update_title();
                            window.drawing_manager.set_current_symbol_key(&window.symbol, &window.exchange, &window.account_type);
                            window.bars.clear();
                            window.viewport.bar_count = 0;
                            window.viewport.view_start = 0.0;
                            window.pending_symbol_load = true;
                            window.drawing_manager.clear_all_primitives();
                            window.restore_drawings_for_symbol(symbol_part, resolved_exchange.as_str(), &search_at_label);
                        }
                        // Switch to the exchange that owns this symbol and request bars.
                        self.active_exchange = resolved_exchange;
                        // Unsubscribe only the old symbol's trade stream, leaving other
                        // windows' streams intact.
                        if !previous_symbol.is_empty() {
                            self.bridge.unsubscribe_trades(old_trade_exchange, &previous_symbol, old_trade_at);
                        }
                        let eid_str = resolved_exchange.as_str();
                        if !self.sidebar_state.connector_enabled.get(eid_str).copied().unwrap_or(true) {
                            eprintln!("[ChartApp] Exchange {} is disabled, skipping connector call (search symbol select)", eid_str);
                        } else {
                            let search_at = crate::account_type_from_label(&search_at_label);
                            self.bridge.ensure_connector(resolved_exchange);
                            self.bridge.request_bars(resolved_exchange, symbol_part, &timeframe, search_at, None, Some(self.panel_app.user_manager.profile.bar_count as usize), false);
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
                        self.sidebar_data_dirty = true;
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
                                series: series.without_bars(),
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
                        // Determine auto-color for overlay duplicates BEFORE create_instance
                        // (borrowck: create_instance needs &mut, query needs &).
                        let auto_color: Option<String> = self
                            .panel_app
                            .panel_grid
                            .docking()
                            .active_leaf()
                            .and_then(|leaf| self.panel_app.panel_grid.chart_id_for_leaf(leaf))
                            .and_then(|cid| {
                                self.indicator_manager.pick_overlay_color(item_id, &symbol, cid.0)
                            });
                        if let Some(new_id) = self.indicator_manager.create_instance(item_id, &symbol) {
                            // Apply auto-color when this is a duplicate overlay indicator.
                            if let Some(ref color) = auto_color {
                                if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                                    for oc in inst.outputs.values_mut() {
                                        oc.color = Some(color.clone());
                                    }
                                }
                            }
                            // Set window_id for the new instance so it's scoped to this window.
                            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                                if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                                    if let Some(inst) = self.indicator_manager.get_instance_mut(new_id) {
                                        inst.window_id = Some(chart_id.0);
                                    }
                                    // If this window is in a sync group and indicators are synced,
                                    // track the config in the group and push to peers.
                                    if let Some(group_id) = self.panel_app.panel_grid
                                        .window_for_leaf(active_leaf)
                                        .and_then(|w| w.group_id)
                                    {
                                        let sync_on = self.panel_app.tag_manager.group(group_id)
                                            .map(|g| g.sync_flags.sync_indicators)
                                            .unwrap_or(true);
                                        if sync_on {
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
                                        } else {
                                            // sync_indicators is OFF — this indicator belongs to this
                                            // window only. Record it in pre_tag_indicator_ids so that
                                            // if the window is later desynced (untagged), the desync
                                            // logic does NOT treat it as a group indicator and delete it.
                                            if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(active_leaf) {
                                                window.pre_tag_indicator_ids.push(new_id);
                                                eprintln!(
                                                    "[TagManager] sync_indicators=off: recorded id={} in pre_tag_indicator_ids for leaf {:?}",
                                                    new_id, active_leaf
                                                );
                                            }
                                        }
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
                            self.sidebar_data_dirty = true;
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
                        window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                    }
                    if next_mode.is_auto_y() {
                        window.calc_auto_scale();
                    }
                    let is_auto = next_mode.is_auto_y();
                    for sp in &mut window.sub_panes {
                        // update_sub_pane_ranges() already bakes in symmetrization
                        // and 5% padding, so price_min/price_max already match
                        // what is displayed.  Just flip the flag.
                        sp.auto_scale = is_auto;
                    }
                }
                // Propagate scale_mode change to sync-group peers.
                if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    let viewport_state = self.panel_app.panel_grid
                        .active_window()
                        .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                    if let Some((view_start, bar_spacing)) = viewport_state {
                        self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, Some(next_mode));
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

        // 2. Sub-pane overlay button check — clicks on delete/hide/move-up/expand
        //    buttons are handled here before any other canvas logic.
        //
        //    IMPORTANT: `pane_index` from hit_test is the index in overlay_results
        //    (which may be filtered, e.g. only the maximized pane).  We resolve
        //    the actual sub-pane via `instance_id` from the overlay result.
        {
            let overlay_results_btn = self.panel_app.panel_grid.active_window()
                .map(|w| w.sub_pane_overlay_results.clone())
                .unwrap_or_default();
            let extended_btn = self.build_extended_layout();
            let tester_btn = ExtendedLayoutHitTester::new(&extended_btn)
                .with_overlays(&overlay_results_btn);
            use zengeld_chart::input::ChartHitTester;
            if let zengeld_chart::engine::input::HitResult::SubPaneOverlayButton { pane_index, button } = tester_btn.hit_test(x, y) {
                // Resolve instance_id from overlay results (correct even when maximized).
                let instance_id = overlay_results_btn.get(pane_index)
                    .map(|o| o.instance_id)
                    .unwrap_or(0);
                if instance_id == 0 { return; }

                // Find the real index in window.sub_panes by instance_id.
                let real_index = self.panel_app.panel_grid.active_window()
                    .and_then(|w| w.sub_panes.iter().position(|p| p.instance_id == instance_id));

                use zengeld_chart::ui::modal_settings::SubPaneButton;
                match button {
                    SubPaneButton::Delete => {
                        self.delete_indicator_instance(instance_id);
                    }
                    SubPaneButton::Hide => {
                        // Single source of truth: instance.visible controls
                        // both overlay and sub-pane indicator visibility.
                        // sub_pane.hidden is not used — render filters by instance.visible.
                        self.indicator_manager.toggle_visibility(instance_id);
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                    SubPaneButton::MoveUp => {
                        if let Some(idx) = real_index {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                let len = window.sub_panes.len();
                                if idx < len {
                                    let is_above = window.sub_panes[idx].above_main;
                                    if is_above {
                                        // Already above main — swap within above group if possible.
                                        if idx > 0 {
                                            window.sub_panes.swap(idx, idx - 1);
                                        }
                                    } else {
                                        // Below main: check if this is the first below-main pane.
                                        let first_below = window.sub_panes[..idx]
                                            .iter()
                                            .all(|p| p.above_main);
                                        if first_below {
                                            // Promote to above-main: set flag and move to end of
                                            // above group (last above = closest to main chart).
                                            window.sub_panes[idx].above_main = true;
                                            let above_end = window.sub_panes[..idx]
                                                .iter()
                                                .filter(|p| p.above_main)
                                                .count();
                                            // above_end is the count of above panes before idx;
                                            // since we just set [idx].above_main = true the pane
                                            // should sit at position above_end (end of the above
                                            // group).  Rotate it from idx down to above_end.
                                            window.sub_panes[above_end..=idx].rotate_right(1);
                                        } else {
                                            // Normal swap within below group.
                                            window.sub_panes.swap(idx, idx - 1);
                                        }
                                    }
                                    for (i, pane) in window.sub_panes.iter_mut().enumerate() {
                                        pane.index = i;
                                    }
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::MoveDown => {
                        if let Some(idx) = real_index {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                let len = window.sub_panes.len();
                                if idx < len {
                                    let is_above = window.sub_panes[idx].above_main;
                                    if !is_above {
                                        // Already below main — swap within below group if possible.
                                        if idx + 1 < len {
                                            window.sub_panes.swap(idx, idx + 1);
                                        }
                                    } else {
                                        // Above main: check if this is the last above-main pane.
                                        let last_above = window.sub_panes[idx + 1..]
                                            .iter()
                                            .all(|p| !p.above_main);
                                        if last_above {
                                            // Demote to below-main: set flag and move to position 0
                                            // in the below group (first below = closest to main
                                            // chart).
                                            window.sub_panes[idx].above_main = false;
                                            // Count how many above-main panes are at positions
                                            // 0..idx (they stay in place).  The pane should move
                                            // to position idx (first below slot) — it's already
                                            // there since all after idx are below-main, so a
                                            // rotate_left on [idx..] places it at the start of the
                                            // below group.
                                            // Actually: positions [idx+1..] are all below_main,
                                            // so rotating [idx..] left by 1 moves idx to end.
                                            // We want it at position idx (start of below group),
                                            // so no rotate needed — it's already at the boundary.
                                        } else {
                                            // Normal swap within above group.
                                            window.sub_panes.swap(idx, idx + 1);
                                        }
                                    }
                                    for (i, pane) in window.sub_panes.iter_mut().enumerate() {
                                        pane.index = i;
                                    }
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::Expand => {
                        if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                            for pane in window.sub_panes.iter_mut() {
                                if pane.instance_id == instance_id {
                                    pane.pre_maximize_height_ratio = pane.height_ratio;
                                    pane.maximized = true;
                                } else {
                                    pane.maximized = false;
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::Restore => {
                        if let Some(idx) = real_index {
                            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                                if let Some(sub_pane) = window.sub_panes.get_mut(idx) {
                                    sub_pane.maximized = false;
                                    sub_pane.height_ratio = sub_pane.pre_maximize_height_ratio;
                                }
                            }
                        }
                        self.autosave_snapshot();
                    }
                    SubPaneButton::IndicatorEye => {
                        self.indicator_manager.toggle_visibility(instance_id);
                        self.sidebar_data_dirty = true;
                        self.autosave_snapshot();
                    }
                    SubPaneButton::IndicatorAlert => {
                        // TODO: open alert creation for this indicator
                    }
                    SubPaneButton::IndicatorSettings => {
                        self.panel_app.indicator_settings_state.open(instance_id);
                    }
                    SubPaneButton::IndicatorDelete => {
                        self.delete_indicator_instance(instance_id);
                    }
                }
                return;
            }
        }

        // 3. Drawing tool check — if a tool is active, convert the click to
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
                eprintln!("[CANVAS_CLICK] screen=({:.0},{:.0}) -> bar={:.2}, price={:.12e}, price_range=[{:.12e}..{:.12e}]", x, y, bar, price, price_min, price_max);
                let primitive_created = self.panel_app.panel_grid.active_window_mut()
                    .map(|w| w.drawing_manager.on_click(bar, price))
                    .unwrap_or(false);

                if primitive_created {
                    // Populate point_timestamps immediately so TF switching keeps
                    // the primitive anchored to the correct time position.
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let bars = window.bars.clone();
                        window.drawing_manager.update_all_timestamps_from_bars(&bars);
                    }
                    let prim_count = self.panel_app.panel_grid.active_window()
                        .map(|w| w.drawing_manager.primitives().len()).unwrap_or(0);
                    eprintln!("[ChartApp] Primitive created (main) at bar={:.2}, price={:.12e}, total_primitives={}", bar, price, prim_count);
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
                    self.sidebar_data_dirty = true;
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
                    // Populate point_timestamps immediately so TF switching keeps
                    // the primitive anchored to the correct time position.
                    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                        let bars = window.bars.clone();
                        window.drawing_manager.update_all_timestamps_from_bars(&bars);
                    }
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
                    self.sidebar_data_dirty = true;
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
                    // Use corrected viewport dimensions from the layout so that
                    // hit-test coordinate mapping matches the render path exactly.
                    let mut corrected_vp = window.viewport.clone();
                    corrected_vp.chart_width = chart_rect.width;
                    corrected_vp.chart_height = chart_rect.height;
                    if let Some(prim_idx) = window.drawing_manager.hit_test(
                        local_x, local_y, &corrected_vp, &window.price_scale,
                    ) {
                        // Select the primitive, clear indicator selection, and return.
                        self.selected_indicator_id = None;
                        // Deselect all other windows first so control points don't accumulate.
                        let active_cid = self.panel_app.panel_grid.active_chart_id();
                        for (cid, win) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                            if Some(*cid) != active_cid {
                                win.drawing_manager.deselect();
                            }
                        }
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
                        // Deselect all other windows first so control points don't accumulate.
                        let active_cid = self.panel_app.panel_grid.active_chart_id();
                        for (cid, win) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                            if Some(*cid) != active_cid {
                                win.drawing_manager.deselect();
                            }
                        }
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
                        for win in self.panel_app.panel_grid.windows_mut().values_mut() {
                            win.drawing_manager.deselect();
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
                    // Deselect any drawing primitive across ALL windows when an indicator is selected.
                    for win in self.panel_app.panel_grid.windows_mut().values_mut() {
                        win.drawing_manager.deselect();
                    }
                    eprintln!("[ChartApp] Indicator selected: id={}", ind_id);
                    return;
                }
            }
        }

        // No primitive or indicator hit — deselect everything across ALL windows.
        self.selected_indicator_id = None;
        for window in self.panel_app.panel_grid.windows_mut().values_mut() {
            window.drawing_manager.deselect();
        }

        let extended = self.build_extended_layout();
        let overlay_results_cc = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(&extended)
            .with_overlays(&overlay_results_cc);
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
                "profile_manager" => {
                    // In skeleton mode (vault unlock required), profile manager cannot be dismissed.
                    if !self.panel_app.user_settings_state.needs_vault_unlock {
                        self.panel_app.user_settings_state.show_profile_manager = false;
                        self.panel_app.user_settings_state.profile_manager_page =
                            zengeld_chart::ui::modal_settings::ProfileManagerPage::ProfileList;
                    }
                    eprintln!("[ChartApp] close_topmost_modal_layer: profile_manager (needs_vault_unlock={})", self.panel_app.user_settings_state.needs_vault_unlock);
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
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
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
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
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
                                let bars = w.bars.clone();
                                w.drawing_manager.update_all_timestamps_from_bars(&bars);
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
                            self.sync_drawing_back_to_group();
                            self.autosave_snapshot();
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
                        self.sidebar_data_dirty = true;
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
                        self.sidebar_data_dirty = true;
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
                    "reset_cache" => {
                        self.pending_reset_cache = true;
                        eprintln!("[ChartApp] Reset cache requested");
                    }
                    "reset_storage" => {
                        self.pending_reset_storage = true;
                        eprintln!("[ChartApp] Reset storage requested");
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
        self.sidebar_data_dirty = true;
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
        self.panel_app.panel_grid.reassign_active_chart_id();
        for w in self.panel_app.panel_grid.windows_mut().values_mut() {
            w.drawing_manager.clear_all_primitives();
        }
        self.panel_app.tag_manager.clear();
        self.indicator_manager.clear_all();
        self.panel_app.indicator_overlay_states.clear();
        self.panel_app.leaf_color_tags.clear();
        self.alert_manager.clear();
        self.sidebar_data_dirty = true;

        // Auto-create a default sync group for the single window so primitives
        // always go through the grouped path (standalone path is broken).
        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
            if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(active_leaf) {
                // Auto groups get transparent color — never occupy palette slots
                let (symbol, timeframe) = self.panel_app.panel_grid
                    .window_for_leaf(active_leaf)
                    .map(|w| (w.symbol.clone(), w.timeframe.clone()))
                    .unwrap_or_else(|| (
                        "BTCUSDT".to_string(),
                        zengeld_chart::state::Timeframe::h1(),
                    ));
                let group_id = self.panel_app.tag_manager.create_group_auto([0.0, 0.0, 0.0, 0.0], symbol, timeframe);
                let _ = self.panel_app.tag_manager.connect_chart(chart_id, group_id);
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(active_leaf) {
                    window.group_id = Some(group_id);
                }
                eprintln!("[ChartApp] Auto-created invisible sync group {:?} for new chart", group_id);
            }
        }

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
                values: (*inst.values).clone(),
            });
        }

        // Snapshot alerts
        preset.alerts = self.alert_manager.snapshot();

        // Snapshot leaf_color_tags
        preset.leaf_color_tags = self.panel_app.leaf_color_tags
            .iter()
            .map(|(lid, color)| (lid.0, *color))
            .collect();

        // Snapshot per-slot FreeItem docking layouts.
        // Each entry is a LayoutSnapshot JSON string, or None if the slot is empty.
        // `slot_leaves` carries the per-leaf kind + panel_id for state restoration.
        for i in 0..4 {
            let tree = self.sidebar_state.slot_dockings[i].inner().tree();
            preset.slot_layouts[i] = uzor::panels::serialize::LayoutSnapshot::from_tree(tree, "slot")
                .to_json()
                .ok();

            // Collect persisted leaf descriptors.
            let persist_source = |source: &zengeld_panels::trading::SymbolSource| -> zengeld_chart::preset::preset::PersistedSymbolSource {
                match source {
                    zengeld_panels::trading::SymbolSource::HyperFocus => zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus,
                    zengeld_panels::trading::SymbolSource::Fixed { symbol, exchange, account_type } => zengeld_chart::preset::preset::PersistedSymbolSource::Fixed {
                        symbol: symbol.clone(),
                        exchange: exchange.clone(),
                        account_type: account_type.clone(),
                    },
                    zengeld_panels::trading::SymbolSource::BoundToChart { leaf_id } => zengeld_chart::preset::preset::PersistedSymbolSource::BoundToChart {
                        leaf_id: *leaf_id,
                    },
                }
            };
            let leaves_desc: Vec<zengeld_chart::preset::preset::PersistedFreeLeaf> = tree
                .leaves()
                .into_iter()
                .filter_map(|leaf| {
                    let item = leaf.panels.get(leaf.active_tab)?;
                    let panel_id = item.panel_id().0;
                    use sidebar_content::free_slot::FreeItem;
                    use zengeld_chart::preset::preset::PersistedFreeItemKind;
                    // Helper: derive a PersistedSymbolSource from a panel's exchange/symbol/account_type
                    // fields (market-data panels no longer carry a SymbolSource field).
                    let persist_source_from_fields = |symbol: &str, exchange: &str, account_type: &str|
                        -> zengeld_chart::preset::preset::PersistedSymbolSource
                    {
                        if exchange.is_empty() {
                            zengeld_chart::preset::preset::PersistedSymbolSource::HyperFocus
                        } else {
                            zengeld_chart::preset::preset::PersistedSymbolSource::Fixed {
                                symbol: symbol.to_string(),
                                exchange: exchange.to_string(),
                                account_type: account_type.to_string(),
                            }
                        }
                    };

                    let kind = match item {
                        FreeItem::Dom(id) => {
                            let state = self.panels_store.dom.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::Dom {
                                source,
                                tick_size: state.tick_size,
                                levels_displayed: state.levels_displayed,
                                center_price: state.center_price,
                            }
                        }
                        FreeItem::Footprint(id) => {
                            let state = self.panels_store.footprint.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::Footprint {
                                source,
                                tick_size: state.tick_size,
                            }
                        }
                        FreeItem::VolumeProfile(id) => {
                            let state = self.panels_store.volume_profile.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::VolumeProfile {
                                source,
                                tick_size: state.tick_size,
                            }
                        }
                        FreeItem::LiquidityHeatmap(id) => {
                            let state = self.panels_store.liquidity_heatmap.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::LiquidityHeatmap {
                                source,
                                tick_size: state.tick_size,
                                snapshot_interval_ms: state.snapshot_interval_ms,
                            }
                        }
                        FreeItem::BigTrades(id) => {
                            let state = self.panels_store.big_trades.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::BigTrades {
                                source,
                            }
                        }
                        FreeItem::L2Tape(id) => {
                            let state = self.panels_store.l2_tape.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::L2Tape {
                                source,
                            }
                        }
                        FreeItem::TradeTape(id) => {
                            let state = self.panels_store.trade_tape.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::TradeTape {
                                source,
                            }
                        }
                        FreeItem::OrderEntry(id) => {
                            let state = self.panels_store.order_entry.get(id)?;
                            let source = persist_source(&state.source);
                            PersistedFreeItemKind::OrderEntry {
                                source,
                            }
                        }
                        FreeItem::PositionManager(_) => PersistedFreeItemKind::PositionManager,
                        FreeItem::TradeLog(_) => PersistedFreeItemKind::TradeLog,
                        FreeItem::RiskCalculator(_) => PersistedFreeItemKind::RiskCalculator,
                        FreeItem::TradingContainer(id) => {
                            let state = self.panels_store.trading_container.get(id)?;
                            let source = persist_source(&state.source);
                            PersistedFreeItemKind::TradingContainer {
                                source,
                                tick_size: state.tick_size,
                                market_price: state.market_price,
                            }
                        }
                    };
                    Some(zengeld_chart::preset::preset::PersistedFreeLeaf {
                        leaf_id: leaf.id.0,
                        panel_id,
                        kind,
                    })
                })
                .collect();
            preset.slot_leaves[i] = leaves_desc;
        }

        self.panel_app.presets.insert(id.to_string(), preset);
    }

    // =========================================================================
    // Color picker drag helper
    // =========================================================================

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

/// Estimate the character index in `line_text` closest to pixel coordinate `x`.
///
/// Uses proportional estimation based on text_x (where the line starts) and the
/// total character count. This avoids needing a rendering context at input time;
/// the selection overlay in render.rs will use the precise measure_text values.
fn chat_char_idx_from_x(line_text: &str, x: f64, text_x: f64) -> u16 {
    let char_count = line_text.chars().count();
    if char_count == 0 {
        return 0;
    }
    if x <= text_x {
        return 0;
    }
    // Proportional estimate: assume ~7.5px per character for normal fonts.
    // The render-side overlay uses precise measure_text, so this just needs
    // to be a reasonable starting point for the drag anchor.
    let offset_px = x - text_x;
    let approx_char = (offset_px / 7.5).round() as usize;
    approx_char.min(char_count) as u16
}

/// Convert a CSS hex color string (`#RRGGBB` or `#RRGGBBAA`) to an RGBA `[f32; 4]` array.
///
/// All channels are in the 0.0–1.0 range.  Returns a default opaque black on
/// parse failure.
pub(super) fn hex_str_to_rgba(hex: &str) -> [f32; 4] {
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

/// Return a mutable reference to the active `ScrollState` for the Tags & Tabs modal.
///
/// Dispatch depends on which sidebar section and sub-tab are currently active:
/// - TABS sidebar → `tabs_scroll`
/// - TAGS sidebar, Groups sub-tab → `tags_groups_scroll`
/// - TAGS sidebar, Details sub-tab → `tags_details_scroll`
/// - MAP sidebar → `tabs_scroll` (MAP section has no scrollable content)
fn tags_tabs_active_scroll(
    state: &mut zengeld_chart::ui::modal_settings::TagsTabsState,
) -> &mut zengeld_chart::ui::scroll_state::ScrollState {
    use zengeld_chart::ui::modal_settings::{TagsTabsSidebar, TagsTabsTagsTab};
    match state.sidebar {
        TagsTabsSidebar::Tabs => &mut state.tabs_scroll,
        TagsTabsSidebar::Tags => match state.tags_tab {
            TagsTabsTagsTab::Groups  => &mut state.tags_groups_scroll,
            TagsTabsTagsTab::Details => &mut state.tags_details_scroll,
        },
        TagsTabsSidebar::Map => &mut state.tabs_scroll,
    }
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
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("delete", "reset_cache", "Сбросить кэш"),
        ContextMenuItemState::action_with_icon("delete", "reset_storage", "Сбросить хранилище"),
    ]
}
