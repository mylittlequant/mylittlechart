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
mod modals;
mod settings;

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
    ChartPanelLayout,
    CursorStyle,
    input::DragMode,
    localize_primitive_name,
};
use zengeld_chart::ui::context_menu::{
    ContextMenuTarget, ContextMenuItemState,
    build_primitive_context_menu,
};
use zengeld_chart::ui::modal_state::OpenModal;
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
        let pty_id = WidgetId::from(crate::text_input::AGENT_PTY);
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
            .map(|w| w.as_str().to_string());
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

        // 3. Click on canvas — close any open transient overlays first.
        self.close_transient_overlays();

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
                            // Mutate group.scale_mode when sync_viewport is on so the
                            // group remembers the mode for new joiners.
                            let viewport_sync_on = self.panel_app.panel_grid
                                .chart_id_for_leaf(leaf_id)
                                .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
                                .and_then(|gid| {
                                    self.panel_app.tag_manager.group_mut(gid).map(|g| {
                                        g.scale_mode = next_mode;
                                        g.sync_flags.sync_viewport
                                    })
                                })
                                .unwrap_or(false);
                            // Propagate viewport + scale_mode to peers when sync_viewport is on.
                            if viewport_sync_on {
                                let viewport_state = self.panel_app.panel_grid
                                    .window_for_leaf(leaf_id)
                                    .map(|w| (w.viewport.view_start, w.viewport.bar_spacing));
                                if let Some((view_start, bar_spacing)) = viewport_state {
                                    self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, Some(next_mode));
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

        // Make the window UNDER THE CURSOR active before hit-testing, so the
        // context menu opens in the hovered window (not whichever was focused).
        // Mirrors the left-click path (resolve_input → set_active_leaf).
        if self.panel_app.panel_grid.is_split() {
            use zengeld_chart::state::ChartInputTarget;
            let target = self.panel_app.panel_grid.resolve_input(
                x, y, self.content_rect.x, self.content_rect.y,
            );
            let cursor_leaf = match target {
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => Some(leaf_id),
                ChartInputTarget::Separator { .. } | ChartInputTarget::None => None,
            };
            if let Some(leaf_id) = cursor_leaf {
                self.panel_app.panel_grid.set_active_leaf(leaf_id);
                // The cached active_frame_layout was built for the PREVIOUS active
                // leaf. Now that the cursor's leaf is active, that cache holds the
                // wrong leaf's chart rect — the right-click hit-test below would
                // subtract the wrong window origin and target the old window.
                // Invalidate it so the fallback rebuilds the layout for the newly
                // active (cursor's) leaf. (Split-pane only; single window never
                // changes active leaf here.)
                self.active_frame_layout = None;
            }
        }

        let w = self.width as f64;
        let h = self.height as f64;

        // Right-click on color-tag square: no context menu (the popup has a Remove button).

        // Hit-test primitives and indicators, then dispatch to context menu.
        // Steps 1–5 (geometry + hit-test) live in ChartPanelGrid::handle_right_click.
        // Step 6 (opening context menu / settings) stays here — it touches panel_app state.
        let _fallback_rc;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_rc = self.build_extended_layout(); &_fallback_rc }
        };

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
            x as f64, y as f64, extended,
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
                            .map(|item| {
                                let localized = localize_primitive_name(&item.type_id, &item.display_name);
                                (localized, item.locked, item.visible)
                            })
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
            .map(|w| w.as_str().to_string());
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

        let double_click_wid: Option<String> = self.input_coordinator.borrow_mut().process_double_click(x, y).map(|w| w.as_str().to_string());
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

        let _fallback_dc;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_dc = self.build_extended_layout(); &_fallback_dc }
        };
        let overlay_results_dc = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
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
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.as_str().to_string());
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
                        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
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
            .map(|h| h.as_str() == "right_sidebar_separator")
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
                .and_then(|h| h.as_str().strip_prefix("watchlist_sep_").and_then(|s| s.parse::<usize>().ok()))
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
                .and_then(|h| h.as_str().strip_prefix("watchlist_").and_then(|s| s.parse::<usize>().ok()));
            if let Some(idx) = on_watchlist_row {
                // Verify this is a plain index (not "watchlist_delete_N" etc.).
                let hovered_id = self.input_coordinator.borrow_mut().hovered_widget()
                    .map(|h| h.as_str().to_string())
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
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.as_str().to_string());
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

        // ── Slot panel drag initiation ───────────────────────────────────────
        // Detect drag on `slot:{idx}:leaf:{leaf_id}:dom:body` (DOM BlackboxPanel)
        // or `slot:{idx}:leaf:{leaf_id}:focus_content` (other panel types).
        // DOM is now registered as a BlackboxPanel widget with id `slot:N:leaf:M:dom:body`.
        if self.active_drag_panel.is_none() {
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.as_str().to_string());
            if let Some(ref wid) = hovered_wid {
                // Check for DOM BlackboxPanel widget: slot:N:leaf:M:dom:body
                if let Some(rest) = wid.strip_prefix("slot:") {
                    if let Some((slot_str, after_slot)) = rest.split_once(":leaf:") {
                        if let Ok(slot_idx) = slot_str.parse::<usize>() {
                            if slot_idx < 4 {
                                // DOM body widget
                                if let Some(leaf_id_str) = after_slot.strip_suffix(":dom:body") {
                                    if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                        let leaf_id = uzor::panels::LeafId(raw);
                                        let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                            .inner()
                                            .tree()
                                            .leaf(leaf_id)
                                            .and_then(|l| l.active_panel().cloned());
                                        use sidebar_content::free_slot::FreeItem;
                                        if let Some(item @ FreeItem::Dom(_)) = item_opt {
                                            if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                                if panel.handle_drag_start("dom:body", x, y) {
                                                    self.active_drag_panel = Some((item, "dom:body".to_string(), x, y));
                                                    self.ui_drag_active = true;
                                                    return false;
                                                }
                                            }
                                        }
                                    }
                                }
                                // Other panel types via focus_content widget
                                else if let Some(leaf_id_str) = after_slot.strip_suffix(":focus_content") {
                                    if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                        let leaf_id = uzor::panels::LeafId(raw);
                                        let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                            .inner()
                                            .tree()
                                            .leaf(leaf_id)
                                            .and_then(|l| l.active_panel().cloned());
                                        use sidebar_content::free_slot::FreeItem;
                                        match item_opt {
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
                        .map(|h| h.as_str() == "right_sidebar_separator")
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
            let id = wid.as_str().to_string();
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
                        UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_tab_scroll,
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
                                self.panel_app.user_settings_state.new_passphrase_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.new_passphrase_focused = true;
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
                                self.panel_app.user_settings_state.new_passphrase_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.recovery_key_display_focused = false;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                            }
                            "profile_mgr:recovery_key_input" => {
                                self.panel_app.user_settings_state.recovery_key_display_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.recovery_key_display_focused = true;
                                self.panel_app.user_settings_state.new_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_profile_name_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                            }
                            "profile_mgr:new_passphrase_input" => {
                                self.panel_app.user_settings_state.new_passphrase_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.new_passphrase_focused = true;
                                self.panel_app.user_settings_state.new_passphrase_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = None;
                            }
                            "profile_mgr:confirm_passphrase_input"
                            | "wizard_confirm_passphrase_input"
                            | "profile_mgr:create_confirm_passphrase_input" => {
                                self.panel_app.user_settings_state.confirm_passphrase_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.confirm_passphrase_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                                self.panel_app.user_settings_state.new_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_profile_name_focused = false;
                                self.panel_app.user_settings_state.recovery_key_display_focused = false;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                                self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
                            }
                            "profile_mgr:recovery_key_display" => {
                                self.panel_app.user_settings_state.recovery_key_display_editing.cursor = drag_cursor;
                                self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = Some(drag_cursor);
                                self.panel_app.user_settings_state.recovery_key_display_focused = true;
                                self.panel_app.user_settings_state.new_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_profile_name_focused = false;
                                self.panel_app.user_settings_state.confirm_passphrase_focused = false;
                                self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
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
                    .map(|h| h.as_str().starts_with("wl_modal:delete:"))
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
                    .map(|h| h.as_str() == "ilb:inline:name")
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
                        UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_tab_scroll,
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

        let _fallback_ds;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_ds = self.build_extended_layout(); &_fallback_ds }
        };
        let overlay_results_ds = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
            .with_overlays(&overlay_results_ds);

        // Delegate freehand-start and primitive/control-point hit-test to ChartPanelGrid.
        // Freehand: drawing_manager.start_freehand() runs inside handle_drag_start.
        // Background: viewport_before_drag already captured above.
        use zengeld_chart::state::ChartDragStartHit;
        let drag_start_hit = self.panel_app.panel_grid.handle_drag_start(x, y, extended);

        let (drag_start_mode, extra_actions) = match drag_start_hit {
            ChartDragStartHit::FreehandStarted => {
                // Baseline behavior: do NOT touch crosshair on freehand start.
                // The crosshair remains in whatever state on_mouse_move last
                // set it (typically visible from prior hover) — this matches
                // pre-migration UX where crosshair stayed visible during a
                // freehand stroke (mouse-move doesn't fire while button held,
                // so visible state is implicitly preserved).
                return false;
            }
            ChartDragStartHit::SubPaneSeparator { instance_id } => {
                // Sub-pane separator drag — set DragMode so on_drag_move
                // routes to drag_pane_separator. No output action needed.
                self.input_handler.state.drag_mode = DragMode::PaneSeparator { instance_id };
                self.ui_drag_active = true;
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

        // ── Coordinator-routed panel drag (DOM, L2Tape, Footprint, BigTrades, LiquidityHeatmap, VolumeProfile) ─────
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
                // Persist the auto-close so it survives restart.
                self.sidebar_data_dirty = true;
                self.persist_profile();
                return;
            }
            // Enforce [MIN_SIDEBAR_WIDTH, right_toolbar_left_x - min_chart_w] clamp.
            let max_w = self.right_toolbar_left_x - min_chart_w;
            let new_width = (self.right_toolbar_left_x - x)
                .clamp(sidebar_content::state::MIN_SIDEBAR_WIDTH, max_w);
            self.sidebar_state.set_right_width(new_width);
            // NOTE: width is persisted on drag-end (on_drag_end), not on every
            // mousemove — that would be hundreds of disk writes per second.
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
                        self.panel_app.user_settings_state.new_passphrase_editing.cursor = new_cursor;
                    }
                    "wizard_profile_name_input" | "profile_mgr:name_input" => {
                        self.panel_app.user_settings_state.new_profile_name_editing.cursor = new_cursor;
                    }
                    "profile_mgr:recovery_key_input" => {
                        self.panel_app.user_settings_state.recovery_key_display_editing.cursor = new_cursor;
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
                        UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_tab_scroll,
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

        // PRIORITY: Freehand drawing (brush/highlighter) — add points during drag.
        // Also update the crosshair so it tracks the cursor as the stroke is
        // drawn (without this, on_mouse_move doesn't fire while button is held
        // and the crosshair freezes at its pre-press position, only "jumping"
        // when win32 cursor polling catches the cursor leaving the window).
        //
        // In split mode the crosshair is written to the active leaf (freehand
        // drawing is locked to the leaf where the stroke started, which is the
        // active leaf) via update_crosshair_split so the correct leaf layout is
        // used (F1).  Propagation to sync-group peers is also applied (F11).
        {
            let _fallback_fh;
            let extended = match self.active_frame_layout.as_ref() {
                Some(e) => e,
                None => { _fallback_fh = self.build_extended_layout(); &_fallback_fh }
            };
            if self.panel_app.panel_grid.extend_freehand(x, y, extended) {
                let drag_mode = self.input_handler.state.drag_mode;
                if self.panel_app.panel_grid.is_split() {
                    // Freehand drawing is on the active leaf; use its layout for
                    // the crosshair update so coordinates map correctly (F1).
                    let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                    if let Some(active_leaf) = active_leaf_opt {
                        let leaf_rect_opt = self.get_leaf_absolute_rect(active_leaf);
                        if let Some(leaf_rect) = leaf_rect_opt {
                            let ext_opt = self.frame_layouts.get(&active_leaf)
                                .cloned()
                                .or_else(|| self.build_extended_layout_for_leaf(active_leaf, &leaf_rect));
                            if let Some(ext) = ext_opt {
                                if let Some((ts, price, vis, pane_idx)) = self.panel_app.panel_grid
                                    .update_crosshair_split(x, y, active_leaf, &ext)
                                {
                                    // Propagate to sync-group peers (F11).
                                    self.propagate_crosshair_to_sync_group(
                                        active_leaf, ts, price, vis, pane_idx,
                                    );
                                }
                            }
                        }
                    }
                } else {
                    self.panel_app.panel_grid.update_crosshair(
                        x, y, drag_mode, /* drawing_active */ true, &extended,
                    );
                }
                // Propagate the freehand stroke's growing point list to sync-group peers
                // so they render the live projection. Click-based primitives already do this
                // via modals.rs handlers; freehand never did until now (bug F-PROJ).
                if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    self.propagate_drawing_state_to_sync_group(active_leaf);
                }
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
                        match self.frame_layouts.get(&leaf)
                            .map(|e| e as &zengeld_chart::ExtendedFrameLayout)
                            .or_else(|| None)
                        {
                            Some(ext) => ext.sub_panes.iter()
                                .map(|sp| (sp.instance_id, sp.content.height))
                                .collect(),
                            None => match self.build_extended_layout_for_leaf(leaf, &leaf_rect) {
                                Some(ext) => ext.sub_panes.iter()
                                    .map(|sp| (sp.instance_id, sp.content.height))
                                    .collect(),
                                None => std::collections::HashMap::new(),
                            },
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

        // Use the frame-cached layout; fall back to building on demand.
        let _fallback_dm;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_dm = self.build_extended_layout(); &_fallback_dm }
        };
        let overlay_results_dm = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
            .with_overlays(&overlay_results_dm);
        let actions = self.input_handler.process_action(
            ChartInputAction::DragMove { mode: drag_mode, x, y, delta_x: dx, delta_y: dy },
            &hit_tester,
        );
        self.process_output_actions(actions);

        // Update crosshair during drag via ChartPanelGrid.
        // In split mode, resolve the hovered leaf and use update_crosshair_split
        // so the crosshair follows the hovered leaf (not the active leaf) — F2.
        // Propagation also uses the hovered leaf — F3.
        // Re-read active_frame_layout after process_output_actions (may have updated it).
        let _fallback_dm2;
        let extended2 = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_dm2 = self.build_extended_layout(); &_fallback_dm2 }
        };
        let drag_mode = self.input_handler.state.drag_mode;
        if self.panel_app.panel_grid.is_split() {
            use zengeld_chart::state::ChartInputTarget;
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
                if let Some(leaf_rect) = leaf_rect_opt {
                    let ext_opt = self.frame_layouts.get(&leaf_id)
                        .cloned()
                        .or_else(|| self.build_extended_layout_for_leaf(leaf_id, &leaf_rect));
                    if let Some(ext) = ext_opt {
                        if let Some((ts, price, vis, pane_idx)) = self.panel_app.panel_grid
                            .update_crosshair_split(x, y, leaf_id, &ext)
                        {
                            self.propagate_crosshair_to_sync_group(leaf_id, ts, price, vis, pane_idx);
                        }
                    }
                }
            }
        } else if let Some((timestamp, price, crosshair_visible, pane_index)) = self.panel_app.panel_grid
            .update_crosshair(x, y, drag_mode, false, extended2)
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

        // ── End coordinator-routed panel drag (DOM, L2Tape, Footprint, BigTrades, LiquidityHeatmap, VolumeProfile) ─
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
                    self.panel_app.user_settings_state.new_passphrase_editing.selection_start,
                    self.panel_app.user_settings_state.new_passphrase_editing.cursor,
                ),
                "wizard_profile_name_input" | "profile_mgr:name_input" => (
                    self.panel_app.user_settings_state.new_profile_name_editing.selection_start,
                    self.panel_app.user_settings_state.new_profile_name_editing.cursor,
                ),
                "profile_mgr:recovery_key_input" => (
                    self.panel_app.user_settings_state.recovery_key_display_editing.selection_start,
                    self.panel_app.user_settings_state.recovery_key_display_editing.cursor,
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
                        self.panel_app.user_settings_state.new_passphrase_editing.selection_start = None;
                    }
                    "wizard_profile_name_input" | "profile_mgr:name_input" => {
                        self.panel_app.user_settings_state.new_profile_name_editing.selection_start = None;
                    }
                    "profile_mgr:recovery_key_input" => {
                        self.panel_app.user_settings_state.recovery_key_display_editing.selection_start = None;
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
                &mut self.panel_app.user_settings_state.server_tab_scroll,
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
            // Clear in-progress drawing state on sync peers. After
            // complete_freehand the source DM has transitioned to Idle, but
            // peers still hold `DrawingState::Creating` from the last drag-move
            // propagation. Pushing the now-Idle state clears their live stroke
            // so the only remaining freehand visible on peers is the finalized
            // primitive (via add_synced_primitives / TagManager).
            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                self.propagate_drawing_state_to_sync_group(active_leaf);
            }
            // Save after both grouped and standalone paths — intercept may have moved the
            // primitive to TagManager, so the snapshot must be taken after that transfer.
            self.autosave_snapshot();
            self.sidebar_data_dirty = true;
            return;
        }

        let drag_mode = self.input_handler.state.drag_mode;
        let _fallback_de;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_de = self.build_extended_layout(); &_fallback_de }
        };
        let overlay_results_de = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
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
        self.panel_app.clock_popup_state.hovered_item = None;
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
            let id_str = hovered.as_str();
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
            } else if id_str.starts_with("clock_popup:") {
                // Strip the "clock_popup:" prefix to match hovered_item keys used in renderer.
                if let Some(item) = id_str.strip_prefix("clock_popup:") {
                    self.panel_app.clock_popup_state.hovered_item = Some(item.to_string());
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
            let hovered = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.as_str().to_string());
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

        // Phase 7.3: crosshair gate — show only when hovered widget is chart:pane:*
        // AND cursor is inside the actual chart canvas area (not over the price /
        // time scales which the chart:pane BlackboxPanel covers but logically own).
        //
        // Hovering price/time scale, sub-pane separator, toolbar, modal, etc. all
        // suppress the crosshair. Click-tool drawing bypasses the gate so the
        // preview anchor stays visible when the cursor drifts outside the chart
        // pane. Freehand drawing does NOT bypass — the freehand stroke renders
        // its own preview, the crosshair is redundant and visually noisy.
        //
        // In split mode the crosshair follows the HOVERED leaf (not the active
        // leaf). Resolve hovered leaf once here and reuse it throughout the
        // split-mode crosshair and overlay update paths below (avoids duplicate
        // resolve_input calls — I1).
        let split_hovered_leaf: Option<zengeld_chart::LeafId> =
            if self.panel_app.panel_grid.is_split() {
                use zengeld_chart::state::ChartInputTarget;
                match self.panel_app.panel_grid.resolve_input(
                    x, y, self.content_rect.x, self.content_rect.y,
                ) {
                    ChartInputTarget::Chart { leaf_id }
                    | ChartInputTarget::PriceScale { leaf_id }
                    | ChartInputTarget::TimeScale { leaf_id }
                    | ChartInputTarget::ScaleCorner { leaf_id, .. } => Some(leaf_id),
                    _ => None,
                }
            } else {
                None
            };
        {
            // Drawing bypass: keep crosshair visible while the hovered leaf's
            // drawing manager reports an in-progress primitive.  In split mode
            // the hovered leaf (not the active leaf) is checked so a drawing on
            // a non-active leaf does not incorrectly suppress the crosshair (F7).
            let is_drawing = if let Some(hovered) = split_hovered_leaf {
                // Split mode: read from hovered leaf's window.
                self.panel_app.panel_grid.window_for_leaf(hovered)
                    .map(|w| w.drawing_manager.is_drawing())
                    .unwrap_or(false)
            } else {
                // Single-window mode: active leaf == hovered leaf.
                self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.is_drawing())
                    .unwrap_or(false)
            };

            // Inside-chart check: chart:pane covers canvas + scales + sub-pane seps,
            // but crosshair should only show over the canvas. Use the chart's own
            // hit-tester to distinguish canvas (Chart) from scale (PriceScale /
            // TimeScale / ScaleCorner) and separator (PaneSeparator).
            //
            // In split mode use the hovered leaf's layout so the hit-test reflects
            // the correct coordinate space — the cursor is outside the active leaf's
            // rect in split mode, making an active-leaf layout always return
            // on_canvas=false and suppressing the crosshair incorrectly (F8).
            //
            // STALE-HOVER FIX: hovered_widget() reflects the last begin_frame, which
            // can lag behind rapid cursor movement (the render loop hasn't run yet for
            // the new position). When hovered_chart_pane is false purely because the
            // hover state is stale/None, we must NOT suppress the crosshair — that
            // causes a deadlock (no redraw requested from this path → hover never
            // refreshes → crosshair stays hidden forever). The geometry hit-test uses
            // fresh frame_layouts/active_frame_layout, so we always run it and use
            // the widget-name check only as an ADDITIONAL suppressor (i.e. when a
            // known non-chart widget is explicitly hovered, trust that over geometry).
            let on_canvas = {
                use zengeld_chart::input::ChartHitTester;
                use zengeld_chart::engine::input::HitResult;
                // Resolve the layout and overlay for the relevant leaf.
                let (extended, overlays) = if let Some(hovered) = split_hovered_leaf {
                    let leaf_rect_opt = self.get_leaf_absolute_rect(hovered);
                    let ext = self.frame_layouts.get(&hovered)
                        .cloned()
                        .or_else(|| leaf_rect_opt.and_then(|r| self.build_extended_layout_for_leaf(hovered, &r)));
                    let ovl = self.panel_app.panel_grid.window_for_leaf(hovered)
                        .map(|w| w.sub_pane_overlay_results.clone())
                        .unwrap_or_default();
                    (ext, ovl)
                } else {
                    let ext = self.active_frame_layout.clone()
                        .or_else(|| Some(self.build_extended_layout()));
                    let ovl = self.panel_app.panel_grid.active_window()
                        .map(|w| w.sub_pane_overlay_results.clone())
                        .unwrap_or_default();
                    (ext, ovl)
                };
                let geometry_says_canvas = if let Some(ext) = extended {
                    let tester = ExtendedLayoutHitTester::new(&ext).with_overlays(&overlays);
                    matches!(
                        tester.hit_test(x, y),
                        HitResult::Chart
                            | HitResult::SubPaneChart { .. }
                            | HitResult::PriceScale
                            | HitResult::SubPanePriceScale { .. }
                    )
                } else {
                    false
                };
                // A non-chart widget is explicitly hovered → trust that, suppress crosshair.
                // An empty/stale hover (None) → trust the geometry instead.
                let explicit_non_chart = self.input_coordinator.borrow().hovered_widget()
                    .map(|w| !w.as_str().starts_with("chart:pane:"))
                    .unwrap_or(false);
                geometry_says_canvas && !explicit_non_chart
            };

            if !on_canvas && !is_drawing {
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
        //
        // Read drawing state from the hovered leaf's window, not the active
        // leaf's window — if the active leaf is drawing but the hovered leaf is
        // not, the split crosshair block should still run for the hovered leaf (F9).
        let is_drawing_skip = if let Some(hovered) = split_hovered_leaf {
            self.panel_app.panel_grid.window_for_leaf(hovered)
                .map(|w| w.drawing_manager.is_drawing())
                .unwrap_or(false)
        } else {
            self.panel_app.panel_grid.active_window()
                .map(|w| w.drawing_manager.is_drawing())
                .unwrap_or(false)
        };
        if self.panel_app.panel_grid.is_split() && !is_drawing_skip {
            use zengeld_chart::state::ChartInputTarget;
            // Reuse split_hovered_leaf resolved above to avoid a redundant
            // resolve_input call (I1 eliminated).  Convert back to a full
            // ChartInputTarget so the Separator/None hide path still works.
            let target = match split_hovered_leaf {
                Some(leaf_id) => ChartInputTarget::Chart { leaf_id },
                None => self.panel_app.panel_grid.resolve_input(
                    x, y, self.content_rect.x, self.content_rect.y,
                ),
            };
            match target {
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id }
                | ChartInputTarget::ScaleCorner { leaf_id, .. } => {
                    // Use frame-cached layout for the hovered leaf; delegate crosshair
                    // update to ChartPanelGrid::update_crosshair_split.
                    let leaf_rect_opt = self.get_leaf_absolute_rect(leaf_id);
                    let extended_opt = self.frame_layouts.get(&leaf_id)
                        .cloned()
                        .or_else(|| leaf_rect_opt.and_then(|r| self.build_extended_layout_for_leaf(leaf_id, &r)));
                    if let Some(extended) = extended_opt {
                        if let Some((ts, price, vis, pane_idx)) = self.panel_app.panel_grid
                            .update_crosshair_split(x, y, leaf_id, &extended)
                        {
                            // Propagate to sync-group peers (handles order-flow panels too).
                            self.propagate_crosshair_to_sync_group(leaf_id, ts, price, vis, pane_idx);
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
            // Reuse split_hovered_leaf (resolved at the top of this mouse_move
            // handler) instead of calling resolve_input again (I1 eliminated).
            {
                let hovered_leaf = split_hovered_leaf;
                if let Some(leaf_id) = hovered_leaf {
                    let leaf_rect_opt = self.get_leaf_absolute_rect(leaf_id);
                    let extended_opt = self.frame_layouts.get(&leaf_id)
                        .cloned()
                        .or_else(|| leaf_rect_opt.and_then(|lr| self.build_extended_layout_for_leaf(leaf_id, &lr)));
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
        // Read from the frame-cached layout — no need to rebuild on every mouse event.
        let is_drawing = self.panel_app.panel_grid.active_window()
            .map(|w| w.drawing_manager.is_drawing())
            .unwrap_or(false);

        // Hide crosshair during separator drag — resize cursor is sufficient feedback.
        if matches!(self.input_handler.state.drag_mode, DragMode::PaneSeparator { .. }) {
            self.hide_crosshair();
            return;
        }
        // Clone the cached layout so the borrow ends here, allowing the mutable
        // propagate_crosshair_to_sync_group and update_sub_pane_overlay_hover calls below.
        let extended = self.active_frame_layout.clone()
            .unwrap_or_else(|| self.build_extended_layout());
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
        //
        // In split mode the drawing may be on any leaf, not just the active one.
        // Check all leaves so a drawing-in-progress on a non-active leaf is not
        // ignored (F4 — previously only active window was checked).
        let is_drawing = if self.panel_app.panel_grid.is_split() {
            let leaf_ids: Vec<zengeld_chart::LeafId> = self.panel_app
                .panel_grid
                .panel_rects()
                .keys()
                .copied()
                .collect();
            leaf_ids.iter().any(|&lid| {
                self.panel_app.panel_grid.window_for_leaf(lid)
                    .map(|w| w.drawing_manager.is_drawing())
                    .unwrap_or(false)
            })
        } else {
            self.panel_app.panel_grid.active_window()
                .map(|w| w.drawing_manager.is_drawing())
                .unwrap_or(false)
        };
        if !is_drawing {
            // hide_crosshair is split-aware: clears all leaves in split mode and
            // propagates the hide to every sync-group's order-flow panels (F4).
            self.hide_crosshair();
        }
    }

    /// Hide the crosshair on the active window and propagate hide to sync group.
    ///
    /// In split mode all leaves are cleared (F5) and each leaf's sync-group peers
    /// receive the hide via `propagate_crosshair_to_sync_group` (F6), so order-flow
    /// panels attached to any group are also cleared.  In single-window mode the
    /// active leaf is used — active leaf == only leaf, so no ambiguity.
    fn hide_crosshair(&mut self) {
        if self.panel_app.panel_grid.is_split() {
            // Collect leaf ids to avoid borrow conflicts.
            let leaf_ids: Vec<zengeld_chart::LeafId> = self.panel_app
                .panel_grid
                .panel_rects()
                .keys()
                .copied()
                .collect();
            // Clear visible on all split leaves (F5).
            for &lid in &leaf_ids {
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(lid) {
                    window.crosshair.visible = false;
                }
            }
            // Propagate hide from each leaf so order-flow panels in every sync
            // group are notified (F6).
            for lid in leaf_ids {
                self.propagate_crosshair_to_sync_group(lid, 0, 0.0, false, None);
            }
        } else {
            let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
            if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
                window.crosshair.visible = false;
            }
            // Propagate hide to sync group peers.
            if let Some(active_leaf) = active_leaf_opt {
                self.propagate_crosshair_to_sync_group(active_leaf, 0, 0.0, false, None);
            }
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
                .map(|h| h.as_str() == "right_sidebar_separator")
                .unwrap_or(false);
        if on_sidebar_separator {
            return CursorStyle::EwResize;
        }

        // Watchlist column separators: show EwResize cursor when hovering or dragging.
        let on_watchlist_col_sep = self.sidebar_state.watchlist_sep_drag.is_some()
            || self.input_coordinator.borrow_mut().hovered_widget()
                .map(|h| h.as_str().starts_with("watchlist_sep_"))
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
            let hovered = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.as_str().to_string());
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
                    // Use frame-cached layout for the hovered leaf to get correct cursor.
                    let leaf_rect_opt = self.get_leaf_absolute_rect(leaf_id);
                    let ext_opt = self.frame_layouts.get(&leaf_id)
                        .cloned()
                        .or_else(|| leaf_rect_opt.and_then(|r| self.build_extended_layout_for_leaf(leaf_id, &r)));
                    if let Some(extended) = ext_opt {
                        let tester = zengeld_chart::layout::ExtendedLayoutHitTester::new(&extended);
                        use zengeld_chart::input::ChartHitTester;
                        let hit = tester.hit_test(x, y);
                        return hit.cursor();
                    }
                    return CursorStyle::Default;
                }
                ChartInputTarget::None => return CursorStyle::Default,
            }
        }

        // Not over any UI element — use extended layout hit_test for chart zones
        // (includes sub-pane price scales and separators).
        let _fallback_cursor;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_cursor = self.build_extended_layout(); &_fallback_cursor }
        };
        let tester = zengeld_chart::layout::ExtendedLayoutHitTester::new(extended);
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
                    wid.as_str().strip_prefix("color_picker_")
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
            // process_scroll(x, y) finds the TOPMOST widget at (x, y) WITH Sense::SCROLL —
            // skipping any non-scroll widgets stacked above (rows, items, dimmer, etc.).
            // This is sense-aware (uzor v1.1.3+) and immune to the 1-frame lag of
            // hovered_widget() which reads previous frame's hover state.
            {
                let hovered_id = self.input_coordinator.borrow_mut().process_scroll(x, y).map(|h| h.as_str().to_string());
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
                                    UserSettingsTab::Server => &mut self.panel_app.user_settings_state.server_tab_scroll,
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
            // wheel events via coordinator. Sense::SCROLL was added to their registrations.
            // process_scroll(x, y) is sense-aware — only returns SCROLL widgets, ignoring
            // any non-scroll widgets stacked above.
            {
                let hovered_wid = self.input_coordinator.borrow_mut().process_scroll(x, y).map(|w| w.as_str().to_string());

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
                    // Sense-aware process_scroll(x, y) — skips non-scroll widgets stacked
                    // above (signal rows, group headers, etc.) and finds the topmost
                    // SCROLL-sensitive widget at cursor.
                    {
                        let sg_hovered = self.input_coordinator.borrow_mut().process_scroll(x, y).map(|h| h.as_str().to_string());
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
                    // Sense-aware process_scroll(x, y) — finds topmost SCROLL widget
                    // ignoring rows/items stacked above without scroll sense.
                    if self.sidebar_state.right_panel == sidebar_content::state::RightSidebarPanel::Agents {
                        let hovered_id = self.input_coordinator.borrow_mut().process_scroll(x, y).map(|h| h.as_str().to_string());
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
                            use sidebar_content::free_slot::FreeItem;
                            let scroll_step = dy;

                            // First: check if coordinator scroll target is a DOM BlackboxPanel.
                            // Sense-aware process_scroll(x, y) finds dom:body even if non-scroll
                            // widgets (overlays/buttons) are stacked above.
                            let dom_leaf_opt = {
                                let hovered = self.input_coordinator.borrow_mut().process_scroll(x, y).map(|h| h.as_str().to_string());
                                hovered.and_then(|wid| {
                                    let rest = wid.strip_prefix("slot:")?;
                                    let (idx_str, after_slot) = rest.split_once(":leaf:")?;
                                    let leaf_id_str = after_slot.strip_suffix(":dom:body")?;
                                    let panel_idx: usize = idx_str.parse().ok()?;
                                    if panel_idx != slot_idx { return None; }
                                    let raw: u64 = leaf_id_str.parse().ok()?;
                                    Some(uzor::panels::LeafId(raw))
                                })
                            };

                            if let Some(dom_leaf_id) = dom_leaf_opt {
                                let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                    .inner()
                                    .tree()
                                    .leaf(dom_leaf_id)
                                    .and_then(|l| l.active_panel().cloned());
                                if let Some(FreeItem::Dom(pid)) = item_opt {
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        if ctrl {
                                            // Ctrl+scroll: zoom tick_size (depth aggregation).
                                            let new_tick = if scroll_step > 0.0 {
                                                (state.tick_size * 10.0).clamp(0.0001, 100.0)
                                            } else if scroll_step < 0.0 {
                                                (state.tick_size / 10.0).clamp(0.0001, 100.0)
                                            } else {
                                                state.tick_size
                                            };
                                            state.set_tick_size(new_tick);
                                        } else {
                                            use zengeld_panels::BlackboxEvent;
                                            use zengeld_panels::panel_trait::TradingPanel;
                                            state.handle_blackbox_event(0.0, 0.0, BlackboxEvent::Scroll { dy: scroll_step as f32 });
                                        }
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                                return;
                            }

                            // Hit-test all `slot:{idx}:leaf:{lid}:focus_content` rects for other panels.
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

                                match item_opt {
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
                    // Sense-aware process_scroll(x, y) — skips non-scroll widgets stacked above
                    // (slot focus_content, signal_group items, watchlist rows, etc.) and finds
                    // the topmost SCROLL-sensitive widget under the cursor.
                    {
                        let sidebar_hovered = self.input_coordinator.borrow_mut().process_scroll(x, y).map(|h| h.as_str().to_string());
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

        let _fallback_sc;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_sc = self.build_extended_layout(); &_fallback_sc }
        };
        let overlay_results_sc = self.panel_app.panel_grid.active_window()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
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
            && !self.panel_app.user_settings_state.new_passphrase_focused
            && !self.panel_app.user_settings_state.new_profile_name_focused
            && !self.panel_app.user_settings_state.recovery_key_display_focused
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
        let hex_id = WidgetId::from(crate::text_input::HEX_COLOR);
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
        let pty_id = WidgetId::from(crate::text_input::AGENT_PTY);
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
        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
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

        // Handle E2E passphrase input in User Settings Sync tab, Welcome Wizard, or Profile Manager
        if (self.panel_app.user_settings_state.is_open || self.panel_app.user_settings_state.show_welcome_wizard || self.panel_app.user_settings_state.needs_vault_unlock || self.panel_app.user_settings_state.show_profile_manager)
            && self.panel_app.user_settings_state.new_passphrase_focused
        {
            let editing = &mut self.panel_app.user_settings_state.new_passphrase_editing;
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
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
                }
                '\x09' => {
                    // Tab moves to next field in wizard.
                    if self.panel_app.user_settings_state.show_welcome_wizard {
                        self.panel_app.user_settings_state.new_passphrase_focused = false;
                        self.panel_app.user_settings_state.confirm_passphrase_focused = true;
                    }
                }
                '\x1b' => {
                    // Escape unfocuses without submitting
                    self.panel_app.user_settings_state.new_passphrase_focused = false;
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
            && self.panel_app.user_settings_state.recovery_key_display_focused
        {
            let editing = &mut self.panel_app.user_settings_state.recovery_key_display_editing;
            match ch {
                '\r' | '\n' => {
                    // Enter submits the recovery key if long enough.
                    let key_text = editing.text.clone();
                    if key_text.len() >= 40 {
                        self.panel_app.user_settings_state.vault_unlock_error = None;
                        self.pending_updater_cmd = Some(format!("recovery_unlock:{}", key_text));
                        eprintln!("[ChartApp] profile_mgr: recovery unlock submitted via Enter");
                    }
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
                }
                '\x1b' => {
                    // Escape unfocuses without submitting
                    self.panel_app.user_settings_state.recovery_key_display_focused = false;
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
            // For SetNewPassphrase page, compare against new_passphrase_editing; for wizard/CreatePassphrase compare against new_passphrase_editing.
            let passphrase_text = if self.panel_app.user_settings_state.show_profile_manager
                && !self.panel_app.user_settings_state.new_passphrase_editing.text.is_empty()
            {
                self.panel_app.user_settings_state.new_passphrase_editing.text.clone()
            } else {
                self.panel_app.user_settings_state.new_passphrase_editing.text.clone()
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
                        self.panel_app.user_settings_state.new_passphrase_focused = true;
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
                        self.panel_app.user_settings_state.new_passphrase_focused = true;
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
                            let active_cid = self.panel_app.panel_grid.docking().active_leaf()
                                .and_then(|leaf| self.panel_app.panel_grid.chart_id_for_leaf(leaf))
                                .map(|c| c.0);
                            let bars_opt = self.panel_app.panel_grid.active_window().map(|w| w.bars.clone());
                            if let (Some(cid), Some(bars)) = (active_cid, bars_opt) {
                                self.indicator_manager.calculate_all_for_window(cid, &bars);
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
            && !self.panel_app.user_settings_state.new_passphrase_focused
            && !self.panel_app.user_settings_state.new_profile_name_focused
            && !self.panel_app.user_settings_state.recovery_key_display_focused
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
            && self.panel_app.user_settings_state.new_passphrase_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.new_passphrase_editing, key);
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
            && self.panel_app.user_settings_state.recovery_key_display_focused
        {
            apply_key(&mut self.panel_app.user_settings_state.recovery_key_display_editing, key);
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
        let hex_id = WidgetId::from(crate::text_input::HEX_COLOR);
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
        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
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
            let hovered_wid = self.input_coordinator.borrow_mut().hovered_widget().map(|h| h.as_str().to_string());
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
            if let Some(text) = get_selection(&uss.new_passphrase_editing) {
                return Some(text);
            }
            if let Some(text) = get_selection(&uss.new_profile_name_editing) {
                return Some(text);
            }
            if let Some(text) = get_selection(&uss.recovery_key_display_editing) {
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
                let (bars_snapshot, chart_id_val) = match self.panel_app.panel_grid.active_window() {
                    Some(w) => (w.bars.clone(), w.id),
                    None => return,
                };
                if self.indicator_manager.create_instance_with_id(*instance_id, type_id) {
                    self.indicator_manager.assign_window(*instance_id, Some(chart_id_val.0));
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
                let (active_cid, bars) = self.panel_app.panel_grid.docking().active_leaf()
                    .and_then(|leaf| {
                        let cid = self.panel_app.panel_grid.chart_id_for_leaf(leaf)?;
                        let bars = self.panel_app.panel_grid.window_for_leaf(leaf)?.bars.clone();
                        Some((cid.0, bars))
                    })
                    .unwrap_or_default();
                self.indicator_manager.calculate_all_for_window(active_cid, &bars);
                self.sync_sub_panes_from_manager();
                // Propagate to sync group peers
                if let Some(leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    let (exchange, account_type) = self.panel_app.panel_grid.active_window()
                        .map(|w| (w.exchange.clone(), w.account_type.clone()))
                        .unwrap_or_default();
                    self.propagate_symbol_to_sync_group(leaf, new_symbol, &exchange, &account_type);
                }
                self.autosave_snapshot();
            }
            Command::ChangeTimeframe { new_timeframe, .. } => {
                // Recalculate indicators for new bars
                let (active_cid, bars) = self.panel_app.panel_grid.docking().active_leaf()
                    .and_then(|leaf| {
                        let cid = self.panel_app.panel_grid.chart_id_for_leaf(leaf)?;
                        let bars = self.panel_app.panel_grid.window_for_leaf(leaf)?.bars.clone();
                        Some((cid.0, bars))
                    })
                    .unwrap_or_default();
                self.indicator_manager.calculate_all_for_window(active_cid, &bars);
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
                    if let Some(style_arc) = self.panel_app.tag_manager
                        .group(group_id)
                        .map(|g| g.last_used_style.clone())
                    {
                        window.drawing_manager.bind_style_store(style_arc);
                    }
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
                symbol: String::new(),
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
    use zengeld_chart::i18n::{MenuKey, current_language};
    let lang = current_language();
    vec![
        ContextMenuItemState::action_with_icon("settings", "chart_settings", MenuKey::OpenSettings.get(lang)),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("zoom_reset", "reset_zoom", MenuKey::ResetZoom.get(lang)),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("camera", "screenshot", MenuKey::Screenshot.get(lang)),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("search", "symbol_search", MenuKey::SymbolSearch.get(lang)),
        ContextMenuItemState::separator(),
        ContextMenuItemState::action_with_icon("delete", "reset_cache", MenuKey::ResetCache.get(lang)),
        ContextMenuItemState::action_with_icon("delete", "reset_storage", MenuKey::ResetStorage.get(lang)),
    ]
}
