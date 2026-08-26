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
#[cfg(target_arch = "wasm32")]
use crate::agent as gate4agent;


mod sliders;
mod color_picker;
mod overlay;
pub mod chart_out_events;
mod panel_click;
mod sync_group;
mod modals;
mod settings;
pub(crate) mod drawing_session;
mod trading_container_bind;

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
    /// Copy the selected drawing primitive to the in-RAM clipboard (Ctrl+C
    /// fallback when nothing text-selectable consumed the key first).
    CopyPrimitive,
    /// Paste the in-RAM primitive clipboard into the active bubble (Ctrl+V
    /// fallback when no text field/PTY consumed the key first).
    PastePrimitive,
    /// Cut the selected drawing primitive: copy to the in-RAM clipboard then
    /// delete it (Ctrl+X fallback when there was no text selection to cut).
    CutPrimitive,
}

use crate::{ChartApp, sglog};
use mlc_core::{
    ChartInputAction,
    ChartOutputAction,
    ChartHitTester,
    ExtendedFrameLayout,
    LayoutRect,
    CursorStyle,
    input::DragMode,
    localize_primitive_name,
};
use mlc_app_layout::layout::ExtendedLayoutHitTester;
use mlc_app_layout::panel_app::ChartPanelLayout;
use mlc_app_layout::ui::context_menu::{
    ContextMenuTarget, ContextMenuItemState,
    build_primitive_context_menu, build_wave_menu, build_successor_menu, build_predecessor_menu,
};
use mlc_app_layout::ui::modal_settings::DualSliderHandle;
use mlc_core::drawing::TimeframeVisibilityConfig;
use mlc_core::drawing::primitives_v2::elliott::WaveLink;
use mlc_core::drawing::primitives_v2::{HitTestResult, Primitive};
use mlc_core::state::GestureTrigger;
use uzor::WidgetId;

/// Convert chart-app's `KeyPress` to the uzor `KeyPress` understood by `TextFieldStore`.
///
/// Only the variants that `TextFieldStore` acts on are mapped; everything
/// else becomes `None` and the caller skips forwarding to the store.
pub(crate) fn to_uzor_key(key: &KeyPress) -> Option<uzor::input::KeyPress> {
    use uzor::input::KeyPress as UK;
    match key {
        KeyPress::Delete      => Some(UK::Delete),
        KeyPress::ArrowLeft   => Some(UK::ArrowLeft),
        KeyPress::ArrowRight  => Some(UK::ArrowRight),
        KeyPress::ArrowUp     => Some(UK::ArrowUp),
        KeyPress::ArrowDown   => Some(UK::ArrowDown),
        KeyPress::Enter       => Some(UK::Enter),
        KeyPress::Escape      => Some(UK::Escape),
        KeyPress::Tab         => Some(UK::Tab),
        KeyPress::Backspace   => Some(UK::Backspace),
        KeyPress::CtrlC       => Some(UK::CtrlC),
        KeyPress::PageUp      => Some(UK::PageUp),
        KeyPress::PageDown    => Some(UK::PageDown),
        KeyPress::Home        => Some(UK::Home),
        KeyPress::End         => Some(UK::End),
        KeyPress::SelectAll   => Some(UK::SelectAll),
        KeyPress::ShiftLeft   => Some(UK::ShiftLeft),
        KeyPress::ShiftRight  => Some(UK::ShiftRight),
        KeyPress::ShiftHome   => Some(UK::ShiftHome),
        KeyPress::ShiftEnd    => Some(UK::ShiftEnd),
        KeyPress::Copy        => Some(UK::Copy),
        KeyPress::Paste(s)    => Some(UK::Paste(s.clone())),
        KeyPress::Undo        => Some(UK::Undo),
        KeyPress::Redo        => Some(UK::Redo),
        // Not text-field concepts — never forwarded to TextFieldStore.
        KeyPress::CopyPrimitive | KeyPress::PastePrimitive | KeyPress::CutPrimitive => None,
    }
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
                    // Only read on native — no PTY runtime to write to on wasm.
                    let _id = desc.instance_id;
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Some(rt) = self.core.native_rt() {
                        let _ = rt.block_on(self.agent.write_pty_instance(_id, text));
                    }
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
        use mlc_app_sidebar_content::agent_types::AgentSnapshotMode;
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
            // Trading center chart scale-corner buttons (A/M cycle,
            // lin/log/% mode) live INSIDE the container body widget —
            // intercept before the generic focus_content dispatch. z-order
            // is already respected: process_click returned the body only
            // because nothing (dropdown/modal) sat above it.
            if id.starts_with("slot:") && id.ends_with(":focus_content") {
                if let Some((_pid, cid, rect)) = self.trading_center_hit(x, y) {
                    if self.trading_center_corner_click(cid, x, y) {
                        return;
                    }
                    if self.trading_center_overlay_click(cid, &rect, x, y) {
                        return;
                    }
                }
            }
            eprintln!("[ChartApp] click dispatched to: {}", id);
            self.dispatch_panel_click(&id, x, y);
            return;
        }

        // 1c. Click inside a free-slot order-flow panel — route to the active panel.
        //     Panels currently receive the click but have no built-in behavior.
        //     Infrastructure is wired so behavior can be added per-panel later.
        {
            use mlc_app_sidebar_content::state::RightSidebarPanel;
            let slot_idx_opt = match self.sidebar_state.right_panel {
                RightSidebarPanel::SlotTrading   => Some(0usize),
                RightSidebarPanel::SlotDom       => Some(1),
                RightSidebarPanel::SlotTape      => Some(2),
                RightSidebarPanel::SlotL2        => Some(3),
                RightSidebarPanel::SlotBigTrades => Some(4),
                RightSidebarPanel::SlotLiq       => Some(5),
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
                                    use mlc_core::state::Panel;
                                    match item_opt {
                                        Some(Panel::Dom(_))
                                        | Some(Panel::L2Tape(_))
                                        | Some(Panel::TradeTape(_))
                                        | Some(Panel::Footprint(_))
                                        | Some(Panel::LiquidityHeatmap(_))
                                        | Some(Panel::BigTrades(_))
                                        | Some(Panel::LiqTape(_))
                                        | Some(Panel::VolumeProfile(_)) => {
                                            // Click acknowledged — no panel action yet.
                                            // Return without closing dropdowns so the click
                                            // doesn't reset UI state the user didn't touch.
                                            // EXCEPT the live symbol quick-edit: a click
                                            // into a panel body is a click-away for it
                                            // (owner defect 2026-07-31).
                                            if self.symbol_quick_edit.is_some() {
                                                self.close_symbol_quick_dropdown();
                                            }
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
        // A click that hit NO registered widget (empty toolbar strip,
        // canvas gutters) is a click-away for the live symbol quick-edit
        // too — the dispatch-path click-away block never sees it (owner
        // defect 2026-07-31: «на пустой тулбар не сбрасывает»).
        if self.symbol_quick_edit.is_some() {
            self.close_symbol_quick_dropdown();
        }
        self.close_transient_overlays();

        // 4. Split panel routing — route click to the correct leaf.
        if self.panel_app.panel_grid.is_split() {
            use crate::panel_grid::ChartInputTarget;
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
                    // Returns early below (no fallthrough to `handle_canvas_click`,
                    // which owns the general chart-canvas-click focus write) — set
                    // workspace focus back to the chart here explicitly.
                    if let Some(cid) = self.panel_app.panel_grid.container_id_for_leaf(leaf_id) {
                        self.workspace_focus = crate::focus::WorkspaceFocus::Chart(cid);
                    }
                    // Handle the scale corner click on the correct leaf's window.
                    use mlc_core::ScaleCornerButton;
                    match button {
                        ScaleCornerButton::AutoManual => {
                            use mlc_core::ScaleMode;
                            let current_mode = self.panel_app.panel_grid
                                .container_for_leaf(leaf_id)
                                .map(|w| w.price_scale().scale_mode)
                                .unwrap_or(ScaleMode::Auto);
                            let next_mode = current_mode.next();
                            let series_for_calc = self.panel_app.panel_grid
                                .container_id_for_leaf(leaf_id)
                                .map(|cid| self.active_series_for_chart(cid))
                                .unwrap_or_default();
                            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                                window.cycle_scale_mode(next_mode, series_for_calc.as_active_series());
                            }
                            // Mutate group.scale_mode when sync_viewport is on so the
                            // group remembers the mode for new joiners.
                            let viewport_sync_on = self.panel_app.panel_grid
                                .container_id_for_leaf(leaf_id)
                                .and_then(|cid| self.bubble_for_chart(cid))
                                .and_then(|gid| {
                                    self.core.bubbles_mut().set_scale_mode(gid, next_mode);
                                    self.core.bubbles().sync_flags(gid).map(|sf| sf.sync_viewport)
                                })
                                .unwrap_or(false);
                            // Propagate viewport + scale_mode to peers when sync_viewport is on.
                            if viewport_sync_on {
                                let viewport_state = self.panel_app.panel_grid
                                    .container_for_leaf(leaf_id)
                                    .map(|w| (w.viewport().view_start, w.viewport().bar_spacing));
                                if let Some((view_start, bar_spacing)) = viewport_state {
                                    self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, Some(next_mode));
                                }
                            }
                        }
                        ScaleCornerButton::Mode => {
                            self.process_output_actions(vec![mlc_core::ChartOutputAction::TogglePriceScaleMode]);
                        }
                        ScaleCornerButton::None => {}
                    }
                    return;
                }
                ChartInputTarget::Chart { leaf_id }
                | ChartInputTarget::PriceScale { leaf_id }
                | ChartInputTarget::TimeScale { leaf_id } => {
                    self.panel_app.panel_grid.set_active_leaf(leaf_id);
                    // Invalidate the cached layout so the commit path below
                    // rebuilds it for the newly active leaf's rect, not the
                    // previous leaf's (mirrors the right-click invalidation at ~522).
                    self.active_frame_layout = None;
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

    /// Deselect the drawing-primitive selection across every bubble.
    ///
    /// Called on the agent-panel text-focus TRANSITION (PTY/chat gaining
    /// focus) so text-field focus and chart primitive selection stay
    /// mutually exclusive — mirrors the deselect-all pattern used when a
    /// canvas click misses every primitive.
    pub(crate) fn deselect_all_primitives(&mut self) {
        let all_bids: Vec<_> = self.core.bubbles().bubble_ids().collect();
        for bid in all_bids {
            self.core.drawings_mut().deselect(bid);
        }
    }

    /// THE single z-order occlusion gate (owner spec 2026-07-28): true when
    /// the cursor is over any layer at or above the popup tier — dropdowns,
    /// submenus, indicator overlays, modals, context menus, color pickers.
    /// Live query (modal-barrier aware) over the last completed frame's
    /// widget accumulation. Canvas-level cursor affordances (crosshair,
    /// hover glow, overlay reveals) must consult THIS — never hand-check
    /// individual popup rects.
    ///
    /// Two checks, ORed, because a direct widget hit is not the whole
    /// story: `topmost_layer_z_at` only fires when the point lands on a
    /// widget's OWN registered rect, so a point over a modal's dimmed
    /// backdrop — or over a lower modal stacked underneath a smaller one,
    /// like the "Save as…" name dialog sitting on top of its parent
    /// settings modal — hits nothing and reports no occlusion at all
    /// (2026-08-09 fix: the crosshair leaked through both). `InputCoordinator
    /// ::is_blocked_by_modal` is the coordinator's own answer to "is this
    /// point inside the reach of any active modal layer, hit or not" and
    /// covers exactly that gap; it is a no-op (`false`) when no modal is
    /// open, so the Dropdown-tier check above still owns plain (non-modal)
    /// popups on its own.
    pub(crate) fn cursor_over_popup(&self, x: f64, y: f64) -> bool {
        let coord = self.input_coordinator.borrow();
        let over_registered_popup = coord
            .topmost_layer_z_at(x, y)
            .map_or(false, |z| {
                z >= mlc_app_layout::ui::z_order::ZLayer::Dropdown.z_order()
            });
        over_registered_popup || coord.is_blocked_by_modal(x, y)
    }

    /// True while any modal is open that should suppress chart-level
    /// right-click / primitive-copy-paste hotkeys from bleeding through.
    pub(crate) fn any_blocking_modal_open(&self) -> bool {
        self.modal_state.is_open()
            || self.standalone_panels.watchlist_modal.is_open()
            || self.standalone_panels.wl_group_name_input.is_open()
            || self.panel_app.primitive_settings_state.is_open()
            || self.panel_app.indicator_settings_state.is_open()
            || self.panel_app.alert_settings_state.is_open()
            || self.panel_app.compare_settings_state.is_open()
            || self.panel_app.chart_settings_state.is_open
            || self.panel_app.preset_name_input.is_open
            || self.panel_app.chart_browser.is_open
            || self.panel_app.overlay_settings_state.is_open
            || self.panel_app.tags_tabs_state.is_open
            || self.tc_modal.is_open
            || self.rc_modal.is_open
            || self.dom_modal.is_open
    }

    /// Ask the wave-gesture question for `(x, y)`: does it land on an
    /// Elliott primitive's own wave segment, terminal point, or origin
    /// point, and — if so — does `trigger` open a menu for it under the
    /// active [`mlc_core::state::GestureBindings`]? The ONE entry point
    /// both `on_double_click` and `on_right_click`'s primitive arm call
    /// BEFORE their own generic behaviour, returning early on `true` —
    /// see each call site's own comment for why it runs where it does
    /// relative to what was already there. Neither call site matches on
    /// `trigger` itself: that indirection is `GestureBindings`'s whole
    /// reason for existing (owner ask 2026-08-13 — "вопрос только на что
    /// это вешать на даблклик или на ПКМ хз, дискуссионно" — see that
    /// module's own doc).
    ///
    /// Resolves the primitive under the cursor through the SAME
    /// `DrawingManager::hit_test`/`hit_test_in_pane` entry points every
    /// ordinary click/right-click already calls (main chart first, then
    /// sub-panes — the same two steps `ChartPanelGrid::handle_right_click`
    /// and `handle_canvas_click` both already run) — never a second
    /// geometry search. Returns `false` immediately when nothing is hit,
    /// the hit is not an Elliott primitive (`wave_degree()` is `None`), or
    /// the primitive is individually locked (`PrimitiveData::locked` —
    /// distinct from the bubble-wide drawing-session lock `hit_test`/
    /// `hit_test_in_pane` already refuse through before ever returning a
    /// `prim_id`).
    ///
    /// The target — a wave segment vs. the structure's own terminal point
    /// vs. its own origin point (owner ask 2026-08-14: a count is read
    /// backwards as often as forwards, so building a chain must not be
    /// limited to appending after the terminal pivot) — is decided with
    /// the SAME per-primitive `hit_test` (control points first, at the
    /// drag path's own hit radius — `Primitive::control_point_hit_test`'s
    /// default, which every concrete `hit_test` override still goes
    /// through) and `wave_at` (body + label + ratio-rib search) every
    /// other consumer of those two methods already uses; this never
    /// invents a third geometry test.
    pub(crate) fn try_wave_gesture(&mut self, x: f64, y: f64, trigger: GestureTrigger) -> bool {
        use mlc_core::state::{ChartGesture, GestureTarget};

        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else {
            return false;
        };

        let _fallback;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback = self.build_extended_layout(); &_fallback }
        };
        let bars: Vec<mlc_core::Bar> = self.bars_for_bubble(bid);
        let chart_rect = extended.main_chart.chart;
        let local_x = x - chart_rect.x;
        let local_y = y - chart_rect.y;

        // Stage 1: main chart — the same `hit_test` call
        // `ChartPanelGrid::handle_right_click`'s own step 1 and
        // `handle_canvas_click`'s own main-chart branch use.
        let mut resolved: Option<(u64, f64, f64, mlc_core::Viewport, mlc_core::PriceScale, Option<u64>)> = None;
        if local_x >= 0.0 && local_x <= chart_rect.width && local_y >= 0.0 && local_y <= chart_rect.height {
            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let mut corrected_vp = window.viewport().clone();
                corrected_vp.chart_width = chart_rect.width;
                corrected_vp.chart_height = chart_rect.height;
                let price_scale = window.price_scale().clone();
                if let Some(prim_id) = self.core.drawings().hit_test(bid, local_x, local_y, &bars, &corrected_vp, &price_scale) {
                    resolved = Some((prim_id, local_x, local_y, corrected_vp, price_scale, None));
                }
            }
        }

        // Stage 2: sub-panes — the same `hit_test_in_pane` call
        // `handle_right_click`'s own step 2 uses.
        if resolved.is_none() {
            for pane_layout in extended.sub_panes.iter() {
                let content = pane_layout.content;
                let plx = x - content.x;
                let ply = y - content.y;
                if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                    continue;
                }
                let instance_id = pane_layout.instance_id;
                let Some(window) = self.panel_app.panel_grid.active_container() else { continue };
                let (price_min, price_max) = window.sub_pane_y_state.get(&instance_id)
                    .map(|s| (s.price_min, s.price_max))
                    .unwrap_or((0.0, 100.0));
                let sub_price_scale = mlc_core::PriceScale::new(price_min, price_max);
                let mut sub_viewport = window.viewport().clone();
                sub_viewport.chart_height = content.height;
                if let Some(prim_id) = self.core.drawings().hit_test_in_pane(bid, plx, ply, instance_id, &bars, &sub_viewport, &sub_price_scale) {
                    resolved = Some((prim_id, plx, ply, sub_viewport, sub_price_scale, Some(instance_id)));
                    break;
                }
            }
        }

        let Some((prim_id, hx, hy, hit_vp, hit_ps, hit_pane)) = resolved else { return false; };

        // A plain find over the bubble's own primitive list, cloned out so
        // the read lock is not held across the selection write below — the
        // same `Box<dyn Primitive>: Clone` every undo/redo snapshot path
        // already relies on.
        let prim: Box<dyn Primitive> = {
            let guard = self.core.drawings();
            match guard.primitives(bid).iter().find(|p| p.data().id == prim_id) {
                Some(p) => p.clone(),
                None => return false,
            }
        };

        if prim.wave_degree().is_none() || prim.data().locked {
            return false;
        }

        // Which control point (if any) the hit landed on — read ONCE, then
        // compared against both `origin_control_point` and
        // `terminal_control_point` rather than hit-testing twice.
        let hit_cp = match prim.hit_test(hx, hy, &bars, &hit_vp, &hit_ps) {
            HitTestResult::ControlPoint(cp) => Some(cp),
            _ => None,
        };
        let origin_hit = hit_cp.is_some() && hit_cp == prim.origin_control_point(&bars, &hit_vp, &hit_ps);
        let terminal_hit = hit_cp.is_some() && hit_cp == prim.terminal_control_point(&bars, &hit_vp, &hit_ps);
        // A structure realized down to a single point reports that SAME
        // point as both its origin (index 0) and its terminal (index
        // `realized - 1`, which is also 0 when only one point exists) —
        // `origin_control_point`/`terminal_control_point` never resolve
        // that overlap themselves (see each method's own doc), so the
        // caller must, and does it here: origin wins. A predecessor menu
        // on the structure's only placed point is the useful reading (the
        // count has nowhere to go FORWARD yet, since there is no terminal
        // pivot distinct from the origin); a successor menu on a
        // one-point structure would open on a pivot indistinguishable from
        // where the analyst is still drawing.
        let target_and_slot = if origin_hit {
            Some((GestureTarget::OriginPoint, None))
        } else if terminal_hit {
            Some((GestureTarget::TerminalPoint, None))
        } else {
            prim.wave_at(hx, hy, &bars, &hit_vp, &hit_ps).map(|slot| (GestureTarget::WaveSegment, Some(slot)))
        };
        let Some((target, wave_slot)) = target_and_slot else { return false; };
        // Right-click is not a BINDING, it is the primitive's own menu
        // growing an Elliott block — owner, 2026-08-13, after the first
        // version gated it behind the binding and so never showed it: "как
        // только мы делаем ПКМ по волнам, должно открывать сверху над
        // Settings эллиот-блок, соответствующий тому, в какую часть ткнули —
        // то же самое что даблклик левой, только ПКМ и просто сверху уже
        // имеющегося меню". So a right-click over a wave ALWAYS resolves to
        // the block for whatever it landed on, and the merge below keeps
        // Settings/Clone/Lock/Hide/Delete one click away underneath it.
        // `GestureBindings` still governs the EXCLUSIVE gesture — the one
        // that opens the wave block alone, double-click today.
        let gesture = match trigger {
            GestureTrigger::RightClick => match target {
                GestureTarget::WaveSegment => ChartGesture::WaveMenu,
                GestureTarget::TerminalPoint => ChartGesture::SuccessorMenu,
                GestureTarget::OriginPoint => ChartGesture::PredecessorMenu,
            },
            _ => match self.gesture_bindings.gesture_for(trigger, target) {
                Some(g) => g,
                None => return false,
            },
        };

        // Select so the inline toolbar and the settings modal agree with
        // what the menu is about — mirrors `on_right_click`'s own
        // select-then-open sequence for the generic primitive menu.
        self.blur_agent_focus_for_primitive_select();
        self.core.drawings_mut().set_current_pane(bid, hit_pane);
        self.core.drawings_mut().select(bid, prim_id);

        let Some(cfg) = self.core.drawings().selected_config(bid) else { return false; };
        let mut items = match (gesture, wave_slot) {
            (ChartGesture::WaveMenu, Some(slot)) => {
                let child_id = self.core.drawings().wave_child_id(bid, prim_id, slot);
                build_wave_menu(&cfg, slot, child_id)
            }
            (ChartGesture::SuccessorMenu, _) => build_successor_menu(&cfg),
            (ChartGesture::PredecessorMenu, _) => build_predecessor_menu(&cfg),
            // `GestureBindings::gesture_for` only ever maps
            // `GestureTarget::WaveSegment` to `ChartGesture::WaveMenu`, and
            // the `WaveSegment` branch above always carries a slot — this
            // arm cannot be reached in practice. Fail closed rather than
            // build a menu with no subject.
            (ChartGesture::WaveMenu, None) => return false,
        };

        // Right-click does not TAKE the primitive's own menu away from the
        // user — owner, 2026-08-13: "ПКМ перехватывается основным ПКМ, но для
        // эллиота можно просто скрестить эллиот-дропдаун и обычный ПКМ, и
        // эллиот поставит блоком сверху". So when the gesture is bound to the
        // right button the wave block goes ON TOP of the ordinary primitive
        // menu, and everything that menu offers (settings, clone, lock,
        // visibility, delete) is still one click away. Double-click has no
        // such menu to preserve and stays the wave block alone.
        if trigger == GestureTrigger::RightClick {
            let data = prim.data();
            let display_name = localize_primitive_name(prim.type_id(), prim.display_name());
            items.push(ContextMenuItemState::separator());
            items.extend(build_primitive_context_menu(&display_name, data.locked, data.visible));
        }

        let w = self.width as f64;
        let h = self.height as f64;
        self.panel_app.context_menu_state.open_smart(x, y, ContextMenuTarget::Primitive(prim_id), items, w, h);
        eprintln!("[ChartApp] wave gesture menu opened for primitive #{}", prim_id);
        true
    }

    /// Handle a right-click at screen coordinates `(x, y)`.
    ///
    /// Opens a context menu: primitive menu if clicking on a primitive,
    /// chart background menu otherwise.
    pub fn on_right_click(&mut self, x: f64, y: f64) {
        // Suppress context menu when any modal is open — right-clicks inside
        // modals must not bleed through to the chart background.
        if self.any_blocking_modal_open() {
            return;
        }

        // Close any existing context menu / dropdown first.
        self.panel_app.context_menu_state.close();
        self.panel_app.toolbar_state.open_dropdown_id = None;
        self.granularity_edit = None;
        self.granularity_drag_pending = None;
        self.granularity_drag = None;

        // Make the window UNDER THE CURSOR active before hit-testing, so the
        // context menu opens in the hovered window (not whichever was focused).
        // Mirrors the left-click path (resolve_input → set_active_leaf).
        if self.panel_app.panel_grid.is_split() {
            use crate::panel_grid::ChartInputTarget;
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

        // A right-click on the chart (split or single-window) returns
        // workspace focus to the chart, same as the left-click path in
        // `handle_canvas_click` — mirrors the "make the window under the
        // cursor active" comment above: this IS a chart-activation gesture.
        if let Some(cid) = self.panel_app.panel_grid.active_container_id() {
            self.workspace_focus = crate::focus::WorkspaceFocus::Chart(cid);
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
        let (viewport, price_scale, sub_pane_ranges) = {
            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let ranges: Vec<(u64, f64, f64)> = window.sub_pane_y_state
                    .iter()
                    .map(|(&iid, s)| (iid, s.price_min, s.price_max))
                    .collect();
                (
                    window.viewport().clone(),
                    window.price_scale().clone(),
                    ranges,
                )
            } else {
                (
                    mlc_core::Viewport::default(),
                    mlc_core::PriceScale::new(0.0, 1.0),
                    Vec::new(),
                )
            }
        };

        let overlay_bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        let indicator_overlay_hit = |lx: f64, ly: f64, chart_height: f64| {
            let bid = overlay_bid?;
            self.core.indicators().hit_test_overlay(
                bid, lx, ly, &viewport, &price_scale, chart_height, 8.0,
            )
        };
        let indicator_subpane_hit = |instance_id: u64, plx: f64, ply: f64, pane_height: f64| {
            let (price_min, price_max) = sub_pane_ranges
                .iter()
                .find(|(id, _, _)| *id == instance_id)
                .map(|&(_, mn, mx)| (mn, mx))
                .unwrap_or((0.0, 100.0));
            self.core.indicators().hit_test_sub_pane(
                instance_id, plx, ply, &viewport, price_min, price_max, pane_height, 8.0,
            )
        };

        let active_bid_rc = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        let active_bars: Vec<mlc_core::Bar> = active_bid_rc
            .and_then(|bid| self.with_bubble(bid, |b| b.bars.clone()))
            .unwrap_or_default();
        let hit = if let Some(bid) = active_bid_rc {
            let mut dreg = self.core.drawings_mut();
            let mut session = crate::input::drawing_session::DrawingSessionView::new(&mut dreg, bid);
            self.panel_app.panel_grid.handle_right_click(
                x as f64, y as f64, extended,
                indicator_overlay_hit,
                indicator_subpane_hit,
                &mut session,
                &active_bars,
            )
        } else {
            crate::panel_grid::ChartRightClickHit::Miss
        };

        match hit {
            crate::panel_grid::ChartRightClickHit::Primitive { prim_id } => {
                // Wave-gesture second control surface (owner ask
                // 2026-08-13): when `GestureBindings::wave_menu`/
                // `successor_menu` is bound to RightClick, a right-click on
                // an Elliott wave's own segment or terminal point opens its
                // own popup instead of the generic primitive menu below.
                // Bound to DoubleClick by default, so this is a no-op today
                // — flipping the binding is the only thing that changes
                // that, never this call site.
                if self.try_wave_gesture(x, y, GestureTrigger::RightClick) {
                    return;
                }

                // Right-clicked on a primitive — look up info via primitive_list.
                let (display_name, is_locked, is_visible) = active_bid_rc
                    .and_then(|bid| {
                        self.core.drawings().primitive_list(bid)
                            .into_iter()
                            .find(|item| item.id == prim_id)
                            .map(|item| {
                                let localized = localize_primitive_name(&item.type_id, &item.display_name);
                                (localized, item.locked, item.visible)
                            })
                    })
                    .unwrap_or_else(|| ("Primitive".to_string(), false, true));

                let items = build_primitive_context_menu(&display_name, is_locked, is_visible);

                // Select the primitive so it highlights. Text focus and
                // primitive selection are mutually exclusive.
                if let Some(bid) = active_bid_rc {
                    self.blur_agent_focus_for_primitive_select();
                    self.core.drawings_mut().select(bid, prim_id);
                }

                self.panel_app.context_menu_state.open_smart(
                    x, y,
                    ContextMenuTarget::Primitive(prim_id),
                    items,
                    w, h,
                );
                eprintln!("[ChartApp] Context menu opened for primitive #{}", prim_id);
            }
            crate::panel_grid::ChartRightClickHit::Indicator { indicator_id } => {
                // Right-clicked on an indicator line — select it and open its settings.
                self.selected_indicator_id = Some(indicator_id);
                self.panel_app.indicator_settings_state.open(indicator_id);
                eprintln!("[ChartApp] Indicator right-clicked: id={}, settings opened", indicator_id);
            }
            crate::panel_grid::ChartRightClickHit::Background => {
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
            crate::panel_grid::ChartRightClickHit::Miss => {}
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

        // Trading center chart: double-click through the aux engine handler —
        // scale-strip double-clicks reset price/time scales with the exact
        // main-chart semantics (chart-in-container arc, 2026-07-26).
        if let Some((_pid, cid, rect)) = self.trading_center_hit(x, y) {
            self.trading_center_process(
                cid,
                &rect,
                mlc_core::engine::input::ChartInputAction::DoubleClick { x, y },
            );
            return;
        }

        // Wave-gesture second control surface (owner ask 2026-08-13): a
        // double-click on an Elliott wave's own segment or terminal point
        // opens its own popup. Runs BEFORE the generic drawing
        // double-click behaviour further down (which only finishes an
        // in-progress multipoint drawing — meaningless for an
        // already-placed primitive, so the two never compete for the same
        // click).
        if self.try_wave_gesture(x, y, GestureTrigger::DoubleClick) {
            return;
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
                            let chat_wid = uzor::WidgetId::from(crate::text_input::AGENT_CHAT);
                            let chat_already_focused = self.input_coordinator.borrow().text_fields().is_focused(&chat_wid);
                            self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_CHAT);
                            if !chat_already_focused {
                                self.deselect_all_primitives();
                            }
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
                            use mlc_core::state::Panel;
                            match item_opt {
                                Some(Panel::Dom(pid)) => {
                                    if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                        state.auto_center = true;
                                        state.center_price = state.market_price;
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                                Some(item @ Panel::L2Tape(_))
                                | Some(item @ Panel::Footprint(_))
                                | Some(item @ Panel::BigTrades(_))
                                | Some(item @ Panel::LiqTape(_))
                                | Some(item @ Panel::LiquidityHeatmap(_))
                                | Some(item @ Panel::VolumeProfile(_)) => {
                                    let local_id = match &item {
                                        Panel::L2Tape(_) => "l2tape:body",
                                        Panel::Footprint(_) => "footprint:body",
                                        Panel::BigTrades(_) => "bigtrades:body",
                                        Panel::LiqTape(_) => "liqtape:body",
                                        Panel::LiquidityHeatmap(_) => "heatmap:body",
                                        Panel::VolumeProfile(_) => "volprofile:body",
                                        _ => unreachable!(),
                                    };
                                    if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                        panel.handle_double_click(local_id, x, y);
                                        self.sidebar_data_dirty = true;
                                    }
                                }
                                Some(Panel::TradeTape(pid)) => {
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
        let overlay_results_dc = self.panel_app.panel_grid.active_container()
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
        if let mlc_core::engine::input::HitResult::SubPanePriceScale { pane_index } = hit {
            use mlc_core::ScaleMode;
            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                let instance_id = crate::chart_container::collect_inner_instance_ids(
                    &window.inner_docking,
                ).into_iter().nth(pane_index);
                if let Some(iid) = instance_id {
                    window.sub_pane_y_state.entry(iid).or_default().auto_scale = true;
                }
                // Restore the main chart to Auto so the A/M button reflects the true state.
                window.set_scale_mode(ScaleMode::Auto);
            }
            // Propagate Auto mode to sync-group peers.
            let viewport_state = self.panel_app.panel_grid.active_container()
                .map(|w| (w.viewport().view_start, w.viewport().bar_spacing));
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
        // Modal resize. The grab zones are ordinary widgets the shell registers
        // last on the modal layer, so the coordinator hands them over like any
        // other drag; this runs first only because a gesture that starts on a
        // frame edge must not be read as anything else.
        //
        // It used to hit-test the edges against `frame_result` instead, on the
        // belief that a zone registered after the body never reached a drag
        // start. It did reach it — `process_drag_start` returned the id and had
        // already begun the drag. Nothing here accepted an id that was not a
        // `:header`, so it fell through the match and was dropped, and the zone
        // carried DRAG without HOVER, so the `is_hovered` probe used to confirm
        // the theory answered false by construction.
        {
            use mlc_app_layout::ui::modal_settings::ResizableModal;
            use mlc_app_layout::ui::widgets::ResizeEdge;

            let grip = self
                .input_coordinator
                .borrow()
                .hit_test_now(x, y)
                .map(|w| w.as_str().to_string())
                .and_then(|id| {
                    let (ns, edge) = id.split_once(":resize:")?;
                    Some((ns.to_string(), ResizeEdge::from_id(edge)?))
                });
            if let Some((ns, edge)) = grip {
                // The frame the drag starts from: the geometry as RENDERED,
                // which the state itself does not know until it has been
                // resized once.
                let frame = self.frame_result.as_ref().and_then(|r| match ns.as_str() {
                    "chart_settings" => r.chart_settings.as_ref().map(|m| m.modal_rect),
                    "ind_settings" => r.indicator_settings.as_ref().map(|m| m.modal_rect),
                    "prim_settings" => r.primitive_settings.as_ref().map(|m| m.modal_rect),
                    _ => None,
                });
                if let Some(f) = frame {
                    let (pos, size) = ((f.x, f.y), (f.width, f.height));
                    match ns.as_str() {
                        "chart_settings" => self
                            .panel_app
                            .chart_settings_state
                            .start_resize(edge, (x, y), pos, size),
                        "prim_settings" => self
                            .panel_app
                            .primitive_settings_state
                            .start_resize(edge, (x, y), pos, size),
                        _ => self
                            .panel_app
                            .indicator_settings_state
                            .start_resize(edge, (x, y), pos, size),
                    }
                    // FALSE, like every modal drag below: `true` here means "a
                    // popup was dismissed, drop this press", and the shell
                    // answers it by calling on_drag_end at once — which would
                    // cancel the resize on the frame it started.
                    self.ui_drag_active = true;
                    return false;
                }
            }
        }

        // Compute live hit-test results once and drop the borrow immediately.
        // Using stale hovered_widget()/is_over_ui() snapshots here caused the
        // "first gesture lost after focus" bug — the snapshot is one frame stale
        // and arbitrarily stale right after OS window focus.
        let (live_over_ui, live_hover): (bool, Option<String>) = {
            let coord = self.input_coordinator.borrow();
            (coord.is_over_ui_now(x, y), coord.hit_test_now(x, y).map(|h| h.as_str().to_string()))
        };

        // Track whether this drag started on a UI element (for crosshair suppression).
        self.ui_drag_active = live_over_ui;

        // ── Agent-panel separator drag initiation ────────────────────────────
        // MUST run BEFORE PTY/Chat drag so separator hit zones take priority
        // over the leaf content areas that may overlap the separator zone.
        if self.agent_sep_drag.is_none() {
            let hovered_wid = live_hover.clone();
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
                            .get(&hovered_leaf_id).map(|s| s.as_str()).unwrap_or_default();
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
                            mlc_app_sidebar_content::state::PtySelection::new(row, col),
                        );
                    }
                    self.agent_pty_drag_active = true;
                    // Also focus PTY so keyboard events still route to it.
                    // Text focus and primitive selection are mutually exclusive —
                    // only deselect on the actual focus transition, not every drag.
                    let pty_wid = uzor::WidgetId::from(crate::text_input::AGENT_PTY);
                    let pty_already_focused = self.input_coordinator.borrow().text_fields().is_focused(&pty_wid);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_PTY);
                    if !pty_already_focused {
                        self.deselect_all_primitives();
                    }
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
                    // Text focus and primitive selection are mutually exclusive —
                    // only deselect on the actual focus transition.
                    let chat_wid = uzor::WidgetId::from(crate::text_input::AGENT_CHAT);
                    let chat_already_focused = self.input_coordinator.borrow().text_fields().is_focused(&chat_wid);
                    self.input_coordinator.borrow_mut().text_fields_mut().focus(crate::text_input::AGENT_CHAT);
                    if !chat_already_focused {
                        self.deselect_all_primitives();
                    }
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
                                        if let mlc_app_sidebar_content::agent_types::AgentSnapshotMode::Chat(ref msgs) = snap.mode {
                                            msgs.get(msg_idx as usize)
                                        } else {
                                            None
                                        }
                                    })
                                    .map(|msg| msg.role == mlc_app_sidebar_content::agent_types::ChatRole::Thinking || msg.role == mlc_app_sidebar_content::agent_types::ChatRole::Tool)
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
                                mlc_app_sidebar_content::state::ChatSelection::new_at(msg_idx, line_idx, char_idx),
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

        // Granularity FAVORITES/CUSTOM row drag — begin pending drag-to-
        // reorder (owner spec 2026-07-16 items 2+3). Mirrors the watchlist
        // modal's mousedown arm exactly: find the row via the LIVE hovered
        // widget id, store as PENDING (not yet an active drag), do NOT
        // return early — a short click still falls through to the normal
        // `on_click` dropdown-item routing further down the pipeline.
        // Skipped entirely while the inline text editor is active (owner:
        // trailing-icon zones/other rows are inert during edit — same
        // contract `panel_click.rs`'s dropdown-item guard already enforces)
        // and while a reorder-drag is already pending/active (avoids
        // re-arming mid-gesture).
        if self.granularity_edit.is_none()
            && self.granularity_drag_pending.is_none()
            && self.granularity_drag.is_none()
        {
            if let Some(row_id) = live_hover.as_deref()
                .and_then(|h| h.strip_prefix("dropdown:timeframe_selector:"))
            {
                if let Some((family, section, index)) = self.resolve_granularity_reorder_row(row_id) {
                    self.granularity_drag_pending = Some(crate::GranularityDragPending {
                        family, section, from_index: index, start_x: x, start_y: y,
                    });
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
            fn outside_popup(picker: &mlc_app_layout::ui::color_picker_state::ColorPickerState, x: f64, y: f64) -> bool {
                if !picker.is_open() { return false; }
                let (ox, oy) = picker.origin;
                let (pw, ph) = match picker.level {
                    mlc_app_layout::ui::color_picker_state::ColorPickerLevel::L1 => picker.l1_config().calculate_size(),
                    mlc_app_layout::ui::color_picker_state::ColorPickerLevel::L2 => picker.l2_config().calculate_size(),
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
        let on_sidebar_separator = live_hover.as_deref() == Some("right_sidebar_separator");
        if on_sidebar_separator {
            self.sidebar_separator_drag_active = true;
            return false;
        }

        // Check if drag starts on a watchlist column separator — begin separator drag.
        // Must be checked before the watchlist row drag so separators win the hit-test.
        if self.sidebar_state.right_panel == mlc_app_sidebar_content::state::RightSidebarPanel::Watchlist {
            // Widget ids use 1-based indexing; strip prefix and subtract 1 for 0-based sep index.
            let on_sep = live_hover.as_deref()
                .and_then(|h| h.strip_prefix("watchlist_sep_").and_then(|s| s.parse::<usize>().ok()))
                .map(|one_based| one_based.saturating_sub(1));
            // Skip separator drag when columns are in equal-width (aligned) mode.
            let align_cols = self.watchlist_ui.column_config(self.watchlist_ui.active_list_id).align_columns;
            if let Some(sep_idx) = on_sep.filter(|_| !align_cols) {
                // Compute the current separator absolute X offset from area_left.
                let item_padding = 8.0_f64;
                let scrollbar_width = 8.0_f64;
                let content_width = self.sidebar_state.right_sidebar_width - scrollbar_width;
                let usable_w = content_width - item_padding * 2.0;
                let area_left = item_padding; // relative to sidebar left edge

                // Determine current separator offset from area_left.
                let sep_offset_at_start = {
                    let c = self.watchlist_ui.column_config(self.watchlist_ui.active_list_id);
                    let n_seps = {
                        let mut n = 0usize; // symbol always present
                        if c.show_exchange     { n += 1; }
                        if c.show_account_type { n += 1; }
                        if c.show_last_price   { n += 1; }
                        if c.show_change_pct   { n += 1; }
                        if c.show_spark        { n += 1; }
                        if c.show_change_abs   { n += 1; }
                        if c.show_high_low     { n += 2; }
                        if c.show_volume       { n += 1; }
                        n
                    };

                    let use_custom = c.separator_offsets.as_ref()
                        .map(|o| o.len() == n_seps)
                        .unwrap_or(false);

                    if use_custom {
                        c.separator_offsets.as_ref()
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
        if self.sidebar_state.right_panel == mlc_app_sidebar_content::state::RightSidebarPanel::Watchlist {
            let on_watchlist_row = live_hover.as_deref()
                .and_then(|h| h.strip_prefix("watchlist_").and_then(|s| s.parse::<usize>().ok()));
            if let Some(idx) = on_watchlist_row {
                // Verify this is a plain index (not "watchlist_delete_N" etc.).
                let hovered_id = live_hover.as_deref().unwrap_or_default();
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
            let hovered_wid = live_hover.clone();
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

        // ── Trading center chart drag initiation ────────────────────────────
        // (chart-in-container arc, 2026-07-26). FULL engine mechanics: the
        // aux handler's DragStart hit-tests the aux layout itself, so this
        // one gesture covers chart pan AND price/time scale drags. Raw rect
        // hit-test on the render-cached center rect, gated on the live
        // hover target being the container body (`focus_content`) so any
        // dropdown/modal layered above the chart keeps winning the z-order.
        if !self.trading_center_drag_active && self.active_drag_panel.is_none() {
            let over_body = live_hover
                .as_ref()
                .map(|w| w.starts_with("slot:") && w.ends_with(":focus_content"))
                .unwrap_or(false);
            if over_body {
                if let Some((pid, cid, rect)) = self.trading_center_hit(x, y) {
                    // Sub-pane separator drag wins over the engine gesture
                    // (same priority the main path gives it in
                    // `handle_drag_start`'s SubPaneSeparator arm).
                    if let Some(instance_id) = self.trading_center_sep_hit(cid, &rect, x, y) {
                        self.trading_center_sep_drag = Some(instance_id);
                        self.ui_drag_active = true;
                        return false;
                    }
                    self.trading_center_drag_active = true;
                    self.ui_drag_active = true;
                    self.workspace_focus = crate::focus::WorkspaceFocus::TradingPanel(pid);
                    // Focus follows the click into the embedded chart too
                    // (the engine gesture bypasses widget routing, so the
                    // container's click-focus hook never sees it).
                    if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
                        use mlc_app_panels::trading::trading::trading_container::TcLeaf;
                        tc.focus_free_kind(&TcLeaf::Chart);
                    }
                    self.trading_center_process(
                        cid,
                        &rect,
                        mlc_core::engine::input::ChartInputAction::DragStart {
                            mode: mlc_core::engine::input::DragMode::None,
                            x,
                            y,
                        },
                    );
                    return false;
                }
            }
        }

        // ── Slot panel drag initiation ───────────────────────────────────────
        // Detect drag on `slot:{idx}:leaf:{leaf_id}:dom:body` (DOM BlackboxPanel)
        // or `slot:{idx}:leaf:{leaf_id}:focus_content` (other panel types).
        // DOM is now registered as a BlackboxPanel widget with id `slot:N:leaf:M:dom:body`.
        // A TradingContainer-hosted DOM emits this SAME widget-id shape (the
        // container forwards its own `slot_prefix` unchanged — see
        // `TradingContainerState::render`) — matched alongside `Panel::Dom`
        // below so the ladder recenter-drag works identically nested inside
        // the container (W1, 2026-07-25).
        if self.active_drag_panel.is_none() {
            let hovered_wid = live_hover.clone();
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
                                        use mlc_core::state::Panel;
                                        if let Some(item @ (Panel::Dom(_) | Panel::TradingContainer(_))) = item_opt {
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
                                        use mlc_core::state::Panel;
                                        match item_opt {
                                            Some(item @ Panel::L2Tape(_))
                                            | Some(item @ Panel::Footprint(_))
                                            | Some(item @ Panel::BigTrades(_))
                                            | Some(item @ Panel::LiqTape(_))
                                            | Some(item @ Panel::LiquidityHeatmap(_))
                                            | Some(item @ Panel::VolumeProfile(_)) => {
                                                let local_id = match &item {
                                                    Panel::L2Tape(_) => "l2tape:body",
                                                    Panel::Footprint(_) => "footprint:body",
                                                    Panel::BigTrades(_) => "bigtrades:body",
                                                    Panel::LiqTape(_) => "liqtape:body",
                                                    Panel::LiquidityHeatmap(_) => "heatmap:body",
                                                    Panel::VolumeProfile(_) => "volprofile:body",
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
                                            Some(Panel::TradeTape(pid)) => {
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

        // ── TradingContainer separator drag initiation ────────────────────────
        // Detect drag on `slot:{idx}:leaf:{leaf_id}:sep:{name}` — one of the 4
        // v1 region separators (`TradingSeparator::wire_id`). Separate block
        // from the DOM/focus_content one above because these are plain
        // registered widgets (not BlackboxPanel composites), so a direct
        // `live_hover` suffix match is enough — no coordinator z-order
        // subtlety to route around.
        if self.active_drag_panel.is_none() {
            if let Some(ref wid) = live_hover {
                if let Some(rest) = wid.strip_prefix("slot:") {
                    if let Some((slot_str, after_slot)) = rest.split_once(":leaf:") {
                        if let Ok(slot_idx) = slot_str.parse::<usize>() {
                            if slot_idx < 4 {
                                if let Some((leaf_id_str, local_id)) = after_slot.split_once(':') {
                                    // Region separators, table column
                                    // boundaries (`{table}:colsep:{i}`) OR
                                    // the ticker-tape hand-scroll
                                    // (`tape:body`), owner spec 2026-07-28.
                                    // Colseps also live on the STANDALONE
                                    // Positions / Trade Log panels, so
                                    // those panel kinds are accepted
                                    // alongside the container.
                                    let is_colsep = local_id.contains(":colsep:");
                                    if mlc_app_panels::trading::trading::trading_container::TradingSeparator::from_wire_id(local_id).is_some()
                                        || is_colsep
                                        || local_id == "tape:body"
                                        || local_id.starts_with("tcfree:sep:")
                                        || local_id.starts_with("tcfree:grip:")
                                        || local_id.starts_with("tcfree:tab:")
                                    {
                                        if let Ok(raw) = leaf_id_str.parse::<u64>() {
                                            let leaf_id = uzor::panels::LeafId(raw);
                                            let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                                .inner()
                                                .tree()
                                                .leaf(leaf_id)
                                                .and_then(|l| l.active_panel().cloned());
                                            use mlc_core::state::Panel;
                                            if let Some(item @ (Panel::TradingContainer(_)
                                                | Panel::PositionManager(_)
                                                | Panel::TradeLog(_))) = item_opt
                                            {
                                                if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                                    if panel.handle_drag_start(local_id, x, y) {
                                                        self.active_drag_panel = Some((item, local_id.to_string(), x, y));
                                                        self.ui_drag_active = true;
                                                        return false;
                                                    }
                                                }
                                            }
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
                    let on_sep = live_hover.as_deref() == Some("right_sidebar_separator");
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

        // Primitive-settings Template menu row drag — begin drag-to-reorder
        // (item 4, 2026-08-08 review). Mirrors the watchlist modal's own
        // `drag_reorder_pending` precedent above: a raw geometric check
        // against last frame's own row rects (`template_menu_rows`), not an
        // InputCoordinator DRAG-sense registration — a short click on the
        // same rect still reaches its own CLICK-sense row handler untouched.
        if self.panel_app.primitive_settings_state.template_dropdown_open {
            if let Some(ref ps) = self.frame_result.as_ref().and_then(|r| r.primitive_settings.as_ref()) {
                // Guard: starting the drag on the row's own rename/delete
                // icon must stay a plain click, not a reorder — same
                // guard shape as the watchlist modal's own `over_delete`.
                let over_icon = live_hover.as_deref()
                    .and_then(|h| h.strip_prefix("prim_settings:item:"))
                    .map(|h| h.starts_with("template_delete:") || h.starts_with("template_rename:"))
                    .unwrap_or(false);
                if !over_icon {
                    let found_key = ps.template_menu_rows.iter()
                        .find(|(_, rect)| rect.contains(x, y))
                        .map(|(key, _)| key.clone());
                    if let Some(key) = found_key {
                        self.panel_app.primitive_settings_state.template_drag_reorder_pending = Some((key, x, y));
                    }
                }
            }
        }

        // Modal header drags — dispatched via InputCoordinator registration.
        // Each modal registers its header rect with Sense::DRAG during render.
        // Bind first so the RefMut is dropped before the body: handlers below
        // take `&mut self` (a drag start that must snapshot state), and a
        // temporary living across the whole `if let` would deny them.
        let drag_start_wid = self.input_coordinator.borrow_mut().process_drag_start(x, y);
        if let Some(wid) = drag_start_wid {
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
                    #[cfg(feature = "cloud")]
                    "cloud_login" => {
                        if let Some(ref cl) = self.frame_result.as_ref().and_then(|r| r.cloud_login.as_ref()) {
                            self.panel_app.cloud_login_state.start_drag(x, y, cl.header_rect.x, cl.header_rect.y);
                            true
                        } else { false }
                    }
                    #[cfg(feature = "cloud")]
                    "cloud_overlay" => {
                        if let Some(ref co) = self.frame_result.as_ref().and_then(|r| r.cloud_overlay.as_ref()) {
                            self.panel_app.cloud_overlay_state.start_drag(x, y, co.header_rect.x, co.header_rect.y);
                            true
                        } else { false }
                    }
                    "ewa_engine_settings" => {
                        if let Some(ref es) = self.frame_result.as_ref().and_then(|r| r.ewa_engine_settings.as_ref()) {
                            self.panel_app.ewa_engine_settings_state.start_drag(x, y, es.header_rect.x, es.header_rect.y);
                            true
                        } else { false }
                    }
                    "markup_engine_settings" => {
                        if let Some(ref ms) = self.frame_result.as_ref().and_then(|r| r.markup_engine_settings.as_ref()) {
                            self.panel_app.markup_engine_settings_state.start_drag(x, y, ms.header_rect.x, ms.header_rect.y);
                            true
                        } else { false }
                    }
                    "tags_tabs" => {
                        if let Some(ref tt) = self.frame_result.as_ref().and_then(|r| r.tags_tabs.as_ref()) {
                            self.panel_app.tags_tabs_state.start_drag(x, y, tt.header_rect.x, tt.header_rect.y);
                            true
                        } else { false }
                    }
                    "tc_modal" => {
                        if let Some(ref tm) = self.tc_modal_result {
                            self.tc_modal.start_drag(x, y, tm.header_rect.x, tm.header_rect.y);
                            true
                        } else { false }
                    }
                    "rc_modal" => {
                        if let Some(ref rm) = self.rc_modal_result {
                            self.rc_modal.start_drag(x, y, rm.header_rect.x, rm.header_rect.y);
                            true
                        } else { false }
                    }
                    "dom_modal" => {
                        let (hx, hy) = self.dom_modal.position.unwrap_or((
                            (self.width as f64 - crate::dom_settings_modal::DOM_MODAL_W) / 2.0,
                            self.height as f64 * 0.2,
                        ));
                        self.dom_modal.start_drag(x, y, hx, hy);
                        true
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
                    // `cmp_settings`, matching the modal's widget-id namespace:
                    // the shell names the header after it, where this modal
                    // used to register its own header under the layer name.
                    "cmp_settings" => {
                        if let Some(ref cs) = self.frame_result.as_ref().and_then(|r| r.compare_settings.as_ref()) {
                            self.panel_app.compare_settings_state.start_drag(x, y, cs.header_rect.x, cs.header_rect.y);
                            true
                        } else { false }
                    }
                    "preset_name_input" => {
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
                            self.standalone_panels.wl_group_name_input.start_drag(x, y, gni.modal_rect.x, gni.modal_rect.y);
                            true
                        } else { false }
                    }
                    "wl_modal" => {
                        if let Some(ref wl) = self.last_watchlist_modal_result {
                            self.standalone_panels.watchlist_modal.start_drag(x, y, wl.modal_rect.x, wl.modal_rect.y);
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
                    // Mark a UI drag so on_drag_move suppresses the chart
                    // crosshair while the modal is being moved. Without this
                    // the crosshair stays frozen at its last chart position
                    // (it belongs to a layer BELOW the modal, but the move
                    // path never hides it). Mirrors the sidebar-scroll drag
                    // above which already sets this flag.
                    self.ui_drag_active = true;
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
                } else if prefix == "tc_modal" {
                    self.tc_modal.scroll.start_drag(y);
                    true
                } else if prefix == "tags_tabs" {
                    let scroll_state = tags_tabs_active_scroll(&mut self.panel_app.tags_tabs_state);
                    scroll_state.start_drag(y);
                    true
                } else if prefix == "user_settings" {
                    use mlc_app_layout::ui::modal_settings::UserSettingsTab;
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
                } else if prefix == "prim_settings" {
                    self.panel_app.primitive_settings_state.scroll.start_drag(y);
                    true
                } else if prefix == "alert_settings" {
                    self.panel_app.alert_settings_state.list_scroll.start_drag(y);
                    true
                } else if prefix == "search_modal" {
                    self.modal_state.scroll.start_drag(y);
                    true
                } else if prefix == "watchlist_modal" {
                    self.standalone_panels.watchlist_modal.scroll.start_drag(y);
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
                        self.capture_compare_style_for_drag();
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
                    self.capture_compare_style_for_drag();
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
                        let active_bid_tf = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                        let handle = if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                            if let Some(data) = active_bid_tf
                                .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
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
                                    if let Some(ref active_fid) = self.panel_app.primitive_settings_state.active_field_id {
                                        if *active_fid == format!("tf_{}_min", tf_idx) {
                                            DualSliderHandle::Min
                                        } else if *active_fid == format!("tf_{}_max", tf_idx) {
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
                        if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                            self.capture_primitive_json_for_drag(prim_id);
                        }
                        self.panel_app.primitive_settings_state.start_dual_slider_drag_from_track(
                            &fid, track_x, track_width, min_val, max_val, handle, x,
                        );
                        return false;
                    }
                    // The gesture is ONE undo step; capture where it began,
                    // because the live writes below land on every pixel.
                    if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                        self.capture_primitive_json_for_drag(prim_id);
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
                                let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(char_positions, x);
                                drag_field = Some(field_id.clone());
                                drag_cursor = new_cursor;
                                break;
                            }
                        }
                    }
                    if let Some(field_id) = drag_field {
                        // Route drag start to TextFieldStore: focus the tapped field,
                        // set cursor + selection anchor, clear selection on all others.
                        // Also update the *_focused booleans that drive proxy population.
                        let uss = &mut self.panel_app.user_settings_state;
                        let focused_wid: Option<WidgetId> = match field_id.as_str() {
                            "e2e_passphrase_input" | "wizard_passphrase_input"
                            | "profile_mgr:new_passphrase_input" => {
                                uss.new_passphrase_focused = true;
                                uss.new_profile_name_focused = false;
                                uss.confirm_passphrase_focused = false;
                                uss.recovery_key_display_focused = false;
                                Some(WidgetId::from(crate::text_input::NEW_PASSPHRASE))
                            }
                            "wizard_profile_name_input" | "profile_mgr:name_input" => {
                                uss.new_profile_name_focused = true;
                                uss.new_passphrase_focused = false;
                                uss.confirm_passphrase_focused = false;
                                uss.recovery_key_display_focused = false;
                                Some(WidgetId::from(crate::text_input::NEW_PROFILE_NAME))
                            }
                            "profile_mgr:recovery_key_input" => {
                                uss.recovery_key_display_focused = true;
                                uss.new_passphrase_focused = false;
                                uss.new_profile_name_focused = false;
                                uss.confirm_passphrase_focused = false;
                                Some(WidgetId::from(crate::text_input::RECOVERY_KEY))
                            }
                            "profile_mgr:confirm_passphrase_input"
                            | "wizard_confirm_passphrase_input"
                            | "profile_mgr:create_confirm_passphrase_input" => {
                                uss.confirm_passphrase_focused = true;
                                uss.new_passphrase_focused = false;
                                uss.new_profile_name_focused = false;
                                uss.recovery_key_display_focused = false;
                                Some(WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE))
                            }
                            "profile_mgr:recovery_key_display" => {
                                uss.recovery_key_display_focused = true;
                                uss.new_passphrase_focused = false;
                                uss.new_profile_name_focused = false;
                                uss.confirm_passphrase_focused = false;
                                Some(WidgetId::from(crate::text_input::RECOVERY_KEY))
                            }
                            _ => None,
                        };
                        if let Some(wid) = focused_wid {
                            let mut ic = self.input_coordinator.borrow_mut();
                            let tf = ic.text_fields_mut();
                            tf.focus(wid.clone());
                            if let Some(fs) = tf.field_state_mut(&wid) {
                                fs.cursor = drag_cursor;
                                fs.selection_start = Some(drag_cursor);
                            }
                        }
                        self.panel_app.user_settings_state.profile_mgr_text_select_dragging = Some(field_id.clone());
                        eprintln!("[ChartApp] profile_mgr text select drag started: {} at char {}", field_id, drag_cursor);
                        return false;
                    }
                }
            }
            // Preset name input — text select drag on the input field (TextFieldStore).
            if let Some(ref pni) = result.preset_name_input {
                if pni.input_rect.contains(x, y) && self.panel_app.preset_name_input.is_open {
                    self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                    return false;
                }
            }
            // Chart browser — text select drag on the search input field (TextFieldStore).
            if let Some(ref br) = result.chart_browser {
                if br.search_input_rect.contains(x, y) && self.panel_app.chart_browser.is_open {
                    self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                    return false;
                }
            }
            // Primitive settings — text select drag on the active sub-field (TextFieldStore).
            if self.panel_app.primitive_settings_state.active_field_id.is_some()
                && self.panel_app.primitive_settings_state.is_open()
            {
                if let Some(ref ps) = result.primitive_settings {
                    if let Some(input_rect) = ps.active_input_rect {
                        if input_rect.contains(x, y) {
                            self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                            return false;
                        }
                    }
                }
            }
            // Indicator settings — text select drag on the active sub-field (TextFieldStore).
            if self.panel_app.indicator_settings_state.active_field_id.is_some()
                && self.panel_app.indicator_settings_state.is_open()
            {
                if let Some(ref is) = result.indicator_settings {
                    if let Some(input_rect) = is.active_input_rect {
                        if input_rect.contains(x, y) {
                            self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                            return false;
                        }
                    }
                }
            }
            // Chart settings — text select drag on the active sub-field (TextFieldStore).
            if self.panel_app.chart_settings_state.active_field_id.is_some()
                && self.panel_app.chart_settings_state.is_open
            {
                if let Some(ref cs) = result.chart_settings {
                    if let Some(input_rect) = cs.active_input_rect {
                        if input_rect.contains(x, y) {
                            self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                            return false;
                        }
                    }
                }
            }
            // Compare settings — text select drag on the active tf_min/tf_max input (TextFieldStore).
            if self.panel_app.compare_settings_state.active_field_id.is_some()
                && self.panel_app.compare_settings_state.is_open()
            {
                if let Some(ref cs_result) = result.compare_settings {
                    if let Some(input_rect) = cs_result.tf_active_input_rect {
                        if input_rect.contains(x, y) {
                            self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                            return false;
                        }
                    }
                }
            }
        }

        // Watchlist group name input — text select drag on the input field (TextFieldStore).
        if self.standalone_panels.wl_group_name_input.is_open() {
            if let Some(ref gni) = self.last_wl_group_name_result {
                if gni.input_rect.contains(x, y) {
                    self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                    return false;
                }
            }
        }

        // Watchlist modal — text select drag on the search input field (Overview tab only, TextFieldStore).
        if self.standalone_panels.watchlist_modal.is_open() {
            use mlc_app_layout::ui::modal_settings::WatchlistModalTab;
            if self.standalone_panels.watchlist_modal.active_tab == WatchlistModalTab::Overview {
                if let Some(ref wl) = self.last_watchlist_modal_result {
                    if wl.search_input_rect.contains(x, y) {
                        self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                        return false;
                    }
                }
            }
        }

        // Watchlist modal item row drag — begin drag-to-reorder.
        if self.standalone_panels.watchlist_modal.is_open() {
            if let Some(ref wl) = self.last_watchlist_modal_result {
                // Watchlist modal item row drag — begin drag-to-reorder.
                // Guard: if the pointer is over a delete button, don't start drag — let it be a click.
                // Check hovered widget AND check delete_btn_rects directly.
                let over_delete_hovered = live_hover.as_deref()
                    .map(|h| h.starts_with("wl_modal:delete:"))
                    .unwrap_or(false);
                let over_delete_rect = wl.delete_btn_rects.iter()
                    .any(|(_sym, r)| r.contains(x, y));
                let over_delete = over_delete_hovered || over_delete_rect;
                // Guard: if the group name input modal is open on top of the watchlist
                // modal, don't start row drag — the click belongs to that modal.
                if !self.standalone_panels.wl_group_name_input.is_open() && !over_delete && wl.list_viewport_rect.contains(x, y) {
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
                        self.standalone_panels.watchlist_modal.drag_reorder_pending = Some((idx, x, y));
                        eprintln!("[WatchlistModal] drag-reorder pending: row {}", idx);
                        // Do NOT return early — let the drag-start flow continue
                        // so other drag handlers (e.g. modal header) get checked.
                    }
                }
            }
        }

        // Search overlay modal (symbol / indicator / compare search) — drag-to-select (TextFieldStore).
        if self.modal_state.current.is_search_overlay() {
            if let Some(ref smr) = self.search_modal_result {
                if smr.input_rect.contains(x, y) {
                    self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
                    return false;
                }
            }
        }
        // === Floating inline bar drag start ===
        // Only drag by the name label — other items are clickable buttons.
        if !self.panel_app.toolbar_state.floating_inline_bar.dragging {
            if let Some(bar_rect) = self.last_inline_bar_rect {
                // Check if we're on the name label specifically
                let on_name_label = live_hover.as_deref() == Some("ilb:inline:name");
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
        if self.tc_modal.is_open {
            if let Some(ref tm) = self.tc_modal_result {
                if let Some(ref track_rect) = tm.scrollbar_track_rect {
                    let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                        && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                    if hit {
                        let (ch, vh) = (tm.setup_content_height, tm.setup_viewport_height);
                        let (ty, th) = (track_rect.y, track_rect.height);
                        self.tc_modal.scroll.handle_track_click(y, ty, th, ch, vh);
                        return false;
                    }
                }
            }
        }
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
                    use mlc_app_layout::ui::modal_settings::UserSettingsTab;

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
                use mlc_app_layout::ui::modal_settings::UserSettingsTab;
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
                                            self.core.indicators().timeframe_visibility_of(ind_id)
                                                .and_then(|tf_cfg| {
                                                    let tf_config = tf_cfg
                                                        .unwrap_or_else(mlc_core::drawing::TimeframeVisibilityConfig::all);
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

        // Primitive settings scrollbar track click — click on empty track
        // (not the handle) jumps the tab content by one page. Handle drag
        // itself goes through the ":scrollbar_handle" DRAG branch above.
        if self.panel_app.primitive_settings_state.is_open()
            && !self.panel_app.primitive_settings_state.is_color_picker_open()
        {
            if let Some(ref result) = &self.frame_result {
                if let Some(ref ps) = result.primitive_settings {
                    if let Some(ref track_rect) = ps.scrollbar_track_rect {
                        let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                            && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                        if hit {
                            self.panel_app.primitive_settings_state.scroll.handle_track_click(
                                y,
                                track_rect.y,
                                track_rect.height,
                                ps.total_content_height,
                                ps.viewport_height,
                            );
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
        if self.standalone_panels.watchlist_modal.is_open() {
            if let Some(ref wmr) = self.last_watchlist_modal_result {
                if let Some(ref track_rect) = wmr.scrollbar_track_rect {
                    let hit = x >= track_rect.x && x <= track_rect.x + track_rect.width
                        && y >= track_rect.y && y <= track_rect.y + track_rect.height;
                    if hit {
                        self.standalone_panels.watchlist_modal.scroll.handle_track_click(
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
            use crate::panel_grid::ChartInputTarget;
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
                    // Invalidate so the drag path decodes bar/price with the
                    // newly active leaf's rect (mirrors right-click at ~522).
                    self.active_frame_layout = None;
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

        // Panning and zooming are NOT undoable (owner ruling 2026-08-04) —
        // navigation is not an edit. A pan used to push a command here, which
        // meant Ctrl+Z undid the pan instead of the drawing the user had just
        // made, and any pan between an undo and a redo cleared the redo stack.

        let _fallback_ds;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_ds = self.build_extended_layout(); &_fallback_ds }
        };
        let overlay_results_ds = self.panel_app.panel_grid.active_container()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
            .with_overlays(&overlay_results_ds);

        // Delegate freehand-start and primitive/control-point hit-test to ChartPanelGrid.
        // Freehand: drawing_manager.start_freehand() runs inside handle_drag_start.
        // Background: viewport_before_drag already captured above.
        use crate::panel_grid::ChartDragStartHit;

        // TEMP-DIAG: fires once per drag gesture; proves whether find_separator_at_y
        // misses because sub_panes is empty or because y coordinates are offset.
        {
            let mc = &extended.main_chart.chart;
            let sep_info: Vec<String> = extended.sub_panes.iter()
                .map(|sp| format!("sep(y={:.1},h={:.1})", sp.separator.y, sp.separator.height))
                .collect();
            // Also dump the layout inputs: which window is active, what the
            // inner docking tree holds, and what the indicator manager returns
            // for that window (id/window_id/pane/visible) — pinpoints which
            // side of the visible_set intersection drops the panes.
            let active_cid = self.panel_app.panel_grid.active_container_id();
            let inner_ids: Vec<u64> = self.panel_app.panel_grid.active_container()
                .map(|w| crate::chart_container::collect_inner_instance_ids(&w.inner_docking))
                .unwrap_or_default();
            let inst_dump: Vec<String> = active_cid
                .and_then(|cid| self.panel_app.panel_grid.containers().get(&cid).map(|w| w.current_tag_id))
                .map(|bid| {
                    self.core.indicators()
                        .instances_for_bubble(bid)
                        .into_iter()
                        .map(|i| format!("inst(id={},bub={:?},pane={},vis={})", i.id, i.bubble_id, i.pane, i.visible))
                        .collect()
                })
                .unwrap_or_default();
            eprintln!(
                "[TEMP-DIAG drag-start] x={:.1} y={:.1} main_chart(x={:.1},y={:.1},w={:.1},h={:.1}) sub_panes={} {} | active_cid={:?} inner_ids={:?} {}",
                x, y, mc.x, mc.y, mc.width, mc.height,
                extended.sub_panes.len(),
                sep_info.join(" "),
                active_cid, inner_ids, inst_dump.join(" ")
            );
        }

        let active_bid_ds = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        // Same rule as the click path: bars come from the bubble already
        // resolved above, never from the lagging tag-registry lookup.
        let ds_bars = active_bid_ds
            .map(|bid| self.bars_for_bubble(bid))
            .unwrap_or_default();
        let drag_start_hit = if let Some(bid) = active_bid_ds {
            let mut dreg = self.core.drawings_mut();
            let mut session = crate::input::drawing_session::DrawingSessionView::new(&mut dreg, bid);
            self.panel_app.panel_grid.handle_drag_start(x, y, extended, &mut session, &ds_bars)
        } else {
            ChartDragStartHit::Miss
        };

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
                eprintln!("[ChartApp] sub-pane sep drag START instance_id={}", instance_id);
                // Pre-cutover this called sync_sub_panes_from_manager() — that
                // function walks every container, takes the indicators read
                // guard once, then for each pane id takes a write guard via
                // calculate_pane_range. The total RwLock work + per-frame
                // overhead was enough on a 4-indicator chart to delay
                // on_drag_move enough that winit CursorMoved events arrived
                // AFTER the user already released the button — so MOVE/END
                // never fired and the separator visually never moved. The
                // sync invariant is maintained by the indicator add/remove
                // path; nothing has changed sub-pane ids since the last add.
                // Removing the per-drag call restores the per-frame drag.
                // Mirror slot_sep_drag pattern: dedicated Option field on
                // ChartApp consumed at the TOP of on_drag_move, before any
                // other drag-handling branches that could swallow the event.
                // Snapshot the leaf content area; uzor's drag_separator
                // handles everything else (bounds, neighbor split, min-size).
                let (content_w, content_h) = self.panel_app.panel_grid.docking().active_leaf()
                    .and_then(|l| self.get_leaf_absolute_rect(l))
                    .map(|r| (r.width, r.height))
                    .unwrap_or((1.0, 1.0));
                self.sub_pane_sep_drag = Some((
                    instance_id, y, content_w, content_h, 0.0, None,
                ));
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
        // A resize drag owns the pointer until it is released — same rule as a
        // header drag, one branch up.
        {
            use mlc_app_layout::ui::modal_settings::ResizableModal;
            if self.panel_app.chart_settings_state.resize_drag().is_some() {
                self.panel_app.chart_settings_state.update_resize((x, y));
                return;
            }
            if self.panel_app.indicator_settings_state.resize_drag().is_some() {
                self.panel_app.indicator_settings_state.update_resize((x, y));
                return;
            }
            if self.panel_app.primitive_settings_state.resize_drag().is_some() {
                self.panel_app.primitive_settings_state.update_resize((x, y));
                return;
            }
        }

        let _ = (x, dx);

        // ── Granularity row drag-reorder: promote pending drag once mouse
        // moves >= 4px (owner spec 2026-07-16 items 2+3 — mirrors
        // `WatchlistModalState::drag_reorder_pending`'s 5px promote
        // threshold, tightened slightly per the owner's explicit "~4px"
        // spec). MUST run before any early-return branch below it so a
        // pending granularity drag is never silently dropped by an
        // unrelated drag handler claiming the move first. ─────────────────
        if let Some(pending) = self.granularity_drag_pending.clone() {
            let ddx = x - pending.start_x;
            let ddy = y - pending.start_y;
            if ddx * ddx + ddy * ddy >= 16.0 {
                self.granularity_drag_pending = None;
                self.granularity_drag = Some(crate::GranularityDrag {
                    family: pending.family,
                    section: pending.section,
                    from_index: pending.from_index,
                    drop_index: Some(pending.from_index),
                });
            }
        }
        if let Some(drag) = self.granularity_drag.clone() {
            // Resolve the drop index from the row currently hovered under the
            // cursor — reuse the SAME live hit-test the mousedown handler
            // used, scoped to rows within this drag's own family+section so
            // hovering a row from a different list never cross-contaminates
            // the index.
            let hovered = self.input_coordinator.borrow().hit_test_now(x, y).map(|h| h.as_str().to_string());
            if let Some(row_id) = hovered.as_deref().and_then(|h| h.strip_prefix("dropdown:timeframe_selector:")) {
                if let Some((family, section, index)) = self.resolve_granularity_reorder_row(row_id) {
                    if family == drag.family && section == drag.section {
                        self.granularity_drag = Some(crate::GranularityDrag { drop_index: Some(index), ..drag });
                    }
                }
            }
            return;
        }

        // ── Sub-pane separator resize drag (mirrors slot_sep_drag) ───────────
        // MUST run BEFORE any other drag-handling branch so the event isn't
        // swallowed by panel-content drag, freehand-extend, or other
        // intermediate handlers.
        //
        // Use incremental dy per frame (winit fires drag move on every mouse
        // event, but `dy` here is already the per-event delta, not cumulative
        // from start).  Apply it to the height_ratio directly each frame —
        // visually 1:1 with cursor.
        if let Some((instance_id, _start_y, _content_w, _content_h, _, _)) = self.sub_pane_sep_drag {
            let _ = y;
            // 2026-06-20: this used to read content_h from `self.frame_layouts`
            // (the per-frame layout cache). During drag the cache turned out to
            // be empty on every move (`frame_layouts.keys=[]`) — winit pauses
            // redraw / prepare_frame under some drag conditions, so the cache
            // was never refreshed and content_h came out 0 → uzor refused the
            // drag (`ok=false`). Build the layout directly from the leaf rect:
            // it's the same call the cache writer makes, just inline, and
            // doesn't depend on prepare_frame having run this frame.
            let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
            let (content_h, content_w): (f64, f64) = active_leaf
                .and_then(|leaf| {
                    let leaf_rect = self.get_leaf_absolute_rect(leaf)?;
                    let ext = self.build_extended_layout_for_leaf(leaf, &leaf_rect)?;
                    let h = ext.main_chart.chart.height
                        + ext.sub_panes.iter().map(|p| p.content.height + p.separator.height).sum::<f64>();
                    let w = ext.main_chart.chart.width;
                    Some((h, w))
                })
                .unwrap_or((0.0, 0.0));
            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                // Above-main panes show their separator at the pane BOTTOM and
                // pair visually with the NEXT above pane (or the main chart),
                // not with their tree-adjacent sibling — route to the dedicated
                // share-transfer drag instead of the uzor tree separator.
                let ok = if window.sub_pane_above_main.contains(&instance_id) {
                    window.drag_above_pane_separator(instance_id, dy, content_h)
                } else {
                    window.drag_sub_pane_separator(instance_id, dy, content_h, content_w)
                };
                eprintln!(
                    "[ChartApp] sub-pane sep DRAG id={} dy={:.1} content_h={:.0} ok={}",
                    instance_id, dy, content_h, ok
                );
            }
            return;
        }
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

        // ── Trading center chart sub-pane separator drag ────────────────────
        if let Some(instance_id) = self.trading_center_sep_drag {
            if let Some((pid, rect)) = self.trading_center_rect {
                if let Some(&cid) = self.trading_center_charts.get(&pid) {
                    self.trading_center_sep_drag_move(cid, &rect, instance_id, dy);
                }
            }
            return;
        }

        // ── Trading center chart drag (full engine mechanics) ───────────────
        if self.trading_center_drag_active {
            if let Some((pid, rect)) = self.trading_center_rect {
                if let Some(&cid) = self.trading_center_charts.get(&pid) {
                    let mode = self.trading_center_input.state.drag_mode;
                    self.trading_center_process(
                        cid,
                        &rect,
                        mlc_core::engine::input::ChartInputAction::DragMove {
                            mode,
                            x,
                            y,
                            delta_x: dx,
                            delta_y: dy,
                        },
                    );
                    // Crosshair follows the cursor during a chart-area pan,
                    // locked to the main-chart coordinate space (same as the
                    // main path's drag-locked update). Scale drags keep the
                    // crosshair where it was.
                    if matches!(mode, mlc_core::engine::input::DragMode::Chart) {
                        self.trading_center_crosshair(cid, &rect, x, y, true);
                    }
                }
            }
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
                self.right_toolbar_left_x - mlc_app_sidebar_content::state::MIN_SIDEBAR_WIDTH;
            if chart_if_min_sidebar < min_chart_w {
                // Not enough room — close the sidebar entirely.
                self.sidebar_state
                    .set_right_panel(mlc_app_sidebar_content::state::RightSidebarPanel::None);
                // Persist the auto-close so it survives restart.
                self.sidebar_data_dirty = true;
                self.persist_profile();
                return;
            }
            // Squeeze-to-collapse (owner order 2026-07-31): the drag can
            // compress down to MIN_SIDEBAR_WIDTH (L3 mosaic width); pushing
            // MORE than 20px past the minimum collapses the sidebar.
            let raw_width = self.right_toolbar_left_x - x;
            if raw_width < mlc_app_sidebar_content::state::MIN_SIDEBAR_WIDTH - 20.0 {
                self.sidebar_separator_drag_active = false;
                self.sidebar_state
                    .set_right_panel(mlc_app_sidebar_content::state::RightSidebarPanel::None);
                self.sidebar_data_dirty = true;
                self.persist_profile();
                return;
            }
            // Enforce [MIN_SIDEBAR_WIDTH, right_toolbar_left_x - min_chart_w] clamp.
            let max_w = self.right_toolbar_left_x - min_chart_w;
            let new_width = raw_width
                .clamp(mlc_app_sidebar_content::state::MIN_SIDEBAR_WIDTH, max_w);
            self.sidebar_state.set_right_width(new_width);
            // NOTE: width is persisted on drag-end (on_drag_end), not on every
            // mousemove — that would be hundreds of disk writes per second.
            return;
        }

        // Watchlist column-separator drag: update absolute separator offset (clip curtain model).
        if let Some((sep_idx, start_x, sep_offset_at_start)) = self.sidebar_state.watchlist_sep_drag {
            let active_list_id = self.watchlist_ui.active_list_id;
            // Skip drag when columns are aligned (equal-width mode).
            let align = self.watchlist_ui.column_config(active_list_id).align_columns;
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
                let c = self.watchlist_ui.column_config(active_list_id);
                let mut n = 0usize;
                if c.show_exchange     { n += 1; }
                if c.show_account_type { n += 1; }
                if c.show_last_price   { n += 1; }
                if c.show_change_pct   { n += 1; }
                if c.show_spark        { n += 1; }
                if c.show_change_abs   { n += 1; }
                if c.show_high_low     { n += 2; }
                if c.show_volume       { n += 1; }
                n
            };

            if sep_idx < n_seps {
                let delta = x - start_x;
                let new_offset = sep_offset_at_start + delta;
                const MIN_GAP: f64 = 16.0;

                // Clamp against left neighbor (or area edge).
                let prev_limit = if sep_idx == 0 { MIN_GAP } else {
                    self.watchlist_ui.column_config(active_list_id).separator_offsets
                        .and_then(|o| o.get(sep_idx - 1).copied())
                        .unwrap_or(MIN_GAP) + MIN_GAP
                };
                let clamped = new_offset.max(prev_limit).min(usable_w - MIN_GAP);

                // Initialise separator_offsets from defaults when not yet set or wrong length.
                let column_config = &mut self.watchlist_ui.list_mut(active_list_id).column_config;
                let needs_init = column_config.separator_offsets
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
                    column_config.separator_offsets = Some(offsets);
                }

                if let Some(offsets) = column_config.separator_offsets.as_mut() {
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
                // Do NOT push an action or set watchlists_dirty during drag:
                // that would cause main.rs to clone app_state back over sidebar_state
                // next frame (1-frame stutter). Persist happens in on_drag_end.
            }
            return;
        }

        // Watchlist modal drag-to-reorder: promote pending drag once mouse moves >= 5px.
        if let Some((pending_idx, start_x, start_y)) = self.standalone_panels.watchlist_modal.drag_reorder_pending {
            let dx = x - start_x;
            let dy = y - start_y;
            if dx * dx + dy * dy >= 25.0 {
                // Promote: clear pending, begin actual drag.
                self.standalone_panels.watchlist_modal.drag_reorder_pending = None;
                self.standalone_panels.watchlist_modal.drag_reorder = Some((pending_idx, y));
                self.standalone_panels.watchlist_modal.drop_index = Some(pending_idx);
                self.ui_drag_active = true;
                eprintln!("[WatchlistModal] drag-reorder promoted: row {}", pending_idx);
            }
        }

        // Watchlist modal drag-to-reorder: update drop index from mouse Y.
        if self.standalone_panels.watchlist_modal.drag_reorder.is_some() {
            // Update Y coordinate in drag state.
            if let Some((drag_idx, _)) = self.standalone_panels.watchlist_modal.drag_reorder {
                self.standalone_panels.watchlist_modal.drag_reorder = Some((drag_idx, y));
            }
            // Compute drop index from Y position using the modal list geometry.
            if let Some(ref wl) = self.last_watchlist_modal_result {
                // Modal list layout constants (must match render_overview_tab).
                let item_h = 28.0_f64;
                let list_top = wl.list_viewport_rect.y;
                let scroll = self.standalone_panels.watchlist_modal.scroll.offset;
                let relative_y = y - list_top + scroll;
                let row_count = wl.item_rects.len();
                let drop = if relative_y < 0.0 {
                    0
                } else {
                    ((relative_y / item_h).round() as usize).min(row_count)
                };
                self.standalone_panels.watchlist_modal.drop_index = Some(drop);
            }
            return;
        }

        // Primitive-settings Template menu drag-to-reorder: promote pending
        // drag once mouse moves >= 5px (item 4, 2026-08-08 review) — same
        // threshold and shape as the watchlist modal's own promotion above.
        if let Some((ref pending_key, start_x, start_y)) = self.panel_app.primitive_settings_state.template_drag_reorder_pending.clone() {
            let dx = x - start_x;
            let dy = y - start_y;
            if dx * dx + dy * dy >= 25.0 {
                self.panel_app.primitive_settings_state.template_drag_reorder_pending = None;
                self.panel_app.primitive_settings_state.template_drag_reorder = Some((pending_key.clone(), y));
                self.ui_drag_active = true;
                eprintln!("[ChartApp] prim_settings template menu drag-reorder promoted: {}", pending_key);
            }
        }

        // Primitive-settings Template menu drag-to-reorder: update drop
        // index from mouse Y. Unlike the watchlist blocks above (fixed row
        // height, one geometry to trust), this menu's own row count varies
        // with tab/save-as visibility, so the drop index is read straight
        // off each row's own rendered rect (`template_menu_rows`) rather
        // than replicated offset arithmetic — the nearest row boundary to
        // the pointer, by comparing `y` against each row's own vertical
        // midpoint.
        if self.panel_app.primitive_settings_state.template_drag_reorder.is_some() {
            if let Some((ref k, _)) = self.panel_app.primitive_settings_state.template_drag_reorder.clone() {
                self.panel_app.primitive_settings_state.template_drag_reorder = Some((k.clone(), y));
            }
            if let Some(ref ps) = self.frame_result.as_ref().and_then(|r| r.primitive_settings.as_ref()) {
                let mut drop = ps.template_menu_rows.len();
                for (i, (_, rect)) in ps.template_menu_rows.iter().enumerate() {
                    if y < rect.y + rect.height / 2.0 {
                        drop = i;
                        break;
                    }
                }
                self.panel_app.primitive_settings_state.template_drop_index = Some(drop);
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
            use mlc_core::SeparatorOrientation;
            let delta = match sep_drag.orientation {
                SeparatorOrientation::Horizontal => (y - sep_drag.start_y) as f32,
                SeparatorOrientation::Vertical => (x - sep_drag.start_x) as f32,
            };
            // Hard-coded per-leaf minimum width: no leaf can ever shrink below
            // LEAF_MIN_WIDTH regardless of which separator is being dragged.
            {
                let leaf_ids: Vec<_> = self.panel_app.panel_grid
                    .iter_containers()
                    .map(|(leaf_id, _w)| leaf_id)
                    .collect();
                for leaf_id in leaf_ids {
                    self.panel_app.panel_grid.set_leaf_min_width(
                        leaf_id,
                        crate::panel_grid::ChartPanelGrid::LEAF_MIN_WIDTH,
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
        if self.tc_modal.is_dragging {
            self.tc_modal.update_drag(x, y);
        }
        if self.dom_modal.is_dragging {
            self.dom_modal.update_drag(x, y);
            return;
        }
        if self.rc_modal.is_dragging {
            self.rc_modal.update_drag(x, y);
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
        #[cfg(feature = "cloud")]
        {
            if self.panel_app.cloud_login_state.is_dragging {
                self.panel_app.cloud_login_state.update_drag(x, y);
                return;
            }
            if self.panel_app.cloud_overlay_state.is_dragging {
                self.panel_app.cloud_overlay_state.update_drag(x, y);
                return;
            }
        }
        if self.panel_app.ewa_engine_settings_state.is_dragging {
            self.panel_app.ewa_engine_settings_state.update_drag(x, y);
            return;
        }
        if self.panel_app.markup_engine_settings_state.is_dragging {
            self.panel_app.markup_engine_settings_state.update_drag(x, y);
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
                let new_cursor = mlc_app_layout::ui::widgets::cursor_from_char_positions(&char_positions, x);
                let drag_wid: Option<WidgetId> = match field_id.as_str() {
                    "e2e_passphrase_input" | "wizard_passphrase_input"
                    | "profile_mgr:new_passphrase_input" => {
                        Some(WidgetId::from(crate::text_input::NEW_PASSPHRASE))
                    }
                    "wizard_profile_name_input" | "profile_mgr:name_input" => {
                        Some(WidgetId::from(crate::text_input::NEW_PROFILE_NAME))
                    }
                    "profile_mgr:recovery_key_input" | "profile_mgr:recovery_key_display" => {
                        Some(WidgetId::from(crate::text_input::RECOVERY_KEY))
                    }
                    "profile_mgr:confirm_passphrase_input"
                    | "wizard_confirm_passphrase_input"
                    | "profile_mgr:create_confirm_passphrase_input" => {
                        Some(WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE))
                    }
                    _ => None,
                };
                if let Some(wid) = drag_wid {
                    if let Some(fs) = self.input_coordinator.borrow_mut().text_fields_mut().field_state_mut(&wid) {
                        fs.cursor = new_cursor;
                        // selection_start stays as the anchor set during drag_start
                    }
                }
            }
            return;
        }
        if self.panel_app.preset_name_input.is_dragging {
            self.panel_app.preset_name_input.update_drag(x, y);
            return;
        }
        // Chart browser search text select drag move — TextFieldStore owns the drag.
        // (The store's drag_field tracks the active drag; no separate flag needed.)
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
        if self.standalone_panels.watchlist_modal.is_dragging {
            self.standalone_panels.watchlist_modal.update_drag(x, y);
            return;
        }
        // Watchlist / wl_group_name / preset_name / search overlay drag move
        // — forwarded to TextFieldStore which owns the drag state.
        {
            let has_drag = self.input_coordinator.borrow().text_fields().drag_active();
            if has_drag {
                self.input_coordinator.borrow_mut().text_fields_mut().on_drag_move(x);
                return;
            }
        }
        // text_select_dragging for all 4 settings modals is now handled by the
        // TextFieldStore drag_active() check above (Wave 2b migration).
        // The old per-modal blocks have been removed.
        if self.standalone_panels.wl_group_name_input.is_dragging {
            self.standalone_panels.wl_group_name_input.update_drag(x, y);
            return;
        }

        // === Search overlay modal drag move ===
        if self.modal_state.is_dragging {
            self.modal_state.update_drag(x, y);
            return;
        }

        // === Floating inline bar drag move ===
        if self.panel_app.toolbar_state.floating_inline_bar.dragging {
            let panel_layout = mlc_app_layout::panel_app::ChartPanelLayout::compute(
                &mlc_core::LayoutRect::new(0.0, 0.0, self.width as f64, self.height as f64),
                &self.panel_app.toolbar_config,
            );
            let bar_width = self.last_inline_bar_rect.map(|r| r.width).unwrap_or(400.0);
            let sidebar_w = self.sidebar_state.right_width();
            mlc_app_layout::panel_app::floating_inline_update_drag(
                &mut self.panel_app.toolbar_state.floating_inline_bar,
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
        if self.tc_modal.scroll.is_dragging {
            if let Some(ref tm) = self.tc_modal_result {
                if let Some(ref track_rect) = tm.scrollbar_track_rect {
                    self.tc_modal.scroll.handle_drag(
                        y,
                        track_rect.height,
                        tm.setup_content_height,
                        tm.setup_viewport_height,
                    );
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
        if self.panel_app.primitive_settings_state.scroll.is_dragging {
            if let Some(ref result) = self.frame_result {
                if let Some(ref ps) = result.primitive_settings {
                    if let Some(ref track_rect) = ps.scrollbar_track_rect {
                        self.panel_app.primitive_settings_state.scroll.handle_drag(
                            y,
                            track_rect.height,
                            ps.total_content_height,
                            ps.viewport_height,
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
            use mlc_app_layout::ui::modal_settings::UserSettingsTab;

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
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
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
        if self.standalone_panels.watchlist_modal.scroll.is_dragging {
            if let Some(ref wmr) = self.last_watchlist_modal_result {
                if let Some(ref track_rect) = wmr.scrollbar_track_rect {
                    self.standalone_panels.watchlist_modal.scroll.handle_drag(
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
            let active_bid_fh = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            let fh_bars: Vec<mlc_core::Bar> = self.active_chart_bars();
            let freehand_extended = if let Some(bid) = active_bid_fh {
                let mut dreg = self.core.drawings_mut();
                let mut session = crate::input::drawing_session::DrawingSessionView::new(&mut dreg, bid);
                self.panel_app.panel_grid.extend_freehand(x, y, extended, &mut session, &fh_bars)
            } else { false };
            if freehand_extended {
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
                                let is_over_separator = matches!(
                                    ExtendedLayoutHitTester::new(&ext).hit_test(x, y),
                                    mlc_core::engine::input::HitResult::PaneSeparator { .. }
                                );
                                if let Some((ts, price, vis, pane_idx)) = self.panel_app.panel_grid
                                    .update_crosshair_split(x, y, active_leaf, &ext, is_over_separator, &fh_bars)
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
                    let active_bid_ch1 = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    if let Some(bid) = active_bid_ch1 {
                        let is_over_separator = matches!(
                            ExtendedLayoutHitTester::new(extended).hit_test(x, y),
                            mlc_core::engine::input::HitResult::PaneSeparator { .. }
                        );
                        let mut dreg = self.core.drawings_mut();
                        let session = crate::input::drawing_session::DrawingSessionView::new(&mut dreg, bid);
                        self.panel_app.panel_grid.update_crosshair(
                            x, y, drag_mode, /* drawing_active */ true, extended, is_over_separator, &session, &fh_bars,
                        );
                    }
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

        // Use the frame-cached layout; fall back to building on demand.
        let _fallback_dm;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback_dm = self.build_extended_layout(); &_fallback_dm }
        };
        let overlay_results_dm = self.panel_app.panel_grid.active_container()
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
            use crate::panel_grid::ChartInputTarget;
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
                        let is_over_separator = matches!(
                            ExtendedLayoutHitTester::new(&ext).hit_test(x, y),
                            mlc_core::engine::input::HitResult::PaneSeparator { .. }
                        );
                        let hovered_bars: Vec<mlc_core::Bar> = self.bars_for_leaf(leaf_id);
                        if let Some((ts, price, vis, pane_idx)) = self.panel_app.panel_grid
                            .update_crosshair_split(x, y, leaf_id, &ext, is_over_separator, &hovered_bars)
                        {
                            self.propagate_crosshair_to_sync_group(leaf_id, ts, price, vis, pane_idx);
                        }
                    }
                }
            }
        } else if let Some((timestamp, price, crosshair_visible, pane_index)) = {
            let active_bid_ch2 = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            let is_over_separator2 = matches!(
                ExtendedLayoutHitTester::new(extended2).hit_test(x, y),
                mlc_core::engine::input::HitResult::PaneSeparator { .. }
            );
            let active_bars_ch2: Vec<mlc_core::Bar> = self.active_chart_bars();
            active_bid_ch2
                .and_then(|bid| {
                    let mut dreg = self.core.drawings_mut();
                    let session = crate::input::drawing_session::DrawingSessionView::new(&mut dreg, bid);
                    self.panel_app.panel_grid
                        .update_crosshair(x, y, drag_mode, false, extended2, is_over_separator2, &session, &active_bars_ch2)
                })
        } {
            let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
            if let Some(active_leaf) = active_leaf_opt {
                self.propagate_crosshair_to_sync_group(active_leaf, timestamp, price, crosshair_visible, pane_index);
            }
        }
    }

    /// Handle drag end at `(x, y)`.
    pub fn on_drag_end(&mut self, x: f64, y: f64) {
        // End any in-progress modal resize.
        {
            use mlc_app_layout::ui::modal_settings::ResizableModal;
            if self.panel_app.chart_settings_state.end_resize()
                || self.panel_app.indicator_settings_state.end_resize()
                || self.panel_app.primitive_settings_state.end_resize()
            {
                return;
            }
        }

        self.ui_drag_active = false;

        // ── End granularity row drag-reorder: clear any pending drag that
        // never crossed the promote threshold (falls through to the normal
        // click routing below, unchanged). ───────────────────────────────
        self.granularity_drag_pending = None;

        // ── End granularity row drag-reorder: apply the reorder then clear
        // drag state (owner spec 2026-07-16 items 2+3) — mirrors the
        // watchlist modal's drag-reorder-end block exactly. ───────────────
        if let Some(drag) = self.granularity_drag.take() {
            let to_index = drag.drop_index.unwrap_or(drag.from_index);
            if drag.from_index != to_index {
                self.process_chart_out_event(mlc_core::events::ChartOutEvent::ReorderGranularity {
                    family: drag.family,
                    section: drag.section,
                    from_index: drag.from_index,
                    to_index,
                });
            }
            return;
        }

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

        // ── End trading center chart sub-pane separator drag ────────────────
        if self.trading_center_sep_drag.take().is_some() {
            self.sidebar_data_dirty = true;
            return;
        }

        // ── End trading center chart drag ────────────────────────────────────
        if self.trading_center_drag_active {
            self.trading_center_drag_active = false;
            if let Some((pid, rect)) = self.trading_center_rect {
                if let Some(&cid) = self.trading_center_charts.get(&pid) {
                    let mode = self.trading_center_input.state.drag_mode;
                    self.trading_center_process(
                        cid,
                        &rect,
                        mlc_core::engine::input::ChartInputAction::DragEnd { mode, x, y },
                    );
                }
            }
            self.sidebar_data_dirty = true;
            return;
        }

        // ── End coordinator-routed panel drag (DOM, L2Tape, Footprint, BigTrades, LiquidityHeatmap, VolumeProfile) ─
        if let Some((item, local_id, _, _)) = self.active_drag_panel.take() {
            if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                panel.handle_drag_end(&local_id);
            }
            // Free-docking gestures restructure the container's tree —
            // persist the fresh snapshot right away (tree persist,
            // owner spec 2026-07-29).
            if local_id.starts_with("tcfree:grip:")
                || local_id.starts_with("tcfree:tab:")
                || local_id.starts_with("tcfree:sep:")
            {
                self.autosave_snapshot();
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

        // End sub-pane separator drag: clear the dedicated field + persist.
        if self.sub_pane_sep_drag.is_some() {
            self.sub_pane_sep_drag = None;
            self.persist_profile();
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
            if let Some(offsets) = &self.watchlist_ui.column_config(self.watchlist_ui.active_list_id).separator_offsets {
                self.watchlist_actions.push(crate::WatchlistAction::SetSeparatorOffsets { offsets: offsets.clone() });
            }
            self.watchlists_dirty = true;
            self.sidebar_state.watchlist_sep_drag = None;
            self.persist_watchlists();
            eprintln!("[ChartApp] Watchlist sep drag ended");
            return;
        }

        // End primitive-settings Template menu drag-to-reorder (item 4,
        // 2026-08-08 review): clear any pending drag that never promoted,
        // then — if one WAS promoted — commit it. The new order is read
        // straight off `template_menu_rows` (this frame's own rendered row
        // keys), not recomputed by hand, so the commit can never disagree
        // with what was actually on screen when the drag started.
        self.panel_app.primitive_settings_state.template_drag_reorder_pending = None;
        if let Some((from_key, _)) = self.panel_app.primitive_settings_state.template_drag_reorder.take() {
            let drop_idx = self.panel_app.primitive_settings_state.template_drop_index.take();
            let keys: Vec<String> = self.frame_result.as_ref()
                .and_then(|r| r.primitive_settings.as_ref())
                .map(|ps| ps.template_menu_rows.iter().map(|(k, _)| k.clone()).collect())
                .unwrap_or_default();
            if let Some(from_idx) = keys.iter().position(|k| k == &from_key) {
                let mut new_order = keys.clone();
                let removed = new_order.remove(from_idx);
                // `drop_idx` is a gap index (0..=keys.len()) in the
                // PRE-removal list; once past `from_idx` it must shift left
                // by one to land in the post-removal list — the same
                // adjustment `WatchlistManager::reorder_symbol`'s own caller
                // makes below.
                let mut to_idx = drop_idx.unwrap_or(from_idx).min(new_order.len() + 1);
                if to_idx > from_idx {
                    to_idx -= 1;
                }
                new_order.insert(to_idx.min(new_order.len()), removed);
                if new_order != keys {
                    eprintln!("[ChartApp] prim_settings template menu reordered: {} -> {}", from_key, to_idx);
                    self.template_actions.push(crate::TemplateAction::ReorderPrimitiveTemplates { order: new_order });
                }
            }
            return;
        }

        // End watchlist modal drag-to-reorder: clear any pending drag that never promoted.
        self.standalone_panels.watchlist_modal.drag_reorder_pending = None;

        // End watchlist modal drag-to-reorder: apply the reorder then clear drag state.
        if let Some((from_idx, _)) = self.standalone_panels.watchlist_modal.drag_reorder.take() {
            let to_idx = self.standalone_panels.watchlist_modal.drop_index.take().unwrap_or(from_idx);
            if from_idx != to_idx {
                let active_list_id = self.watchlist_ui.active_list_id;
                // Clamp to the ungrouped length to avoid panics.
                let list_len = self.core.watchlists()
                    .find(active_list_id)
                    .map(|l| l.ungrouped.len())
                    .unwrap_or(0);
                let clamped_to = to_idx.min(list_len.saturating_sub(1));
                if from_idx != clamped_to {
                    self.core.watchlists_mut().reorder_symbol(active_list_id, from_idx, clamped_to);
                    self.watchlist_actions.push(crate::WatchlistAction::Reorder { from_idx, to_idx: clamped_to });
                    self.watchlist_actions.push(crate::WatchlistAction::ClearOrderSnapshot);
                    self.watchlist_actions.push(crate::WatchlistAction::ResetSort);
                    self.sidebar_state.watchlist_sort_mode = 0;
                    self.watchlists_dirty = true;
                    self.persist_watchlists();
                    eprintln!("[WatchlistModal] reorder: {} -> {}", from_idx, clamped_to);
                    // Manual reorder invalidates any saved sort snapshot — the new
                    // order becomes the "original" and sort mode resets to unsorted.
                    self.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                }
            }
            return;
        }

        // End watchlist drag-to-reorder: apply the reorder then clear drag state.
        if let Some(drag_idx) = self.sidebar_state.watchlist_drag_index.take() {
            let drop_idx = self.sidebar_state.watchlist_drop_index.take().unwrap_or(drag_idx);
            self.sidebar_state.watchlist_drag_y = 0.0;
            let active_list_id = self.watchlist_ui.active_list_id;

            // Clamp drop_idx to the ungrouped length to avoid panics.
            let list_len = self.core.watchlists()
                .find(active_list_id)
                .map(|l| l.ungrouped.len())
                .unwrap_or(0);
            let clamped_drop = drop_idx.min(list_len.saturating_sub(1));

            if drag_idx != clamped_drop {
                self.core.watchlists_mut().reorder_symbol(active_list_id, drag_idx, clamped_drop);
                self.watchlist_actions.push(crate::WatchlistAction::Reorder { from_idx: drag_idx, to_idx: clamped_drop });
                self.watchlist_actions.push(crate::WatchlistAction::ClearOrderSnapshot);
                self.watchlist_actions.push(crate::WatchlistAction::ResetSort);
                self.sidebar_state.watchlist_sort_mode = 0;
                self.watchlists_dirty = true;
                self.persist_watchlists();
                eprintln!("[Sidebar] Watchlist reorder: {} -> {}", drag_idx, clamped_drop);
                // Manual reorder invalidates any saved sort snapshot — the new
                // order becomes the "original" and sort mode resets to unsorted.
                self.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
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
        if self.tc_modal.is_dragging {
            self.tc_modal.end_drag();
        }
        if self.dom_modal.is_dragging {
            self.dom_modal.end_drag();
            return;
        }
        if self.rc_modal.is_dragging {
            self.rc_modal.end_drag();
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
        #[cfg(feature = "cloud")]
        {
            if self.panel_app.cloud_login_state.is_dragging {
                self.panel_app.cloud_login_state.end_drag();
                return;
            }
            if self.panel_app.cloud_overlay_state.is_dragging {
                self.panel_app.cloud_overlay_state.end_drag();
                return;
            }
        }
        if self.panel_app.ewa_engine_settings_state.is_dragging {
            self.panel_app.ewa_engine_settings_state.end_drag();
            eprintln!("[ChartApp] ewa_engine_settings modal drag ended");
            return;
        }
        if self.panel_app.markup_engine_settings_state.is_dragging {
            self.panel_app.markup_engine_settings_state.end_drag();
            eprintln!("[ChartApp] markup_engine_settings modal drag ended");
            return;
        }
        // Profile manager text select drag end.
        if self.panel_app.user_settings_state.profile_mgr_text_select_dragging.is_some() {
            let field_id = self.panel_app.user_settings_state.profile_mgr_text_select_dragging.take().unwrap_or_default();
            // Finalize: if anchor == cursor (plain click jitter), clear the selection.
            let end_wid: Option<WidgetId> = match field_id.as_str() {
                "e2e_passphrase_input" | "wizard_passphrase_input"
                | "profile_mgr:new_passphrase_input" => {
                    Some(WidgetId::from(crate::text_input::NEW_PASSPHRASE))
                }
                "wizard_profile_name_input" | "profile_mgr:name_input" => {
                    Some(WidgetId::from(crate::text_input::NEW_PROFILE_NAME))
                }
                "profile_mgr:recovery_key_input" | "profile_mgr:recovery_key_display" => {
                    Some(WidgetId::from(crate::text_input::RECOVERY_KEY))
                }
                "profile_mgr:confirm_passphrase_input"
                | "wizard_confirm_passphrase_input"
                | "profile_mgr:create_confirm_passphrase_input" => {
                    Some(WidgetId::from(crate::text_input::CONFIRM_PASSPHRASE))
                }
                _ => None,
            };
            if let Some(wid) = end_wid {
                let mut ic = self.input_coordinator.borrow_mut();
                if let Some(fs) = ic.text_fields_mut().field_state_mut(&wid) {
                    if fs.selection_start == Some(fs.cursor) {
                        fs.selection_start = None;
                    }
                }
            }
            eprintln!("[ChartApp] profile_mgr text select drag ended: {}", field_id);
            return;
        }
        if self.panel_app.preset_name_input.is_dragging {
            self.panel_app.preset_name_input.end_drag();
            eprintln!("[ChartApp] preset_name_input modal drag ended");
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
        if self.standalone_panels.watchlist_modal.is_dragging {
            self.standalone_panels.watchlist_modal.end_drag();
            eprintln!("[ChartApp] watchlist_modal drag ended");
            return;
        }
        // text_select_dragging for all 4 settings modals is handled by
        // text_fields_mut().on_drag_end() which is called unconditionally at line ~3632.
        // The old per-modal blocks have been removed (Wave 2b migration).
        if self.standalone_panels.wl_group_name_input.is_dragging {
            self.standalone_panels.wl_group_name_input.end_drag();
            eprintln!("[ChartApp] wl_group_name_input modal drag ended");
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
        if self.tc_modal.scroll.is_dragging {
            self.tc_modal.scroll.end_drag();
            return;
        }
        if self.panel_app.indicator_settings_state.scroll.is_dragging {
            self.panel_app.indicator_settings_state.scroll.end_drag();
            eprintln!("[ChartApp] ind_settings scrollbar drag ended");
            return;
        }
        if self.panel_app.primitive_settings_state.scroll.is_dragging {
            self.panel_app.primitive_settings_state.scroll.end_drag();
            eprintln!("[ChartApp] prim_settings scrollbar drag ended");
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
            // ONE command for the whole gesture, against the state the drag
            // started from.
            self.record_primitive_json_after_drag();
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
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.compare_overlay.set_series_line_width_by_index(idx, width);
                    }
                    self.autosave_snapshot();
                    eprintln!("[ChartApp] cmp_settings line_width committed: {}", width);
                }
                // ONE command for the whole gesture, against the style the
                // drag started from — the live per-pixel writes above are not
                // recorded.
                self.record_compare_style_after_drag();
            } else {
                self.panel_app.compare_settings_state.end_slider_drag();
                self.compare_style_before_drag = None;
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
        if self.standalone_panels.watchlist_modal.scroll.is_dragging {
            self.standalone_panels.watchlist_modal.scroll.end_drag();
            return;
        }

        // Freehand drawing (brush/highlighter) — complete stroke on drag end
        let active_bid_cf = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        if let Some(freehand) = active_bid_cf.and_then(|bid| {
            let mut dreg = self.core.drawings_mut();
            let mut session = crate::input::drawing_session::DrawingSessionView::new(&mut dreg, bid);
            self.panel_app.panel_grid.complete_freehand(&mut session)
        })
        {
            // For grouped windows: move the completed freehand primitive to TagManager.
            // For standalone windows: record undo and propagate to color-tag peers.
            if !self.intercept_completed_primitive_to_group() {
                // Standalone path.
                let cmd = active_bid_cf.and_then(|bid| self.build_creation_command(bid, freehand.prim_id));
                if let Some(cmd) = cmd {
                    self.push_undo_command(cmd);
                }
                // Propagate completed freehand primitive to sync group peers.
                if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                    self.propagate_new_primitive_to_sync_group(active_leaf);
                }
            } else {
                // Grouped path — record undo for the freehand primitive placed into the group.
                let group_create_cmd = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.bubble_for_chart(cid))
                    .and_then(|gid| self.with_bubble(gid, |group| {
                        let prim = group.primitives.last()?;
                        Some(mlc_core::Command::CreatePrimitive {
                            prim_id: prim.data().id,
                            points: prim.points(),
                            snapshot: mlc_core::drawing::PrimitiveSnapshot::from_primitive(prim.as_ref()),
                            cascaded: Vec::new(),
                        })
                    }).flatten());
                if let Some(cmd) = group_create_cmd {
                    self.push_undo_command(cmd);
                    eprintln!("[ChartApp] Recorded CreatePrimitive (freehand, grouped) via group.primitives");
                }
            }
            // propagate_drawing_state_to_sync_group is a no-op stub under the
            // per-bubble model (Step 3.1) — peer windows sharing a tag already
            // share `current_tag_id`, so the finalized freehand primitive
            // (now in `DataBubble.primitives`) is immediately visible to all
            // of them without any propagation step. Kept as a call-site marker.
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
        let overlay_results_de = self.panel_app.panel_grid.active_container()
            .map(|w| w.sub_pane_overlay_results.clone())
            .unwrap_or_default();
        let hit_tester = ExtendedLayoutHitTester::new(extended)
            .with_overlays(&overlay_results_de);
        let actions = self.input_handler.process_action(
            ChartInputAction::DragEnd { mode: drag_mode, x, y },
            &hit_tester,
        );
        self.process_output_actions(actions);
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
        self.leaf_tab_hover = mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::None;
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
                            mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::GearMenu
                        } else if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                            mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::ColorTag
                        } else {
                            mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::Body
                        }
                    } else {
                        mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::None
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

        // --- Trading center chart hover crosshair (chart-in-container arc) ---
        if !self.trading_center_drag_active && self.trading_center_sep_drag.is_none() {
            if let Some((pid, rect)) = self.trading_center_rect {
                if let Some(&cid) = self.trading_center_charts.get(&pid) {
                    let inside = x >= rect.x
                        && x < rect.x + rect.width
                        && y >= rect.y
                        && y < rect.y + rect.height;
                    // Registered widgets layered over the chart (indicator
                    // legend chevron/rows, dropdowns) suppress the
                    // crosshair — same rule the main chart applies via its
                    // over-UI gate. The container body itself is a
                    // registered widget, so "over the chart" means the
                    // live hit is exactly the body.
                    let over_body = !inside
                        || self
                            .input_coordinator
                            .borrow()
                            .hit_test_now(x, y)
                            .map(|h| {
                                let s = h.as_str();
                                s.starts_with("slot:") && s.ends_with(":focus_content")
                            })
                            .unwrap_or(true);
                    // Sub-pane overlay button visibility/hover (aux
                    // counterpart of the split-mode overlay update). True
                    // while a button is under the cursor — crosshair hides
                    // then, like `any_button_hovered_split`.
                    let button_hovered =
                        self.trading_center_overlay_hover(cid, &rect, x, y, inside);
                    if inside && over_body && !button_hovered {
                        self.trading_center_crosshair(cid, &rect, x, y, false);
                        self.sidebar_data_dirty = true;
                    } else if let Some(window) =
                        self.panel_app.panel_grid.containers_mut().get_mut(&cid)
                    {
                        if window.crosshair.visible && !window.crosshair.synced {
                            window.hide_crosshair();
                            self.sidebar_data_dirty = true;
                        }
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
        self.trading_header_tb.hovered_top_toolbar_id = None;
        self.trading_header_tb.hovered_dropdown_item = None;
        self.panel_app.toolbar_state.hovered_inline_id = None;
        self.panel_app.toolbar_state.hovered_inline_dropdown_item = None;
        self.standalone_panels.watchlist_modal.hovered_widget = None;
        self.panel_app.clock_popup_state.hovered_item = None;
        // Reset agent/slot hover highlights each frame; re-set below if still hovering.
        self.sidebar_state.hovered_agent_leaf = None;
        self.sidebar_state.hovered_free_leaf = None;
        // Clear DOM hovered_price each frame; re-set below if a row widget is still hovered.
        for dom_state in self.panels_store.dom.values_mut() {
            dom_state.hovered_price = None;
        }

        // --- Update toolbar hover state from InputCoordinator ---
        // LIVE hit-test at the current cursor (x, y) against the zones registered
        // by the last render frame — NOT the stale hovered_widget() snapshot baked
        // by the previous end_frame. This resolves hover synchronously with the
        // pointer (zero frame lag). We parse the prefix to determine which toolbar
        // strip / dropdown / leaf-tab / etc. is hovered.
        if let Some(hovered) = self.input_coordinator.borrow().hit_test_now(x, y) {
            let id_str = hovered.as_str();
            if let Some(item_id) = id_str.strip_prefix("dtb:") {
                self.panel_app.toolbar_state.hovered_left_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("csb:") {
                self.panel_app.toolbar_state.hovered_top_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("tch:") {
                self.trading_header_tb.hovered_top_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("btb:") {
                self.panel_app.toolbar_state.hovered_bottom_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("rtb:") {
                self.panel_app.toolbar_state.hovered_right_toolbar_id = Some(item_id.to_string());
            } else if let Some(item_id) = id_str.strip_prefix("ilb:") {
                // Check if this is an inline dropdown item (style_option, width_option,
                // wave_option, wave_kind (E2 step 2) + wave_back (E2's own back row), or
                // wave_next_option)
                if item_id.starts_with("inline:style_option:") || item_id.starts_with("inline:width_option:") || item_id.starts_with("inline:wave_option:") || item_id.starts_with("inline:wave_kind:") || item_id == "inline:wave_back" || item_id.starts_with("inline:wave_next_option:") {
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
                // on WHICHEVER toolbar instance owns the open dropdown (main
                // topbar vs the trading container's embedded header).
                let item_id = id_str.splitn(3, ':').nth(2).unwrap_or_default().to_string();
                if self.trading_header_tb.open_dropdown_id.is_some() {
                    self.trading_header_tb.hovered_dropdown_item = Some(item_id);
                } else {
                    self.panel_app.toolbar_state.hovered_dropdown_item = Some(item_id);
                }
            } else if let Some(rest) = id_str.strip_prefix("context_menu:item:") {
                self.hovered_context_menu_item_id = Some(rest.to_string());
            } else if id_str == "context_menu:bg" {
                self.hovered_context_menu_item_id = None;
            } else if let Some(rest) = id_str.strip_prefix("leaf_tab:") {
                if let Some(colon) = rest.find(':') {
                    let leaf_id_str = &rest[..colon];
                    let sub_zone = &rest[colon + 1..];
                    if let Ok(lid) = leaf_id_str.parse::<u64>() {
                        self.leaf_tab_hovered_leaf = Some(mlc_core::LeafId(lid));
                        self.leaf_tab_hover = match sub_zone {
                            "gear" => mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::GearMenu,
                            "color_tag" => mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::ColorTag,
                            _ => mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::Body,
                        };
                    }
                }
            } else if id_str.starts_with("clock_popup:") {
                // Strip the "clock_popup:" prefix to match hovered_item keys used in renderer.
                if let Some(item) = id_str.strip_prefix("clock_popup:") {
                    self.panel_app.clock_popup_state.hovered_item = Some(item.to_string());
                }
            } else if id_str.starts_with("wl_modal:") || id_str.starts_with("wl_group_name:") {
                self.standalone_panels.watchlist_modal.hovered_widget = Some(id_str.to_string());
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
        self.panel_app.alert_settings_state.hovered_item_id = None;
        self.panel_app.chart_browser.hovered_preset_id = None;
        self.standalone_panels.watchlist_modal.hovered_item_id = None;
        self.panel_app.overlay_settings_state.hovered_item_id = None;

        {
            // LIVE hit-test (see note above) — modal hover fields resolved from the
            // current cursor position, not the stale end_frame snapshot.
            let hovered = self.input_coordinator.borrow().hit_test_now(x, y).map(|h| h.as_str().to_string());
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
                } else if id.starts_with("alert_set:") {
                    // Whole id, not the local part: the alert modal compares
                    // against `alert_set:item:condition` and friends. Nothing
                    // set this field at all until 2026-08-06, so every hover
                    // in that dialog was dead — the dropdown rows included.
                    self.panel_app.alert_settings_state.hovered_item_id = Some(id_str.clone());
                } else if let Some(local) = id.strip_prefix("chart_browser:item:") {
                    self.panel_app.chart_browser.hovered_preset_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("chart_browser:rename:") {
                    // Cursor over the rename icon counts as hovering the row —
                    // otherwise the icon disappears (it's only painted on
                    // hover/active) the moment you move toward it.  Owner repro
                    // 2026-06-26: rename/delete icons unclickable on the
                    // non-active row because the icon vanished as soon as the
                    // cursor crossed into its rect.
                    self.panel_app.chart_browser.hovered_preset_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("chart_browser:delete:") {
                    self.panel_app.chart_browser.hovered_preset_id = Some(local.to_string());
                } else if let Some(local) = id.strip_prefix("wl_modal:item:") {
                    self.standalone_panels.watchlist_modal.hovered_item_id = Some(local.to_string());
                } else if id.starts_with("tags_tabs:") {
                    self.panel_app.overlay_settings_state.hovered_item_id = Some(id_str.clone());
                }
            }
        }

        // Sync color grid popup — update swatch hover state
        if self.panel_app.sync_color_grid.is_open() {
            if let Some(ref result) = self.frame_result {
                if let Some(ref grid_draw) = result.sync_color_grid {
                    use mlc_app_layout::ui::sync_color_grid::hit_test_sync_color_grid;
                    let hit = hit_test_sync_color_grid(grid_draw, x, y);
                    match hit {
                        mlc_app_layout::ui::sync_color_grid::SyncColorGridHitResult::Color(idx) => {
                            self.panel_app.sync_color_grid.hovered_index = Some(idx);
                            self.panel_app.sync_color_grid.hovered_remove = false;
                            self.panel_app.sync_color_grid.hovered_add = false;
                        }
                        mlc_app_layout::ui::sync_color_grid::SyncColorGridHitResult::Remove => {
                            self.panel_app.sync_color_grid.hovered_index = None;
                            self.panel_app.sync_color_grid.hovered_remove = true;
                            self.panel_app.sync_color_grid.hovered_add = false;
                        }
                        mlc_app_layout::ui::sync_color_grid::SyncColorGridHitResult::AddCustom => {
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
        if self.standalone_panels.watchlist_modal.is_open() || self.standalone_panels.wl_group_name_input.is_open() {
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
        let split_hovered_leaf: Option<mlc_core::LeafId> =
            if self.panel_app.panel_grid.is_split() {
                use crate::panel_grid::ChartInputTarget;
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
            // Whole-screen UI suppressor — REMOVED 2026-07-25 (crosshair
            // z-order split follow-up). This used to hide the crosshair
            // globally whenever `is_over_ui_now(x, y)` reported ANY
            // registered popup/modal widget under the cursor. That check is
            // POSITIONAL against the widget's registered rect, not its
            // visually-painted extent — a tall popup (e.g. the chart-type
            // grid dropdown, 2 columns × many rows) registers a hit rect
            // spanning most of the chart height, so hovering anywhere inside
            // that box — including areas the popup never actually paints —
            // set `crosshair.visible = false` for the WHOLE chart, killing
            // every segment, not just the part genuinely covered by the
            // popup (owner repro 2026-07-25: dropdown open + cursor on a
            // free area far from the visible menu still went dark).
            //
            // The 2026-07-25 split already guarantees every popup/dropdown/
            // modal is painted AFTER `render_chart_crosshair_layer` in the
            // same frame (`render_to_scene` draws the crosshair mid-loop,
            // toolbar+dropdown after; `render_overlay`/`render_modal_layer`
            // run later still) — so natural z-order now occludes the
            // crosshair correctly wherever a popup actually paints over it,
            // with no need for a state-level kill switch. The narrower,
            // POSITIONAL suppressors below (`on_canvas`'s stale-hover-widget
            // check, and `over_subpane_widget`) still apply and are
            // sufficient — they only fire where a widget genuinely sits
            // under the cursor, not across an oversized registered rect.

            // Drawing bypass: keep crosshair visible while the hovered leaf's
            // drawing manager reports an in-progress primitive.  In split mode
            // the hovered leaf (not the active leaf) is checked so a drawing on
            // a non-active leaf does not incorrectly suppress the crosshair (F7).
            let is_drawing = if let Some(hovered) = split_hovered_leaf {
                // Split mode: read from hovered leaf's window.
                self.panel_app.panel_grid.container_for_leaf(hovered)
                    .map(|w| self.core.drawings().is_drawing(w.current_tag_id))
                    .unwrap_or(false)
            } else {
                // Single-window mode: active leaf == hovered leaf.
                self.panel_app.panel_grid.active_container()
                    .map(|w| self.core.drawings().is_drawing(w.current_tag_id))
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
            // CENTRAL z-order gate (owner spec 2026-07-28): one call decides
            // whether the cursor sits over ANY popup-or-above layer
            // (dropdown, submenu, indicator overlay, modal, context menu,
            // color picker) — live over the last completed frame's layers,
            // modal-barrier aware. Replaces the per-modal rect checks; every
            // future popup that registers on a ZLayer is covered for free.
            if self.cursor_over_popup(x, y) && !is_drawing {
                if self.panel_app.panel_grid.is_split() {
                    self.hide_all_split_crosshairs();
                } else {
                    self.hide_crosshair();
                }
                return;
            }

            let (on_canvas, over_subpane_widget) = {
                use mlc_core::input::ChartHitTester;
                use mlc_core::engine::input::HitResult;
                // Resolve the layout and overlay for the relevant leaf.
                let (extended, overlays) = if let Some(hovered) = split_hovered_leaf {
                    let leaf_rect_opt = self.get_leaf_absolute_rect(hovered);
                    let ext = self.frame_layouts.get(&hovered)
                        .cloned()
                        .or_else(|| leaf_rect_opt.and_then(|r| self.build_extended_layout_for_leaf(hovered, &r)));
                    let ovl = self.panel_app.panel_grid.container_for_leaf(hovered)
                        .map(|w| w.sub_pane_overlay_results.clone())
                        .unwrap_or_default();
                    (ext, ovl)
                } else {
                    let ext = self.active_frame_layout.clone()
                        .or_else(|| Some(self.build_extended_layout()));
                    let ovl = self.panel_app.panel_grid.active_container()
                        .map(|w| w.sub_pane_overlay_results.clone())
                        .unwrap_or_default();
                    (ext, ovl)
                };
                let hit = if let Some(ext) = extended {
                    let tester = ExtendedLayoutHitTester::new(&ext).with_overlays(&overlays);
                    tester.hit_test(x, y)
                } else {
                    HitResult::None
                };
                let geometry_says_canvas = matches!(
                    hit,
                    HitResult::Chart
                        | HitResult::SubPaneChart { .. }
                        | HitResult::PriceScale
                        | HitResult::SubPanePriceScale { .. }
                );
                // Sub-pane overlay buttons and pane separators are NOT canvas
                // (the crosshair must hide over them), but the hover pipeline
                // below must still run: it drives button hover highlight and
                // sub-pane overlay visibility. Early-returning for them left
                // sub-pane buttons and separators permanently hover-dead.
                let over_subpane_widget = matches!(
                    hit,
                    HitResult::SubPaneOverlayButton { .. } | HitResult::PaneSeparator { .. }
                );
                // A non-chart widget is explicitly hovered → trust that, suppress crosshair.
                // An empty/stale hover (None) → trust the geometry instead.
                let explicit_non_chart = self.input_coordinator.borrow().hovered_widget()
                    .map(|w| !w.as_str().starts_with("chart:pane:"))
                    .unwrap_or(false);
                (geometry_says_canvas && !explicit_non_chart, over_subpane_widget)
            };

            if !on_canvas && !is_drawing && !over_subpane_widget {
                if self.panel_app.panel_grid.is_split() {
                    self.hide_all_split_crosshairs();
                } else {
                    self.hide_crosshair();
                }
                return;
            }
            if over_subpane_widget && !is_drawing {
                // Keep the crosshair hidden over sub-pane widgets, but fall
                // through so the overlay hover state updates below still run
                // (downstream is_over_separator / hide_crosshair_for_overlay
                // keep it suppressed during the rest of this handler).
                if self.panel_app.panel_grid.is_split() {
                    self.hide_all_split_crosshairs();
                } else {
                    self.hide_crosshair();
                }
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
            self.panel_app.panel_grid.container_for_leaf(hovered)
                .map(|w| self.core.drawings().is_drawing(w.current_tag_id))
                .unwrap_or(false)
        } else {
            self.panel_app.panel_grid.active_container()
                .map(|w| self.core.drawings().is_drawing(w.current_tag_id))
                .unwrap_or(false)
        };
        if self.panel_app.panel_grid.is_split() && !is_drawing_skip {
            use crate::panel_grid::ChartInputTarget;
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
                        let is_over_separator = matches!(
                            ExtendedLayoutHitTester::new(&extended).hit_test(x, y),
                            mlc_core::engine::input::HitResult::PaneSeparator { .. }
                        );
                        let hovered_bars_mm: Vec<mlc_core::Bar> = self.bars_for_leaf(leaf_id);
                        if let Some((ts, price, vis, pane_idx)) = self.panel_app.panel_grid
                            .update_crosshair_split(x, y, leaf_id, &extended, is_over_separator, &hovered_bars_mm)
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
                        .container_for_leaf(leaf_id)
                        .map(|w| w.sub_pane_overlay_results.clone())
                        .unwrap_or_default();
                    let mut any_button_hovered_split = false;
                    if let (Some(extended), Some(window)) = (extended_opt, self.panel_app.panel_grid.container_for_leaf_mut(leaf_id)) {
                        for state in window.sub_pane_overlay_states.iter_mut() {
                            state.visible = false;
                            state.hovered_button = None;
                            state.hovered_left_button = None;
                        }
                        for (layout_idx, pane_layout) in extended.sub_panes.iter().enumerate() {
                            if pane_layout.content.contains(x, y) {
                                // overlay_states are indexed in tree/layout order (same as layout_idx).
                                let real_idx = layout_idx;
                                while window.sub_pane_overlay_states.len() <= real_idx {
                                    window.sub_pane_overlay_states.push(Default::default());
                                }
                                window.sub_pane_overlay_states[real_idx].visible = true;
                                if layout_idx < overlay_results_split.len() {
                                    use mlc_core::state::modal_data::SubPaneButton;
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
                        if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
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
        let active_bid_is_drawing = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        let is_drawing = active_bid_is_drawing
            .map(|bid| self.core.drawings().is_drawing(bid))
            .unwrap_or(false);

        // Hide crosshair during sub-pane separator drag — resize cursor is enough.
        if self.sub_pane_sep_drag.is_some() {
            self.hide_crosshair();
            return;
        }
        // Clone the cached layout so the borrow ends here, allowing the mutable
        // propagate_crosshair_to_sync_group and update_sub_pane_overlay_hover calls below.
        let extended = self.active_frame_layout.clone()
            .unwrap_or_else(|| self.build_extended_layout());
        let drag_mode = self.input_handler.state.drag_mode;
        let is_over_separator_mm = matches!(
            ExtendedLayoutHitTester::new(&extended).hit_test(x, y),
            mlc_core::engine::input::HitResult::PaneSeparator { .. }
        );
        if let Some((timestamp, price, crosshair_visible, pane_index)) = {
            let active_bid_ch3 = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            let active_bars_ch3: Vec<mlc_core::Bar> = self.active_chart_bars();
            active_bid_ch3
                .and_then(|bid| {
                    let mut dreg = self.core.drawings_mut();
                    let session = crate::input::drawing_session::DrawingSessionView::new(&mut dreg, bid);
                    self.panel_app.panel_grid
                        .update_crosshair(x, y, drag_mode, is_drawing, &extended, is_over_separator_mm, &session, &active_bars_ch3)
                })
        } {
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
            use crate::panel_grid::ChartInputTarget;
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
        let overlay_results_mm = self.panel_app.panel_grid.active_container()
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
            let leaf_ids: Vec<mlc_core::LeafId> = self.panel_app
                .panel_grid
                .panel_rects()
                .keys()
                .copied()
                .collect();
            leaf_ids.iter().any(|&lid| {
                self.panel_app.panel_grid.container_for_leaf(lid)
                    .map(|w| self.core.drawings().is_drawing(w.current_tag_id))
                    .unwrap_or(false)
            })
        } else {
            self.panel_app.panel_grid.active_container()
                .map(|w| self.core.drawings().is_drawing(w.current_tag_id))
                .unwrap_or(false)
        };
        if !is_drawing {
            // hide_crosshair is split-aware: clears all leaves in split mode and
            // propagates the hide to every sync-group's order-flow panels (F4).
            self.hide_crosshair();
        }

        // Trading center chart: cursor left the app window — hide its
        // crosshair too (leaf-keyed hide_crosshair above never reaches the
        // leafless aux chart).
        if let Some((pid, _)) = self.trading_center_rect {
            if let Some(&cid) = self.trading_center_charts.get(&pid) {
                if let Some(window) = self.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                    if window.crosshair.visible {
                        window.hide_crosshair();
                        self.sidebar_data_dirty = true;
                    }
                }
            }
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
            let leaf_ids: Vec<mlc_core::LeafId> = self.panel_app
                .panel_grid
                .panel_rects()
                .keys()
                .copied()
                .collect();
            // Clear visible on all split leaves (F5).
            for &lid in &leaf_ids {
                if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(lid) {
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
            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
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
        self.panel_app.panel_grid.active_container()
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

        // ── Active TradingContainer separator drag: show resize cursor for
        // entire drag duration (mirrors the agent/slot separator checks
        // above) — W1, 2026-07-25.
        if let Some((mlc_core::state::Panel::TradingContainer(_), ref local_id, _, _)) = self.active_drag_panel {
            use mlc_app_panels::trading::trading::trading_container::TradingSeparator;
            if let Some(sep) = TradingSeparator::from_wire_id(local_id) {
                return match sep {
                    TradingSeparator::DomRightCol => CursorStyle::EwResize,
                    TradingSeparator::OeRiskCalc | TradingSeparator::BottomStrip => CursorStyle::NsResize,
                };
            }
            // Free-mode tree separators — orientation is encoded in the id.
            if let Some(rest) = local_id.strip_prefix("tcfree:sep:") {
                return if rest.starts_with("v:") { CursorStyle::EwResize } else { CursorStyle::NsResize };
            }
        }

        // Sidebar separator: show EwResize cursor when hovering over or dragging the separator.
        // LIVE hit-test at (x, y) — not the stale end_frame hover snapshot.
        let on_sidebar_separator = self.sidebar_separator_drag_active
            || self.input_coordinator.borrow().hit_test_now(x, y)
                .map(|h| h.as_str() == "right_sidebar_separator")
                .unwrap_or(false);
        if on_sidebar_separator {
            return CursorStyle::EwResize;
        }

        // Watchlist column separators: show EwResize cursor when hovering or dragging.
        let on_watchlist_col_sep = self.sidebar_state.watchlist_sep_drag.is_some()
            || self.input_coordinator.borrow().hit_test_now(x, y)
                .map(|h| h.as_str().starts_with("watchlist_sep_"))
                .unwrap_or(false);
        if on_watchlist_col_sep {
            return CursorStyle::EwResize;
        }

        // When a watchlist modal or group name input is open, the entire screen
        // should show the default cursor — modals capture all interaction.
        if self.standalone_panels.watchlist_modal.is_open() || self.standalone_panels.wl_group_name_input.is_open() {
            return CursorStyle::Default;
        }

        // ── Agent panel separator hover ────────────────────────────────────────
        // Must be checked before the generic is_over_ui() → Default fallback,
        // because separator widgets ARE registered UI elements (so is_over_ui()
        // would fire first and swallow the resize cursor).
        {
            use uzor::panels::SeparatorOrientation;
            let hovered = self.input_coordinator.borrow().hit_test_now(x, y).map(|h| h.as_str().to_string());
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

                // slot:{idx}:leaf:{leaf_id}:sep:{name} — a TradingContainer's
                // v1 region separators (W1, 2026-07-25). Checked before the
                // slot:{slot_idx}:sep:{sep_idx} shape below since it wouldn't
                // match this one anyway (its `sep_str.parse::<usize>()` fails
                // on a leaf-qualified id) but keeps the two shapes visually
                // grouped.
                if let Some(rest) = wid.strip_prefix("slot:") {
                    if let Some((_slot_str, after_slot)) = rest.split_once(":leaf:") {
                        if let Some((_leaf_id_str, local_id)) = after_slot.split_once(':') {
                            use mlc_app_panels::trading::trading::trading_container::TradingSeparator;
                            if let Some(sep) = TradingSeparator::from_wire_id(local_id) {
                                return match sep {
                                    TradingSeparator::DomRightCol => CursorStyle::EwResize,
                                    TradingSeparator::OeRiskCalc | TradingSeparator::BottomStrip => CursorStyle::NsResize,
                                };
                            }
                            // Free-mode tree separators (`tcfree:sep:{v|h}:{idx}`).
                            if let Some(rest2) = local_id.strip_prefix("tcfree:sep:") {
                                return if rest2.starts_with("v:") { CursorStyle::EwResize } else { CursorStyle::NsResize };
                            }
                        }
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

        // Trading center chart: engine hit-zone cursor (resize arrows over
        // its scale strips, hidden cursor under magnet snap, drag-held
        // cursor mid-gesture). MUST run before the universal is_over_ui
        // check — the container body IS a registered widget, so the generic
        // check would swallow the chart cursors with Default. z-order gated
        // on the live hover target being the container body, so dropdowns/
        // modals above the chart keep the default cursor.
        if self.trading_center_drag_active {
            if let Some(c) = self.trading_center_cursor(x, y) {
                return c;
            }
        }
        {
            let over_tc_body = self.input_coordinator.borrow().hit_test_now(x, y)
                .map(|h| {
                    let s = h.as_str();
                    s.starts_with("slot:") && s.ends_with(":focus_content")
                })
                .unwrap_or(false);
            if over_tc_body {
                if let Some(c) = self.trading_center_cursor(x, y) {
                    return c;
                }
            }
        }

        // Universal UI check: any registered widget hovered = default cursor.
        // Since the chart canvas is NOT a registered widget, is_over_ui_now()
        // returns true only when the pointer is over a toolbar, modal, context
        // menu, dropdown, color picker, or any other registered UI element. LIVE
        // hit-test at (x, y) — not the stale end_frame snapshot.
        if self.input_coordinator.borrow().is_over_ui_now(x, y) {
            return CursorStyle::Default;
        }

        // Split panel: check for separator cursor first.
        if self.panel_app.panel_grid.is_split() {
            use crate::panel_grid::ChartInputTarget;
            use mlc_core::SeparatorOrientation;
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
                        let tester = mlc_app_layout::layout::ExtendedLayoutHitTester::new(&extended);
                        use mlc_core::input::ChartHitTester;
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
        let tester = mlc_app_layout::layout::ExtendedLayoutHitTester::new(extended);
        use mlc_core::input::ChartHitTester;
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
                // Live hit-test so opacity scroll works on the first wheel event
                // even when the frame-end snapshot hasn't been updated yet.
                let source = self.input_coordinator.borrow().hit_test_now(x, y).and_then(|wid| {
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
                        "tc_modal:setup_viewport" => {
                            if let Some(ref tm) = self.tc_modal_result {
                                self.tc_modal.scroll.handle_wheel(
                                    scroll_step,
                                    tm.setup_content_height,
                                    tm.setup_viewport_height,
                                );
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
                        "prim_settings:scroll_viewport" => {
                            if let Some(ref result) = self.frame_result {
                                if let Some(ref ps) = result.primitive_settings {
                                    self.panel_app.primitive_settings_state.scroll.handle_wheel(
                                        scroll_step,
                                        ps.total_content_height,
                                        ps.viewport_height,
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
                                self.standalone_panels.watchlist_modal.scroll.handle_wheel(
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
                                use mlc_app_layout::ui::modal_settings::UserSettingsTab;
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

                                let active_bid_sl = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                                if field_id == "stroke_width" {
                                    if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                                        if let Some(data) = active_bid_sl
                                            .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
                                        {
                                            let new_value = (data.width + delta).clamp(min_val, max_val);
                                            self.apply_slider_value(&field_id, new_value);
                                        }
                                    }
                                } else if let Some(prop_id_owned) = field_id.strip_prefix("style_prop:").map(|s| s.to_string()) {
                                    if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                                        if let Some(bid) = active_bid_sl {
                                            let new_value_opt = {
                                                let dreg = self.core.drawings();
                                                let prims = dreg.primitives(bid);
                                                prims.iter().find(|p| p.data().id == prim_id).and_then(|prim| {
                                                    let style_props = prim.style_properties();
                                                    style_props.iter().find(|p| p.id == prop_id_owned.as_str())
                                                        .and_then(|prop| prop.value.as_number())
                                                        .map(|current| (current + delta).clamp(min_val, max_val))
                                                })
                                            };
                                            if let Some(new_value) = new_value_opt {
                                                self.apply_slider_value(&field_id, new_value);
                                            }
                                        }
                                    }
                                } else if let Some(prop_id_owned) = field_id.strip_prefix("level_prop:").map(|s| s.to_string()) {
                                    if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                                        if let Some(bid) = active_bid_sl {
                                            let new_value_opt = {
                                                let dreg = self.core.drawings();
                                                let prims = dreg.primitives(bid);
                                                prims.iter().find(|p| p.data().id == prim_id).and_then(|prim| {
                                                    let level_props = prim.level_properties();
                                                    level_props.iter().find(|p| p.id == prop_id_owned.as_str())
                                                        .and_then(|prop| prop.value.as_number())
                                                        .map(|current| (current + delta).clamp(min_val, max_val))
                                                })
                                            };
                                            if let Some(new_value) = new_value_opt {
                                                self.apply_slider_value(&field_id, new_value);
                                            }
                                        }
                                    }
                                } else if field_id.starts_with("tf_") && field_id.ends_with("_slider") {
                                    if let Some(tf_idx) = field_id.strip_prefix("tf_")
                                        .and_then(|s| s.strip_suffix("_slider"))
                                        .and_then(|s| s.parse::<usize>().ok())
                                    {
                                        if let Some(prim_id) = self.panel_app.primitive_settings_state.primitive_id {
                                            if let Some(data) = active_bid_sl
                                                .and_then(|bid| self.core.drawings().get_data(bid, prim_id))
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
                                                if let Some(bid) = active_bid_sl {
                                                    self.core.drawings_mut().set_data(bid, prim_id, &new_data);
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
                    use mlc_app_layout::ui::modal_settings::CompareSettingsTab;
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

                                    use mlc_core::drawing::TimeframeVisibilityConfig;
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
                                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
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
                            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
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
                    if self.sidebar_state.right_panel == mlc_app_sidebar_content::state::RightSidebarPanel::Agents {
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

                    // Slot panels (typed slots): route wheel to the hovered leaf body.
                    {
                        use mlc_app_sidebar_content::state::RightSidebarPanel;
                        let slot_idx_opt = match self.sidebar_state.right_panel {
                            RightSidebarPanel::SlotTrading   => Some(0usize),
                            RightSidebarPanel::SlotDom       => Some(1),
                            RightSidebarPanel::SlotTape      => Some(2),
                            RightSidebarPanel::SlotL2        => Some(3),
                            RightSidebarPanel::SlotBigTrades => Some(4),
                            RightSidebarPanel::SlotLiq       => Some(5),
                            _ => None,
                        };

                        if let Some(slot_idx) = slot_idx_opt {
                            use mlc_core::state::Panel;
                            let scroll_step = dy;

                            // Trading center chart: wheel through the aux
                            // engine handler — hit-zone aware (chart area =
                            // X zoom around cursor, price scale strip =
                            // Y range zoom), exactly the main chart's
                            // wheel semantics (chart-in-container arc).
                            if let Some((_pid, cid, rect)) = self.trading_center_hit(x, y) {
                                self.trading_center_process(
                                    cid,
                                    &rect,
                                    mlc_core::engine::input::ChartInputAction::Scroll {
                                        x,
                                        y,
                                        delta_x: dx,
                                        delta_y: dy,
                                    },
                                );
                                return;
                            }

                            // First: check if coordinator scroll target is a DOM BlackboxPanel
                            // (standalone Dom slot) OR a TradingContainer-hosted dom/
                            // position_manager/trade_log widget — same nested widget-id
                            // shape one level under the container's own leaf, since
                            // `TradingContainerState::render` forwards its `slot_prefix`
                            // unchanged to every hosted child (W1, 2026-07-25).
                            // Sense-aware process_scroll(x, y) finds these even if
                            // non-scroll widgets (overlays/buttons) are stacked above.
                            let dom_leaf_opt = {
                                let hovered = self.input_coordinator.borrow_mut().process_scroll(x, y).map(|h| h.as_str().to_string());
                                hovered.and_then(|wid| {
                                    let rest = wid.strip_prefix("slot:")?;
                                    let (idx_str, after_slot) = rest.split_once(":leaf:")?;
                                    let panel_idx: usize = idx_str.parse().ok()?;
                                    if panel_idx != slot_idx { return None; }
                                    for suffix in [":dom:body", ":multi_dom:body", ":position_manager:body", ":trade_log:body", ":orders:history", ":orders:body"] {
                                        if let Some(leaf_id_str) = after_slot.strip_suffix(suffix) {
                                            let raw: u64 = leaf_id_str.parse().ok()?;
                                            return Some((uzor::panels::LeafId(raw), &suffix[1..]));
                                        }
                                    }
                                    None
                                })
                            };

                            if let Some((dom_leaf_id, local_id)) = dom_leaf_opt {
                                let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                    .inner()
                                    .tree()
                                    .leaf(dom_leaf_id)
                                    .and_then(|l| l.active_panel().cloned());
                                match item_opt {
                                    Some(Panel::Dom(pid)) => {
                                        if let Some(state) = self.panels_store.dom.get_mut(&pid) {
                                            if ctrl {
                                                // Ctrl+scroll: zoom aggregation — multi cycles
                                                // the GRID, single zooms tick_size.
                                                if state.multi_mode {
                                                    state.multi.cycle_grid_size();
                                                } else {
                                                    let new_tick = if scroll_step > 0.0 {
                                                        (state.tick_size * 10.0).clamp(0.0001, 100.0)
                                                    } else if scroll_step < 0.0 {
                                                        (state.tick_size / 10.0).clamp(0.0001, 100.0)
                                                    } else {
                                                        state.tick_size
                                                    };
                                                    state.set_tick_size(new_tick);
                                                }
                                            } else {
                                                use mlc_app_panels::BlackboxEvent;
                                                use mlc_app_panels::panel_trait::TradingPanel;
                                                state.handle_blackbox_event(0.0, 0.0, BlackboxEvent::Scroll { dy: scroll_step as f32 });
                                            }
                                            self.sidebar_data_dirty = true;
                                        }
                                    }
                                    Some(Panel::TradingContainer(pid)) => {
                                        if let Some(state) = self.panels_store.trading_container.get_mut(&pid) {
                                            if local_id == "dom:body" && ctrl {
                                                let new_tick = if scroll_step > 0.0 {
                                                    (state.dom.tick_size * 10.0).clamp(0.0001, 100.0)
                                                } else if scroll_step < 0.0 {
                                                    (state.dom.tick_size / 10.0).clamp(0.0001, 100.0)
                                                } else {
                                                    state.dom.tick_size
                                                };
                                                state.dom.set_tick_size(new_tick);
                                            } else {
                                                use mlc_app_panels::panel_trait::TradingPanel;
                                                state.handle_scroll(local_id, 0.0, scroll_step);
                                            }
                                            self.sidebar_data_dirty = true;
                                        }
                                    }
                                    _ => {}
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
                                // Look up the active Panel for this leaf.
                                let item_opt = self.sidebar_state.slot_dockings[slot_idx]
                                    .inner()
                                    .tree()
                                    .leaf(hover_leaf_id)
                                    .and_then(|l| l.active_panel().cloned());

                                match item_opt {
                                    Some(item @ Panel::BigTrades(_))
                                    | Some(item @ Panel::L2Tape(_))
                                    | Some(item @ Panel::LiqTape(_))
                                    | Some(item @ Panel::Footprint(_))
                                    | Some(item @ Panel::LiquidityHeatmap(_))
                                    | Some(item @ Panel::VolumeProfile(_))
                                    | Some(item @ Panel::PositionManager(_))
                                    | Some(item @ Panel::TradeLog(_)) => {
                                        let local_id = match &item {
                                            Panel::BigTrades(_) => "bigtrades:body",
                                            Panel::L2Tape(_) => "l2tape:body",
                                            Panel::LiqTape(_) => "liqtape:body",
                                            Panel::Footprint(_) => "footprint:body",
                                            Panel::LiquidityHeatmap(_) => "heatmap:body",
                                            Panel::VolumeProfile(_) => "volprofile:body",
                                            Panel::PositionManager(_) => "position_manager:body",
                                            Panel::TradeLog(_) => "trade_log:body",
                                            _ => unreachable!(),
                                        };
                                        if let Some(panel) = self.panels_store.get_panel_mut(&item) {
                                            panel.handle_scroll(local_id, 0.0, scroll_step);
                                            self.sidebar_data_dirty = true;
                                        }
                                    }
                                    Some(Panel::TradeTape(pid)) => {
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
            use crate::panel_grid::ChartInputTarget;
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
        let overlay_results_sc = self.panel_app.panel_grid.active_container()
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
        // Cancel an in-progress granularity row drag-reorder first (owner
        // spec 2026-07-16 items 2+3: "Escape ... cancels the drag") — no
        // reorder applies, the dropdown itself stays open.
        if self.granularity_drag_pending.take().is_some() || self.granularity_drag.take().is_some() {
            return;
        }

        // Symbol fast-search dropdown (symbol-search-compare arc): Escape
        // closes it and reverts the button to the active symbol. Escape is
        // routed HERE by the input shell (`chart.on_escape()`), never
        // through the text-field key dispatch — the `TextAction::Cancel`
        // arm in `text_action_symbol_quick_search` only fires for the
        // in-band `\x1b` char path, which the shell does not use (live
        // repro 2026-07-16: Escape left the inline editor open).
        if self.symbol_quick_edit.is_some() {
            self.close_symbol_quick_dropdown();
            return;
        }

        // Trading container embedded header toolbar — Escape closes its
        // open dropdown (TF picker etc.) without applying a selection.
        if self.trading_header_tb.open_dropdown_id.take().is_some() {
            return;
        }

        // If chart browser modal is open, close it.
        if self.panel_app.chart_browser.is_open {
            self.panel_app.chart_browser.close();
            self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
            eprintln!("[ChartApp] chart browser closed via Escape");
            return;
        }

        // If Tags & Tabs modal is open, close it.
        if self.panel_app.tags_tabs_state.is_open {
            self.panel_app.close_tags_tabs();
            eprintln!("[ChartApp] tags_tabs closed via Escape");
            return;
        }

        // If the Trading Container modal is open, close it.
        if self.tc_modal.is_open {
            self.tc_modal.close();
            eprintln!("[ChartApp] tc_modal closed via Escape");
            return;
        }

        // If a standalone panel-settings surface is open, close it.
        if self.panel_settings_open.is_some() {
            self.panel_settings_open = None;
            eprintln!("[ChartApp] panel settings closed via Escape");
            return;
        }

        // If watchlist group name input is open, close it.
        if self.standalone_panels.wl_group_name_input.is_open() {
            self.standalone_panels.wl_group_name_input.close();
            eprintln!("[ChartApp] wl_group_name_input cancelled");
            return;
        }

        // If preset name input modal is open, close it.
        if self.panel_app.preset_name_input.is_open {
            self.panel_app.preset_name_input.close();
            self.panel_app.active_modal = mlc_core::modal::ChartOpenModal::None;
            eprintln!("[ChartApp] preset name input cancelled");
            return;
        }

        // If text editing is active in any settings modal, cancel it first before closing the modal.
        // This follows the pattern: first Escape cancels editing, second Escape closes the modal.
        //
        // HARD INVARIANT (2026-08-06 wedge post-mortem): these four arms clear
        // `active_field_id` and blur DIRECTLY — they must NEVER call
        // `commit_*_editing_text()`. Escape is the modal's last-resort exit, so
        // it has to stay reachable even if a commit path is broken (as one was:
        // `commit_prim_editing_text_inner` used to trap the wasm RwLock on
        // every text_content/stroke_width/font_size commit — see the comment
        // there). Routing Escape through commit would make the escape hatch
        // depend on the exact code it exists to escape from.
        if self.panel_app.primitive_settings_state.active_field_id.is_some() {
            self.panel_app.primitive_settings_state.active_field_id = None;
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            eprintln!("[ChartApp] prim_settings text editing cancelled");
            return;
        }
        if self.panel_app.indicator_settings_state.active_field_id.is_some() {
            self.panel_app.indicator_settings_state.active_field_id = None;
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            eprintln!("[ChartApp] ind_settings text editing cancelled");
            return;
        }
        if self.panel_app.chart_settings_state.active_field_id.is_some() {
            self.panel_app.chart_settings_state.active_field_id = None;
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            eprintln!("[ChartApp] chart_settings text editing cancelled");
            return;
        }
        if self.panel_app.compare_settings_state.active_field_id.is_some() {
            self.panel_app.compare_settings_state.active_field_id = None;
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            eprintln!("[ChartApp] cmp_settings text editing cancelled");
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

        // Everything else that is open: OK, the ✕ and Escape are one exit
        // (owner ruling 2026-08-03), so Escape closes whatever the backdrop
        // click would close — the sidebar settings modals, overlay settings,
        // user settings, the watchlist, the colour pickers. Before this the
        // two paths kept separate hand-written lists and Escape simply did
        // nothing on five of them. An unmatched layer falls through to the
        // tool-deselect below rather than wiping the modal stack.
        if self.close_topmost_known_modal_layer() {
            return;
        }

        // Deselect drawing tool.
        self.panel_app.toolbar_state.deselect_tool();
        let active_bid_key = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        if let Some(bid) = active_bid_key {
            self.core.drawings_mut().set_tool(bid, None);
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
        self.dispatch_text_char(ch);
    }

    /// Handle named key presses that cannot be expressed as a single `char`.
    ///
    /// Routes cursor movement and delete to the active text editing state.
    /// All fields (Wave 1/2a/2b/3) route through TextFieldStore.
    pub fn on_key_press(&mut self, key: super::input::KeyPress) {
        use super::input::KeyPress;

        // While the profile manager is shown, only allow key events to reach the
        // passphrase, recovery key, or name input fields.  This prevents keyboard
        // shortcuts (Escape, arrow keys, etc.) from operating on the hidden chart UI.
        if self.panel_app.user_settings_state.show_profile_manager
            && !self.panel_app.user_settings_state.new_passphrase_focused
            && !self.panel_app.user_settings_state.new_profile_name_focused
            && !self.panel_app.user_settings_state.recovery_key_display_focused
            && !self.panel_app.user_settings_state.confirm_passphrase_focused
        {
            return;
        }

        // Early text dispatch: covers ALERT_TG_TOKEN, profile/passphrase fields,
        // BAR_MODE_PARAM, HEX_COLOR, AGENT_PTY, AGENT_CHAT — all routed before
        // global undo/redo.
        if self.dispatch_text_key_early(&key) {
            return;
        }

        // ── Global undo/redo — after early text fields, before late modals ────
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

        // Late text dispatch: covers chart_browser, watchlist search/group-name,
        // preset name, template names, settings sub-fields, search overlay.
        if self.dispatch_text_key_late(&key) {
            return;
        }

        // ── Drawing-primitive hotkeys ─────────────────────────────────────────
        // Delete / Backspace fall through unchanged (to trading-panel routing
        // below) when no primitive is selected in the active bubble.
        match key {
            KeyPress::Delete => {
                let active_bid_del = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                let selected = active_bid_del.and_then(|bid| self.core.drawings().selected(bid).map(|id| (bid, id)));
                if let Some((bid, prim_id)) = selected {
                    self.delete_primitive_with_undo(bid, prim_id);
                    return;
                }
            }
            KeyPress::Backspace => {
                // Backspace-deletes-selected-primitive (owner spec 2026-07-25).
                // The shell always sends this alongside the char-based
                // '\x08' backspace, so guard the same way as the Ctrl+C/
                // Ctrl+V primitive hotkeys (no text focus, no blocking
                // modal) — text editing itself is handled by the parallel
                // `on_char_input('\x08')` call and is unaffected here.
                if !self.primitive_hotkeys_blocked() {
                    let active_bid_bksp = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    let selected = active_bid_bksp.and_then(|bid| self.core.drawings().selected(bid).map(|id| (bid, id)));
                    if let Some((bid, prim_id)) = selected {
                        self.delete_primitive_with_undo(bid, prim_id);
                        return;
                    }
                }
            }
            KeyPress::CopyPrimitive => {
                self.copy_selected_primitive_to_clipboard();
                return;
            }
            KeyPress::PastePrimitive => {
                self.paste_primitive_at_cursor();
                return;
            }
            KeyPress::CutPrimitive => {
                self.cut_selected_primitive();
                return;
            }
            _ => {}
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
                                    use mlc_core::state::Panel;
                                    // Convert KeyPress to mlc_core::input::KeyCode for panels.
                                    // Most KeyPress variants map to specific KeyCode values; the
                                    // ones with no equivalent are passed as KeyCode::Other.
                                    let panel_key = match &key {
                                        KeyPress::ArrowUp    => mlc_core::input::KeyCode::ArrowUp,
                                        KeyPress::ArrowDown  => mlc_core::input::KeyCode::ArrowDown,
                                        KeyPress::ArrowLeft  => mlc_core::input::KeyCode::ArrowLeft,
                                        KeyPress::ArrowRight => mlc_core::input::KeyCode::ArrowRight,
                                        KeyPress::PageUp     => mlc_core::input::KeyCode::PageUp,
                                        KeyPress::PageDown   => mlc_core::input::KeyCode::PageDown,
                                        KeyPress::Home       => mlc_core::input::KeyCode::Home,
                                        KeyPress::End        => mlc_core::input::KeyCode::End,
                                        KeyPress::Enter      => mlc_core::input::KeyCode::Enter,
                                        KeyPress::Escape     => mlc_core::input::KeyCode::Escape,
                                        KeyPress::Tab        => mlc_core::input::KeyCode::Tab,
                                        KeyPress::Backspace  => mlc_core::input::KeyCode::Backspace,
                                        KeyPress::Delete     => mlc_core::input::KeyCode::Delete,
                                        _                    => mlc_core::input::KeyCode::Unknown,
                                    };
                                    let _consumed = match item_opt {
                                        Some(Panel::Dom(pid)) => {
                                            self.panels_store.dom.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(Panel::L2Tape(pid)) => {
                                            self.panels_store.l2_tape.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(Panel::TradeTape(pid)) => {
                                            self.panels_store.trade_tape.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(Panel::Footprint(pid)) => {
                                            self.panels_store.footprint.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(Panel::LiquidityHeatmap(pid)) => {
                                            self.panels_store.liquidity_heatmap.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(Panel::BigTrades(pid)) => {
                                            self.panels_store.big_trades.get_mut(&pid)
                                                .map(|s| s.handle_key(panel_key))
                                                .unwrap_or(false)
                                        }
                                        Some(Panel::VolumeProfile(pid)) => {
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
        // TextFieldStore covers all registered fields: Wave 1 (search overlays, chart browser,
        // watchlist search/group name, preset name), Wave 2a/2b (settings fields, HexColor,
        // AgentPty, AgentChat), and Wave 3 (profile manager: NEW_PASSPHRASE, NEW_PROFILE_NAME,
        // CONFIRM_PASSPHRASE, RECOVERY_KEY, PROFILE_RENAME, ALERT_TG_TOKEN).
        self.input_coordinator.borrow().text_fields().copy_selection()
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
    pub(crate) fn apply_command_to_active_window(&mut self, cmd: &mlc_core::Command) {
        use mlc_core::Command;

        // Determine up front whether we are operating in grouped or standalone mode.
        // We must not hold a borrow on panel_grid across the mutable tag_manager calls below.
        //
        // Use `shared_group_for_chart` to filter out auto-mono groups — those
        // exist on every freshly seeded chart but their primitives live in
        // `window.drawing_manager`, not `group.primitives`.  Treating an
        // auto-mono group as a real sync group here would make undo/redo of
        // primitive create/delete/modify operate on the empty `group.primitives`
        // Vec while the actual primitive sits in `window.drawing_manager`,
        // producing the symptom owner reported: "drawing undo doesn't do
        // anything" — silent no-op rather than visible error.
        let group_id = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.shared_group_for_chart(cid));
        let active_bid_cmd = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);

        match cmd {
            Command::CreatePrimitive { prim_id: _, points: _, snapshot, cascaded } => {
                // Restoring through `insert_at`'s own JSON path recreates
                // every field the concrete type carries (an Elliott wave's
                // degree/link/follows included) — never a `PrimitiveData`
                // subset. `cascaded` restores every OTHER primitive a
                // cascading delete's own inverse took with it, so a single
                // Ctrl+Z brings the whole subtree back, not just the
                // primitive originally clicked.
                if let Some(gid) = group_id {
                    let mut guard = self.core.drawings_mut();
                    let insert_pos = guard.count(gid);
                    guard.insert_at(gid, insert_pos, &snapshot.type_id, &snapshot.json);
                    for c in cascaded {
                        let pos = guard.count(gid);
                        guard.insert_at(gid, pos, &c.type_id, &c.json);
                    }
                } else if let Some(bid) = active_bid_cmd {
                    let mut guard = self.core.drawings_mut();
                    let insert_pos = guard.count(bid);
                    guard.insert_at(bid, insert_pos, &snapshot.type_id, &snapshot.json);
                    for c in cascaded {
                        let pos = guard.count(bid);
                        guard.insert_at(bid, pos, &c.type_id, &c.json);
                    }
                }
            }
            Command::DeletePrimitive { prim_id, .. } => {
                // `cascaded` needs no separate handling here: every
                // primitive it names is still linked (`WaveLink::parent_id
                // == *prim_id`) to whatever `CreatePrimitive`'s own apply
                // just restored it under, so `DrawingManager::delete`'s own
                // cascade removes them again exactly like the original
                // delete did.
                if let Some(gid) = group_id {
                    self.core.drawings_mut().delete(gid, *prim_id);
                } else if let Some(bid) = active_bid_cmd {
                    self.core.drawings_mut().delete(bid, *prim_id);
                }
            }
            Command::DeleteAllPrimitives { .. } => {
                if let Some(gid) = group_id {
                    self.core.drawings_mut().delete_all(gid);
                } else if let Some(bid) = active_bid_cmd {
                    self.core.drawings_mut().delete_all(bid);
                }
            }
            Command::RestoreAllPrimitives { primitives } => {
                if let Some(gid) = group_id {
                    let mut guard = self.core.drawings_mut();
                    guard.delete_all(gid);
                    for (i, snap) in primitives.iter().enumerate() {
                        guard.insert_at(gid, i, &snap.type_id, &snap.json);
                    }
                } else if let Some(bid) = active_bid_cmd {
                    let mut guard = self.core.drawings_mut();
                    guard.delete_all(bid);
                    for (i, snap) in primitives.iter().enumerate() {
                        guard.insert_at(bid, i, &snap.type_id, &snap.json);
                    }
                }
            }
            Command::PromoteWaveChain { parent_id, parent_snapshot, members } => {
                // Redo of a promotion, or undo of a demotion: recreate the
                // parent from its own snapshot, then re-stamp `link` on
                // every chain member — see `Command::DemoteWaveChain`'s own
                // doc for why the two directions are not symmetric enough
                // to share one apply arm (this one must NOT go through
                // `DrawingManager::delete`).
                if let Some(gid) = group_id {
                    let mut guard = self.core.drawings_mut();
                    let insert_pos = guard.count(gid);
                    guard.insert_at(gid, insert_pos, &parent_snapshot.type_id, &parent_snapshot.json);
                    for (member_id, slot) in members {
                        guard.set_wave_link(gid, *member_id, Some(WaveLink { parent_id: *parent_id, slot: *slot }));
                    }
                } else if let Some(bid) = active_bid_cmd {
                    let mut guard = self.core.drawings_mut();
                    let insert_pos = guard.count(bid);
                    guard.insert_at(bid, insert_pos, &parent_snapshot.type_id, &parent_snapshot.json);
                    for (member_id, slot) in members {
                        guard.set_wave_link(bid, *member_id, Some(WaveLink { parent_id: *parent_id, slot: *slot }));
                    }
                }
            }
            Command::DemoteWaveChain { parent_id, members, .. } => {
                // Undo of a promotion, or redo of a demotion: clear `link`
                // back to `None` on every member FIRST, then remove the
                // parent — in that order only. The members pre-date the
                // promotion and must survive it being undone; removing the
                // parent while they still point at it would fall into
                // `DrawingManager::delete`'s own subdivision cascade and
                // destroy them, which is exactly the behaviour a demote
                // must not have.
                if let Some(gid) = group_id {
                    let mut guard = self.core.drawings_mut();
                    for (member_id, _slot) in members {
                        guard.set_wave_link(gid, *member_id, None);
                    }
                    guard.delete(gid, *parent_id);
                } else if let Some(bid) = active_bid_cmd {
                    let mut guard = self.core.drawings_mut();
                    for (member_id, _slot) in members {
                        guard.set_wave_link(bid, *member_id, None);
                    }
                    guard.delete(bid, *parent_id);
                }
            }
            Command::ConvertPrimitiveKind { prim_id, after, children, .. } => {
                // Redo of a conversion, or undo of ITS OWN inverse:
                // `Command::inverse` already swapped `before`/`after` and
                // each child triple's own before/after slot, so BOTH
                // directions land here reading the SAME two fields —
                // `after` is always "the state to move TO" regardless of
                // which direction this call represents (same contract
                // `apply_command_to_active_window`'s own doc states).
                // Restored through `replace_primitive_from_json` +
                // `set_wave_link` — the same paths `ModifyPrimitiveFull`/
                // `PromoteWaveChain` already restore through, no new
                // mechanism.
                if let Some(gid) = group_id {
                    let mut guard = self.core.drawings_mut();
                    guard.replace_primitive_from_json(gid, *prim_id, &after.type_id, &after.json);
                    for (child_id, _before, after_slot) in children {
                        let link = after_slot.map(|slot| WaveLink { parent_id: *prim_id, slot });
                        guard.set_wave_link(gid, *child_id, link);
                    }
                } else if let Some(bid) = active_bid_cmd {
                    let mut guard = self.core.drawings_mut();
                    guard.replace_primitive_from_json(bid, *prim_id, &after.type_id, &after.json);
                    for (child_id, _before, after_slot) in children {
                        let link = after_slot.map(|slot| WaveLink { parent_id: *prim_id, slot });
                        guard.set_wave_link(bid, *child_id, link);
                    }
                }
            }
            Command::CreateWavePredecessor { prim_id, snapshot, next_id, .. } => {
                // Redo of a predecessor creation, or undo of its own
                // inverse (`DeleteWavePredecessor`): recreate the
                // predecessor from its own snapshot, then re-point
                // `next_id`'s own `follows` at it — see
                // `Command::DeleteWavePredecessor`'s own doc for why the
                // two directions are not symmetric enough to share one
                // apply arm (this one must NOT go through
                // `DrawingManager::delete`).
                if let Some(gid) = group_id {
                    let mut guard = self.core.drawings_mut();
                    let insert_pos = guard.count(gid);
                    guard.insert_at(gid, insert_pos, &snapshot.type_id, &snapshot.json);
                    guard.set_wave_follows(gid, *next_id, Some(*prim_id));
                } else if let Some(bid) = active_bid_cmd {
                    let mut guard = self.core.drawings_mut();
                    let insert_pos = guard.count(bid);
                    guard.insert_at(bid, insert_pos, &snapshot.type_id, &snapshot.json);
                    guard.set_wave_follows(bid, *next_id, Some(*prim_id));
                }
            }
            Command::DeleteWavePredecessor { prim_id, next_id, previous_follows, .. } => {
                // Undo of a predecessor creation, or redo of a demotion:
                // remove the predecessor primitive, then restore
                // `next_id`'s own `follows` to whatever it held before the
                // predecessor was created (`None`, or another chain
                // member it already continued) — `DrawingManager::
                // delete`'s own dangling-`follows` cleanup would otherwise
                // leave it cleared to `None` regardless of what it held
                // before.
                if let Some(gid) = group_id {
                    let mut guard = self.core.drawings_mut();
                    guard.delete(gid, *prim_id);
                    guard.set_wave_follows(gid, *next_id, *previous_follows);
                } else if let Some(bid) = active_bid_cmd {
                    let mut guard = self.core.drawings_mut();
                    guard.delete(bid, *prim_id);
                    guard.set_wave_follows(bid, *next_id, *previous_follows);
                }
            }
            Command::MovePrimitive { prim_id, new_points, .. } => {
                if let Some(gid) = group_id {
                    self.core.drawings_mut().set_points(gid, *prim_id, new_points);
                } else if let Some(bid) = active_bid_cmd {
                    self.core.drawings_mut().set_points(bid, *prim_id, new_points);
                }
            }
            Command::SetPrimitiveVisibility { prim_id, visible, .. } => {
                if let Some(gid) = group_id {
                    self.core.drawings_mut().set_visibility(gid, *prim_id, *visible);
                } else if let Some(bid) = active_bid_cmd {
                    self.core.drawings_mut().set_visibility(bid, *prim_id, *visible);
                }
            }
            Command::SetPrimitiveLock { prim_id, locked, .. } => {
                if let Some(gid) = group_id {
                    self.core.drawings_mut().set_lock(gid, *prim_id, *locked);
                } else if let Some(bid) = active_bid_cmd {
                    self.core.drawings_mut().set_lock(bid, *prim_id, *locked);
                }
            }
            Command::ModifyPrimitiveData { prim_id, new_data, .. } => {
                if let Some(gid) = group_id {
                    self.core.drawings_mut().set_data(gid, *prim_id, new_data);
                } else if let Some(bid) = active_bid_cmd {
                    self.core.drawings_mut().set_data(bid, *prim_id, new_data);
                    self.sync_drawing_back_to_group();
                }
            }
            Command::AddCompareSeries { series } => {
                // The recorded command carries no bars (`without_bars` — they
                // are re-fetchable and would put hundreds of KB per series in
                // the history), so putting the series back has to find them.
                // First chance is the overlay's own detached-bars store, which
                // holds whatever this window removed earlier in the session —
                // that covers undo-of-delete and redo-of-add.
                if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    if !w.compare_overlay.has_symbol(&series.symbol) {
                        w.compare_overlay.add_series(series.clone());
                    }
                }
                // Second chance, for a series this window never held: the
                // per-window provider cache. It is keyed to the WINDOW's own
                // venue, so it only hits for a same-venue compare symbol; a
                // miss leaves the series in the tree with no line and the user
                // can re-pick it from the compare modal to fire a fresh fetch.
                let still_empty = self.panel_app.panel_grid.active_container()
                    .and_then(|w| w.compare_overlay.get_series(&series.symbol))
                    .is_some_and(|s| s.bars.is_empty());
                if still_empty {
                    let main_tf = self.panel_app.panel_grid.active_container_id()
                        .and_then(|cid| self.timeframe_for_chart(cid))
                        .unwrap_or_else(mlc_core::state::Timeframe::h1);
                    let bars: Vec<mlc_core::Bar> = self.panel_app.panel_grid.containers().values()
                        .find_map(|w| w.data_provider.get_bars(&series.symbol, &main_tf))
                        .unwrap_or_default();
                    if let Some(first) = bars.first().copied() {
                        if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                            if let Some(s) = w.compare_overlay.get_series_mut(&series.symbol) {
                                // Re-seed base price/timestamp from the first
                                // bar so the percentage axis lines up.
                                s.base_price = first.close;
                                s.base_timestamp = first.timestamp;
                                s.bars = bars;
                            }
                        }
                    }
                }
            }
            Command::RemoveCompareSeries { symbol, .. } => {
                if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    if w.compare_overlay.has_symbol(symbol) {
                        w.compare_overlay.remove_series_by_symbol(symbol);
                    }
                }
            }
            Command::SetCompareSeriesVisibility { symbol, visible, .. } => {
                if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    if let Some(s) = w.compare_overlay.get_series_mut(symbol) {
                        s.visible = *visible;
                    }
                }
            }
            Command::SetCompareSeriesStyle { symbol, new, .. } => {
                let mut editing_this = false;
                if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    if let Some(s) = w.compare_overlay.get_series_mut(symbol) {
                        s.set_style(new);
                    }
                    let idx = self.panel_app.compare_settings_state.series_index;
                    editing_this = w
                        .compare_overlay
                        .series
                        .get(idx)
                        .is_some_and(|s| &s.symbol == symbol);
                }
                // The settings modal caches what it draws and nothing
                // refreshes that cache on its own — without this an undo
                // would restyle the line while the modal kept showing the
                // old swatch, width and style.
                if editing_this {
                    let st = &mut self.panel_app.compare_settings_state;
                    st.cached_color = new.color.clone();
                    st.cached_line_width = new.line_width;
                    st.cached_line_style = new.line_style.clone();
                    st.cached_chart_type = new.chart_type.clone();
                }
            }
            Command::ClearAllCompareSeries { .. } => {
                if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
                    w.compare_overlay.clear();
                }
            }
            Command::RestoreAllCompareSeries { series } => {
                if let Some(w) = self.panel_app.panel_grid.active_container_mut() {
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
                let chart_id_val = match self.panel_app.panel_grid.active_container() {
                    Some(w) => w.id,
                    None => return,
                };
                let bid = self.panel_app.panel_grid.containers()
                    .get(&chart_id_val)
                    .map(|w| w.current_tag_id);
                if self.core.indicators_mut().restore_with_id(*instance_id, bid, type_id) {
                    let bars_snapshot = self.bars_for_chart(chart_id_val);
                    self.core.indicators_mut().calculate(*instance_id, &bars_snapshot);
                    // Re-add to auto_bubble.pre_tag_indicator_ids for Object Tree visibility.
                    let auto_bid = self.panel_app.panel_grid.containers()
                        .get(&chart_id_val)
                        .map(|w| w.auto_tag_id);
                    if let Some(bid) = auto_bid {
                        self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                            if !auto_bub.pre_tag_indicator_ids.contains(instance_id) {
                                auto_bub.pre_tag_indicator_ids.push(*instance_id);
                            }
                        });
                    }
                    self.sync_sub_panes_from_manager();
                    eprintln!("[Undo/Redo] Re-created indicator {} (id={}) window_id={}", type_id, instance_id, chart_id_val.0);
                } else {
                    eprintln!("[Undo/Redo] Failed to re-create indicator {} (id={})", type_id, instance_id);
                }
            }
            Command::RemoveIndicator { instance_id, .. } => {
                if self.core.indicators_mut().remove(*instance_id).is_some() {
                    // Remove stale entries from auto_bubble.pre_tag_indicator_ids.
                    let removed_id = *instance_id;
                    let auto_bids: Vec<mlc_core::ids::BubbleId> = self.panel_app.panel_grid
                        .containers()
                        .values()
                        .map(|w| w.auto_tag_id)
                        .filter(|&bid| bid != mlc_core::ids::BubbleId(0))
                        .collect();
                    for bid in auto_bids {
                        self.core.bubbles_mut().with_bubble_mut(bid, |auto_bub| {
                            auto_bub.pre_tag_indicator_ids.retain(|iid| *iid != removed_id);
                        });
                    }
                    self.sync_sub_panes_from_manager();
                    eprintln!("[Undo/Redo] Removed indicator instance {}", instance_id);
                } else {
                    eprintln!("[Undo/Redo] Indicator instance {} not found", instance_id);
                }
            }
            Command::ModifyPrimitiveFull { prim_id, type_id, new_json, .. } => {
                // Was falling into the unhandled catch-all — recorded, never
                // applied. Rebuilds the primitive from the recorded JSON, so
                // it covers type-specific state (fib levels and the like) that
                // `ModifyPrimitiveData` cannot express.
                if let Some(bid) = self.bubble_holding_primitive(*prim_id) {
                    self.core.drawings_mut().replace_primitive_from_json(bid, *prim_id, type_id, new_json);
                    self.sidebar_data_dirty = true;
                }
            }
            Command::ModifyIndicator { instance_id, new_json, .. } => {
                if self.core.indicators_mut().replace_from_json(*instance_id, new_json) {
                    let chart_id = self.panel_app.panel_grid.active_container().map(|w| w.id);
                    if let Some(cid) = chart_id {
                        let bars = self.bars_for_chart(cid);
                        self.core.indicators_mut().calculate(*instance_id, &bars);
                    }
                    self.sync_sub_panes_from_manager();
                    self.sidebar_data_dirty = true;
                }
            }
            Command::AddAlert { snapshot } => {
                // Restore a previously deleted (or redo-add) alert from its snapshot.
                let alert_id = snapshot.id;
                if self.core.alerts().find_owner(alert_id).is_none() {
                    // bid resolution is app-level policy (which bubble wins when
                    // the snapshot's own group_id doesn't resolve) — design doc §6.4.
                    let target_bid = snapshot.group_id
                        .map(mlc_core::ids::BubbleId)
                        .or_else(|| self.panel_app.panel_grid.active_container_id()
                            .and_then(|cid| self.bubble_for_chart(cid)));
                    if let Some(bid) = target_bid {
                        self.core.alerts_mut().restore_one(bid, snapshot.clone());
                    }
                    self.sidebar_data_dirty = true;
                    eprintln!("[Undo/Redo] Restored alert id={}", alert_id);
                }
            }
            Command::DeleteAlert { snapshot } => {
                let alert_id = snapshot.id;
                let bid_opt = self.core.alerts().find_owner(alert_id).map(|(bid, _)| bid);
                if let Some(bid) = bid_opt {
                    self.core.alerts_mut().remove(bid, alert_id);
                    self.sidebar_data_dirty = true;
                    eprintln!("[Undo/Redo] Deleted alert id={}", alert_id);
                }
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
            .active_container()
            .map(|win| win.scale_settings.clone())
            .unwrap_or_default();

        // Build the set of visible sub-pane instance IDs from indicator_manager
        // (single source of truth for visibility).  Collect before borrowing
        // active_container() again so the borrow checker stays happy.
        // S2.7.1e: read symbol via branch_meta (B1).
        let visible_set: std::collections::HashSet<u64> = {
            let active_bid = self.panel_app.panel_grid.active_container()
                .map(|w| w.current_tag_id);
            let indicators_guard = self.core.indicators();
            active_bid
                .map(|bid| indicators_guard.instances_for_bubble(bid))
                .unwrap_or_default()
                .into_iter()
                .filter(|i| i.visible && i.pane > 0)
                .map(|i| i.id)
                .collect()
        };

        // Both order and proportions from the container's inner_docking tree,
        // filtered by indicator_manager visibility.
        // When root.proportions is empty (default before any separator drag),
        // pass empty sub_proportions so from_proportions falls back to the
        // same InnerLayout::sync_with defaults the renderer uses — avoids
        // collapsing panes to minimum height (30px) when proportions are zeros.
        let (sub_pane_ids, sub_proportions, main_proportion): (Vec<u64>, Vec<f64>, Option<f64>) = self.panel_app.panel_grid
            .active_container()
            .map(|win| {
                use crate::chart_container::{collect_inner_instance_ids, collect_inner_proportions};
                let ids = collect_inner_instance_ids(&win.inner_docking);
                let props = collect_inner_proportions(&win.inner_docking);
                // props is [main_share, sub0_share, ...]; empty when the uzor
                // docking tree root.proportions has not been written yet.
                // Mirror the renderer: only pass sub-proportions when there is
                // at least one real value (props.len() > 1, same guard the
                // renderer uses in render_chart.rs).
                if props.len() > 1 {
                    let (ids_f, subs): (Vec<u64>, Vec<f64>) = ids.iter()
                        .enumerate()
                        .filter(|(_, id)| visible_set.contains(id))
                        .map(|(i, &id)| (id, props.get(i + 1).copied().unwrap_or(0.0)))
                        .unzip();
                    (ids_f, subs, props.first().copied())
                } else {
                    let filtered_ids: Vec<u64> = ids
                        .iter()
                        .filter(|id| visible_set.contains(*id))
                        .copied()
                        .collect();
                    let empty_props = vec![0.0_f64; 0];
                    (filtered_ids, empty_props, None)
                }
            })
            .unwrap_or_default();
        // When a sub-pane is maximized, mirror the same override that
        // render_full_chart_panel applies so input hit-testing (clicks, hover,
        // separator drag) operates on the correct fullscreen geometry.
        let maximized_id = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.maximized_sub_panes.get(&cid.0))
            .map(|(iid, _)| *iid)
            .filter(|iid| sub_pane_ids.contains(iid));

        let empty_above = std::collections::HashSet::new();
        let above_main_ids: &std::collections::HashSet<u64> = self.panel_app.panel_grid
            .active_container()
            .map(|w| &w.sub_pane_above_main)
            .unwrap_or(&empty_above);

        if let Some(max_iid) = maximized_id {
            ExtendedFrameLayout::maximized(&content_rect, max_iid, &scale_settings)
        } else {
            ExtendedFrameLayout::from_proportions(
                &content_rect,
                &sub_pane_ids,
                &sub_proportions,
                main_proportion,
                &scale_settings,
                1.0,
                above_main_ids,
            )
        }
    }

    /// Build an `ExtendedFrameLayout` for a specific split leaf.
    ///
    /// Unlike `build_extended_layout()` which uses the toolbar-carved content rect
    /// for the active window, this uses the leaf's own absolute rect as the
    /// layout area (minimal config = no toolbar carving).
    pub(crate) fn build_extended_layout_for_leaf(
        &self,
        leaf_id: mlc_core::LeafId,
        leaf_rect: &LayoutRect,
    ) -> Option<ExtendedFrameLayout> {
        let window = self.panel_app.panel_grid.container_for_leaf(leaf_id)?;
        let scale_settings = &window.scale_settings;

        // Build visible set from indicator_manager (single source of truth).
        // S2.7.1e: read symbol via branch_meta (B1).
        let visible_set: std::collections::HashSet<u64> = self.core.indicators()
            .instances_for_bubble(window.current_tag_id)
            .into_iter()
            .filter(|i| i.visible && i.pane > 0)
            .map(|i| i.id)
            .collect();

        use crate::chart_container::{collect_inner_instance_ids, collect_inner_proportions};
        let all_ids = collect_inner_instance_ids(&window.inner_docking);
        let all_props = collect_inner_proportions(&window.inner_docking);
        // Mirror the renderer: only pass sub-proportions when the uzor docking
        // tree root has real values (len > 1).  Empty root.proportions must
        // produce an empty Vec here so from_proportions falls back to
        // InnerLayout::sync_with defaults — the same path the renderer takes.
        let (sub_pane_ids, sub_proportions): (Vec<u64>, Vec<f64>) = if all_props.len() > 1 {
            all_ids
                .iter()
                .enumerate()
                .filter(|(_, id)| visible_set.contains(id))
                .map(|(i, &id)| (id, all_props.get(i + 1).copied().unwrap_or(0.0)))
                .unzip()
        } else {
            let filtered_ids: Vec<u64> = all_ids
                .iter()
                .filter(|id| visible_set.contains(*id))
                .copied()
                .collect();
            (filtered_ids, Vec::new())
        };
        let main_proportion = all_props.first().copied().filter(|_| all_props.len() > 1);
        // When a sub-pane is maximized, mirror the same override that
        // render_full_chart_panel applies so input hit-testing operates on
        // the correct fullscreen geometry for this leaf.
        let container_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
        let maximized_id = container_id
            .and_then(|cid| self.maximized_sub_panes.get(&cid.0))
            .map(|(iid, _)| *iid)
            .filter(|iid| sub_pane_ids.contains(iid));

        let empty_above2 = std::collections::HashSet::new();
        let above_ids: &std::collections::HashSet<u64> = self.panel_app.panel_grid
            .container_for_leaf(leaf_id)
            .map(|w| &w.sub_pane_above_main)
            .unwrap_or(&empty_above2);

        if let Some(max_iid) = maximized_id {
            Some(ExtendedFrameLayout::maximized(leaf_rect, max_iid, scale_settings))
        } else {
            Some(ExtendedFrameLayout::from_proportions(
                leaf_rect,
                &sub_pane_ids,
                &sub_proportions,
                main_proportion,
                scale_settings,
                1.0,
                above_ids,
            ))
        }
    }

    /// Convert a leaf's panel-relative rect to an absolute screen rect.
    ///
    /// `panel_rects()` returns rects relative to the content area top-left.
    /// This method adds `content_rect` as an offset so the result is in
    /// absolute screen coordinates, matching the coordinate space used by
    /// mouse events.
    pub(crate) fn get_leaf_absolute_rect(
        &self,
        leaf_id: mlc_core::LeafId,
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
        let leaf_ids: Vec<mlc_core::LeafId> = self.panel_app
            .panel_grid
            .panel_rects()
            .keys()
            .copied()
            .collect();
        for leaf_id in leaf_ids {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
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
        // `shared_group_for_chart` (not `bubble_for_chart`) so auto-mono
        // groups — present on every fresh chart, but whose primitives live in
        // the window — are not consulted; their commands went to the window.
        let group_id = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.shared_group_for_chart(cid));

        let group_seq = group_id.and_then(|gid| {
            self.core.bubbles().command_history(gid).and_then(|h| h.next_undo_seq())
        });
        let local_seq = self.panel_app.panel_grid.active_container()
            .and_then(|w| w.command_history.next_undo_seq());

        // Newest first, across BOTH stacks — undo has to follow the order the
        // user acted in, not the order the stacks happen to be declared in.
        let take_group = match (group_seq, local_seq) {
            (Some(g), Some(l)) => g > l,
            (Some(_), None) => true,
            _ => false,
        };

        let cmd = if take_group {
            group_id.and_then(|gid| {
                self.core.bubbles_mut()
                    .with_bubble_mut(gid, |g| g.command_history.undo())
                    .flatten()
            })
        } else {
            self.panel_app.panel_grid.active_container_mut()
                .and_then(|w| w.command_history.undo())
        };

        let Some(cmd) = cmd else {
            eprintln!("[ChartApp] Nothing to undo");
            return;
        };
        let desc = cmd.description();
        let inverse = cmd.inverse();
        self.apply_command_to_active_window(&inverse);
        self.post_apply_command_effects(&inverse);
        eprintln!("[ChartApp] Undo ({}): {}", if take_group { "group" } else { "local" }, desc);
    }

    /// Execute a redo step, routing through the group history for shared
    /// operations when the active window is in a sync group.
    fn perform_redo_with_group(&mut self) {
        let group_id = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.shared_group_for_chart(cid));

        let group_seq = group_id.and_then(|gid| {
            self.core.bubbles().command_history(gid).and_then(|h| h.next_redo_seq())
        });
        let local_seq = self.panel_app.panel_grid.active_container()
            .and_then(|w| w.command_history.next_redo_seq());

        // Mirror of undo: undo walked the sequence DOWN, so redo walks it back
        // UP — the oldest of the two redo tops is the one undone most recently.
        let take_group = match (group_seq, local_seq) {
            (Some(g), Some(l)) => g < l,
            (Some(_), None) => true,
            _ => false,
        };

        let cmd = if take_group {
            group_id.and_then(|gid| {
                self.core.bubbles_mut()
                    .with_bubble_mut(gid, |g| g.command_history.redo())
                    .flatten()
            })
        } else {
            self.panel_app.panel_grid.active_container_mut()
                .and_then(|w| w.command_history.redo())
        };

        let Some(cmd) = cmd else {
            eprintln!("[ChartApp] Nothing to redo");
            return;
        };
        let desc = cmd.description();
        self.apply_command_to_active_window(&cmd);
        self.post_apply_command_effects(&cmd);
        eprintln!("[ChartApp] Redo ({}): {}", if take_group { "group" } else { "local" }, desc);
    }

    /// Post-apply hook: run side-effects that the forward path does but
    /// `apply_command_to_active_window` does not (indicator recalc, group
    /// propagation, autosave).
    fn post_apply_command_effects(&mut self, cmd: &mlc_core::Command) {
        use mlc_core::Command;
        match cmd {
            Command::AddIndicator { .. } | Command::RemoveIndicator { .. } => {
                self.autosave_snapshot();
            }
            Command::CreatePrimitive { .. } | Command::DeletePrimitive { .. }
            | Command::DeleteAllPrimitives { .. } | Command::RestoreAllPrimitives { .. }
            | Command::MovePrimitive { .. } | Command::ModifyPrimitiveData { .. }
            | Command::ModifyPrimitiveFull { .. }
            | Command::PromoteWaveChain { .. } | Command::DemoteWaveChain { .. }
            | Command::ConvertPrimitiveKind { .. }
            | Command::CreateWavePredecessor { .. } | Command::DeleteWavePredecessor { .. } => {
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

    /// Remember how the edited compare series looks BEFORE a slider drag.
    ///
    /// The drag writes live on every pixel so the stack can not read the
    /// previous write — it has to hold where the drag began.
    pub(crate) fn capture_compare_style_for_drag(&mut self) {
        let idx = self.panel_app.compare_settings_state.series_index;
        self.compare_style_before_drag = self
            .panel_app
            .panel_grid
            .active_container()
            .and_then(|w| w.compare_overlay.series.get(idx))
            .map(|s| (s.symbol.clone(), s.style()));
    }

    /// Close a slider drag with ONE command spanning the whole gesture.
    pub(crate) fn record_compare_style_after_drag(&mut self) {
        let Some((symbol, previous)) = self.compare_style_before_drag.take() else {
            return;
        };
        let new = self
            .panel_app
            .panel_grid
            .active_container()
            .and_then(|w| w.compare_overlay.series.iter().find(|s| s.symbol == symbol))
            .map(|s| s.style());
        if let Some(new) = new {
            if new != previous {
                self.push_undo_command(mlc_core::Command::SetCompareSeriesStyle { symbol, previous, new });
            }
        }
    }

    /// Run a presentation edit on a compare series and record it as ONE undo
    /// step.
    ///
    /// Snapshots before and after and records nothing when nothing moved: the
    /// colour picker re-applies the colour it already has as the cursor
    /// travels, and each of those would otherwise be a command that changes
    /// nothing. Grouped edits — "reset to defaults", "apply template" — go
    /// through here as a single closure and therefore undo in one press.
    pub(crate) fn edit_compare_style(
        &mut self,
        idx: usize,
        edit: impl FnOnce(&mut mlc_core::chart::CompareOverlay),
    ) {
        let recorded = {
            let Some(window) = self.panel_app.panel_grid.active_container_mut() else {
                return;
            };
            let before = window
                .compare_overlay
                .series
                .get(idx)
                .map(|s| (s.symbol.clone(), s.style()));
            edit(&mut window.compare_overlay);
            before.and_then(|(symbol, previous)| {
                window
                    .compare_overlay
                    .series
                    .get(idx)
                    .map(|s| s.style())
                    .filter(|new| *new != previous)
                    .map(|new| (symbol, previous, new))
            })
        };
        if let Some((symbol, previous, new)) = recorded {
            self.push_undo_command(mlc_core::Command::SetCompareSeriesStyle { symbol, previous, new });
        }
    }

    /// Lift the command sequence above everything a cold load restored.
    ///
    /// Both histories a chart undoes from are ordered against each other by
    /// this counter, so it has to be global AND monotonic across sessions —
    /// a counter that restarted at zero after a reload would stamp the first
    /// new command below every restored one, and undo would reach for the
    /// restored commands first no matter what the user just did.
    pub(crate) fn resume_undo_sequence(&mut self) {
        let mut high = self.undo_seq;
        for window in self.panel_app.panel_grid.containers().values() {
            high = high.max(window.command_history.max_seq());
        }
        {
            let bubbles = self.core.bubbles();
            let ids: Vec<_> = bubbles.bubble_ids().collect();
            for bid in ids {
                if let Some(h) = bubbles.command_history(bid) {
                    high = high.max(h.max_seq());
                }
            }
        }
        self.undo_seq = high;
    }

    /// Run an edit on an indicator instance and record it as ONE undo step.
    ///
    /// Snapshots the whole instance before and after — parameters, outputs,
    /// venues — so every field of the settings modal is covered by one
    /// command, and nothing is recorded when nothing moved.
    pub(crate) fn edit_indicator_recorded<R>(
        &mut self,
        instance_id: u64,
        edit: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if self.edit_recording_depth > 0 {
            return edit(self);
        }
        let before = self.core.indicators().full_clone(instance_id)
            .and_then(|i| serde_json::to_string(&i).ok());
        self.edit_recording_depth += 1;
        let out = edit(self);
        self.edit_recording_depth -= 1;
        let Some(previous_json) = before else { return out };
        let after = self.core.indicators().full_clone(instance_id)
            .and_then(|i| serde_json::to_string(&i).ok());
        if let Some(new_json) = after {
            if new_json != previous_json {
                self.push_undo_command(mlc_core::Command::ModifyIndicator {
                    instance_id,
                    previous_json,
                    new_json,
                });
            }
        }
        out
    }

    /// Try to fold a primitive edit into the command already on top of
    /// whichever history this chart records into.
    fn coalesce_primitive_modify(&mut self, prim_id: u64, previous_json: &str, new_json: &str) -> bool {
        let gid = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.shared_group_for_chart(cid));
        if let Some(gid) = gid {
            if self.core.bubbles().exists(gid) {
                return self.core.bubbles_mut()
                    .with_bubble_mut(gid, |g| {
                        g.command_history.coalesce_primitive_modify(prim_id, previous_json, new_json)
                    })
                    .unwrap_or(false);
            }
        }
        self.panel_app.panel_grid.active_container_mut()
            .map(|w| w.command_history.coalesce_primitive_modify(prim_id, previous_json, new_json))
            .unwrap_or(false)
    }

    /// Which bubble actually holds `prim_id`.
    ///
    /// A primitive can sit in the sync group, in the window's own bubble, or
    /// in the window's auto-bubble stash. Assuming one of them is how undo
    /// ends up silently matching nothing.
    pub(crate) fn bubble_holding_primitive(&self, prim_id: u64) -> Option<mlc_core::BubbleId> {
        let mut candidates: Vec<mlc_core::BubbleId> = Vec::new();
        if let Some(cid) = self.panel_app.panel_grid.active_container_id() {
            if let Some(gid) = self.shared_group_for_chart(cid) {
                candidates.push(gid);
            }
        }
        if let Some(w) = self.panel_app.panel_grid.active_container() {
            candidates.push(w.current_tag_id);
            candidates.push(w.auto_tag_id);
        }
        candidates.dedup();
        let drawings = self.core.drawings();
        candidates.into_iter().find(|bid| drawings.get_type_id(*bid, prim_id).is_some())
    }

    /// Run an edit on the selected primitive and record it as ONE undo step.
    ///
    /// Snapshots the primitive's whole JSON before and after, so this covers
    /// colour, width, style, text AND type-specific state (fib levels and the
    /// like) with a single command, and records nothing when nothing moved.
    pub(crate) fn edit_primitive_recorded<R>(
        &mut self,
        prim_id: u64,
        edit: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if self.edit_recording_depth > 0 {
            return edit(self); // an inner wrapper — the outermost one records
        }
        let bid = self.bubble_holding_primitive(prim_id);
        let before = bid.and_then(|b| self.core.drawings().get_primitive_json(b, prim_id));
        let type_id = bid.and_then(|b| self.core.drawings().get_type_id(b, prim_id));
        self.edit_recording_depth += 1;
        let out = edit(self);
        self.edit_recording_depth -= 1;
        let (Some(bid), Some(before), Some(type_id)) = (bid, before, type_id) else {
            return out;
        };
        let after = self.core.drawings().get_primitive_json(bid, prim_id);
        if let Some(after) = after {
            if after != before && !self.coalesce_primitive_modify(prim_id, &before, &after) {
                self.push_undo_command(mlc_core::Command::ModifyPrimitiveFull {
                    prim_id,
                    type_id,
                    previous_json: before,
                    new_json: after,
                });
            }
        }
        out
    }

    /// Remember a primitive's whole state BEFORE a slider drag.
    ///
    /// A drag writes on every pixel, so the state to restore is where the
    /// drag began, not the previous write.
    pub(crate) fn capture_primitive_json_for_drag(&mut self, prim_id: u64) {
        let bid = self.bubble_holding_primitive(prim_id);
        self.primitive_json_before_drag = bid.and_then(|b| {
            let drawings = self.core.drawings();
            let json = drawings.get_primitive_json(b, prim_id)?;
            let type_id = drawings.get_type_id(b, prim_id)?;
            Some((prim_id, type_id, json))
        });
    }

    /// Close a primitive slider drag with ONE command for the whole gesture.
    pub(crate) fn record_primitive_json_after_drag(&mut self) {
        let Some((prim_id, type_id, previous_json)) = self.primitive_json_before_drag.take() else {
            return;
        };
        let Some(bid) = self.bubble_holding_primitive(prim_id) else { return };
        let new_json = self.core.drawings().get_primitive_json(bid, prim_id);
        if let Some(new_json) = new_json {
            if new_json != previous_json {
                self.push_undo_command(mlc_core::Command::ModifyPrimitiveFull {
                    prim_id,
                    type_id,
                    previous_json,
                    new_json,
                });
            }
        }
    }

    /// Match a restored history against the drawings that actually exist.
    ///
    /// A cold load re-numbers primitives — the collision sweep re-IDs stashed
    /// primitives that clash with a group's, and `dedup_primitive_ids` re-IDs
    /// duplicates — so every id in a restored command can be stale. Live
    /// symptom, measured on the stand: the saved history said
    /// `CreatePrimitive:48/49/50` while the bubble held primitives 59..69, so
    /// three undos deleted nothing and three redos ADDED three more copies of
    /// drawings that were already on the chart.
    ///
    /// A primitive's identity across a re-numbering is its type plus its
    /// geometry, so that is the key: commands that carry both (create,
    /// delete) locate their primitive again and every command for that
    /// primitive is rewritten to the new id. What cannot be located is
    /// dropped — a command aimed at a dead id is worse than no command.
    pub(crate) fn reconcile_history_primitive_ids(&mut self) {
        type Key = (String, Vec<(i64, u64)>);
        fn key(type_id: &str, points: &[(i64, f64)]) -> Key {
            (
                type_id.to_string(),
                points.iter().map(|(t, p)| (*t, p.to_bits())).collect(),
            )
        }

        // 1. What is actually on the chart, keyed by identity.
        let mut live_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();
        let mut by_identity: std::collections::HashMap<Key, u64> = std::collections::HashMap::new();
        {
            let drawings = self.core.drawings();
            let bids: Vec<_> = self.core.bubbles().bubble_ids().collect();
            for bid in bids {
                for item in drawings.primitive_list(bid) {
                    live_ids.insert(item.id);
                    if let (Some(type_id), Some(points)) =
                        (drawings.get_type_id(bid, item.id), drawings.get_points(bid, item.id))
                    {
                        by_identity.entry(key(&type_id, &points)).or_insert(item.id);
                    }
                }
            }
        }

        // 2. What the histories claim, and where each claim now points.
        let mut identities: Vec<(u64, String, Vec<(i64, f64)>)> = Vec::new();
        for window in self.panel_app.panel_grid.containers().values() {
            identities.extend(window.command_history.primitive_identities());
        }
        {
            let bubbles = self.core.bubbles();
            let bids: Vec<_> = bubbles.bubble_ids().collect();
            for bid in bids {
                if let Some(h) = bubbles.command_history(bid) {
                    identities.extend(h.primitive_identities());
                }
            }
        }
        let mut remap: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        for (old_id, type_id, points) in identities {
            if live_ids.contains(&old_id) {
                continue;
            }
            if let Some(&new_id) = by_identity.get(&key(&type_id, &points)) {
                remap.insert(old_id, new_id);
            }
        }

        // 3. Rewrite, and drop what cannot be placed.
        let mut dropped = 0usize;
        for window in self.panel_app.panel_grid.containers_mut().values_mut() {
            dropped += window.command_history.remap_and_prune_primitives(&remap, &live_ids);
        }
        let bids: Vec<_> = self.core.bubbles().bubble_ids().collect();
        for bid in bids {
            self.core.bubbles_mut().with_bubble_mut(bid, |b| {
                dropped += b.command_history.remap_and_prune_primitives(&remap, &live_ids);
            });
        }
        if !remap.is_empty() || dropped > 0 {
            eprintln!(
                "[ChartApp] History reconciled after load: {} primitive id(s) rewritten, {} command(s) dropped",
                remap.len(),
                dropped
            );
        }
    }

    /// Push a command to the appropriate history stack.
    ///
    /// Shared commands (primitives, indicators) go to the sync group's
    /// `command_history` when the active window is in a group. Compare
    /// series live on the window itself, so their commands always go to the
    /// active window's own `command_history`.
    pub(crate) fn push_undo_command(&mut self, cmd: mlc_core::Command) {
        use mlc_core::Command;
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
                | Command::PromoteWaveChain { .. }
                | Command::DemoteWaveChain { .. }
                | Command::ConvertPrimitiveKind { .. }
                | Command::CreateWavePredecessor { .. }
                | Command::DeleteWavePredecessor { .. }
                | Command::AddIndicator { .. }
                | Command::RemoveIndicator { .. }
                | Command::ModifyIndicator { .. }
                | Command::AddAlert { .. }
                | Command::DeleteAlert { .. }
        );

        // Use `shared_group_for_chart` (not `bubble_for_chart`) so auto-mono
        // groups — present on every freshly seeded chart but whose primitives
        // live in `window.drawing_manager`, not `group.primitives` — fall
        // through to the per-window command_history.  Pushing to
        // `group.command_history` for an auto-mono group would route undo
        // through `apply_command_to_active_window`'s grouped branch (which
        // would `group.primitives.remove(idx)` against an empty Vec — silent
        // no-op).  Matches the auto-mono guards already in
        // `intercept_completed_primitive_to_group` (sync_group.rs:1596) and
        // `prepare_frame`.
        let group_id = if is_shared {
            self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.shared_group_for_chart(cid))
        } else {
            None
        };

        // One counter for the whole app: the two stacks are ordered against
        // each other by it, so it must not restart per window or per group.
        self.undo_seq = self.undo_seq.wrapping_add(1);
        let seq = self.undo_seq;

        // A new action invalidates the redo future — ALL of it. `push` clears
        // the redo stack it is pushed onto, which is only half the job here:
        // undo a drawing, add a compare series, undo THAT, press redo, and the
        // drawing came back instead of the compare, because the group's redo
        // stack still held it. Both histories this chart undoes from have to
        // be cleared, whichever one the new command lands in.
        let shared_gid = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.shared_group_for_chart(cid));
        if let Some(gid) = shared_gid {
            if self.core.bubbles().exists(gid) {
                self.core.bubbles_mut().with_bubble_mut(gid, |g| g.command_history.clear_redo());
            }
        }
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.command_history.clear_redo();
        }

        if let Some(gid) = group_id {
            if self.core.bubbles().exists(gid) {
                self.core.bubbles_mut().with_bubble_mut(gid, |group| group.command_history.push(seq, cmd));
                return;
            }
        }

        // Local command or no real group — push to the active window's history.
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.command_history.push(seq, cmd);
        }
    }

    // -------------------------------------------------------------------------
    // Autosave
    // -------------------------------------------------------------------------

    /// Request an autosave snapshot of the active preset.
    ///
    /// Sets `pending_autosave = true` instead of running the full
    /// collect+upsert immediately. The deferred snapshot runs at most once
    /// per frame from `flush_pending_autosave`, which is invoked late in the
    /// tick after all input handlers + sync-group propagations + command
    /// history settle. Previously every callsite (5-7 of them on a single
    /// preset switch — handler + sync-group + park + load + command history)
    /// triggered a full `collect_state_into_preset` deep-copy, freezing the
    /// UI for 50-200 ms per switch.
    ///
    /// Disabled autosave / empty preset id checks are still applied here so
    /// the deferred path also no-ops.
    pub fn autosave_snapshot(&mut self) {
        if !self.panel_app.autosave_enabled { return; }
        if self.panel_app.active_preset_id.is_empty() { return; }
        // Capture the target ID NOW. By flush time `active_preset_id` may
        // already point at a new preset (LoadPreset advances it before the
        // end-of-tick flush). Without this snapshot, the outgoing preset's
        // edits would be silently dropped on every warm switch.
        let target_id = self.panel_app.active_preset_id.clone();
        // If multiple callsites fire in one frame for the same target, keep
        // the first (current materialized state). If they fire for DIFFERENT
        // targets, the more-recent one wins — second autosave_snapshot is
        // usually called AFTER a switch and represents the user's current
        // intent. Either way one flush per frame, no thrash.
        self.pending_autosave = Some(target_id);
    }

    /// Run the pending autosave once per frame. Called from the app tick.
    pub fn flush_pending_autosave(&mut self) {
        let Some(target_id) = self.pending_autosave.take() else { return; };

        if !self.panel_app.autosave_enabled { return; }
        // LoadState gate. While a cold hydrate is in flight, `panel_grid`
        // has been parked into `ChartPanelGrid::placeholder()` and the new
        // `active_preset_id` is already set. Snapshot-and-Upsert here would
        // overwrite the real preset file on disk with placeholder content.
        // This is the `7a9d1d2` disaster path closed in code.
        if !crate::persistence::autosave::is_persist_safe(&self.load_state) {
            eprintln!(
                "[ChartApp] autosave skipped — load_state={:?} (cold hydrate in flight, refusing to persist placeholder)",
                self.load_state,
            );
            return;
        }
        // CRUCIAL: only autosave when the captured target IS the currently
        // active preset. After a warm switch, `target_id` is the OUTGOING
        // preset but `active_preset_id`+`panel_grid` are the INCOMING one.
        // Persisting the captured outgoing id with the incoming grid would
        // overwrite the outgoing preset on disk with the incoming layout —
        // the exact "data disappears" symptom owner reported.
        //
        // The outgoing preset's pre-switch state lives in `live_preset_cache`
        // (parked by LoadPreset). When the user switches BACK to it, unpark
        // restores those exact bytes. The pre-park `autosave_snapshot()`
        // call at the start of the LoadPreset handler is what flushes them
        // to disk — but that path was previously broken by this race.
        // Fixed now: the autosave for the outgoing preset must run BEFORE
        // active_preset_id is changed, or be done synchronously in the
        // LoadPreset handler (still TODO — see below).
        if target_id != self.panel_app.active_preset_id {
            eprintln!(
                "[ChartApp] autosave skipped — target='{}' but active='{}' (warm switch race, outgoing autosave should run synchronously)",
                target_id, self.panel_app.active_preset_id,
            );
            return;
        }
        let id = target_id;
        if id.is_empty() { return; }

        let name = if let Some(preset) = self.core.presets().get(&id) {
            preset.name.clone()
        } else if id == "__default__" {
            "Default".to_string()
        } else {
            eprintln!("[ChartApp] autosave: preset id '{}' not found in presets map", id);
            return;
        };

        let _t0 = web_time::Instant::now();
        self.collect_state_into_preset(&id, &name);
        // Bound first so the `presets()` read guard drops before the body runs
        // (clippy::significant_drop_in_scrutinee).
        let saved_preset = self.core.presets().get(&id).cloned();
        if let Some(preset) = saved_preset {
            self.preset_actions.push(crate::PresetAction::Upsert(preset));
        }
        eprintln!(
            "[ChartApp] autosave snapshot for '{}' (persisted) collect_us={}",
            name, _t0.elapsed().as_micros(),
        );
    }

    // -------------------------------------------------------------------------
    // New-chart helper
    // -------------------------------------------------------------------------

    /// Execute the "New Chart" action with a user-supplied name.
    ///
    /// 1. Auto-saves the current state if it has no named preset yet (__default__).
    /// 2. Performs a full reset (single layout, no primitives, no indicators).
    /// 3. Creates the new blank preset with the given name and makes it active.
    pub(crate) fn execute_new_chart_with_name(&mut self, new_name: String) {
        use mlc_core::preset::preset::unix_timestamp_parts;
        sglog!("[execute_new_chart_with_name] ENTER name='{}'", new_name);

        // Auto-save current named preset before reset.
        // (__default__ is already saved in OpenPresetNewChart before the modal opens.)
        if self.panel_app.active_preset_id != "__default__" && !self.panel_app.active_preset_id.is_empty() {
            self.autosave_snapshot();
        }

        // Hard defaults — new chart always starts clean.
        let default_symbol = "BTCUSDT".to_string();
        let default_timeframe = mlc_core::state::Timeframe::h1();

        // Park the outgoing preset's full session state into `live_preset_cache`
        // so switching back via a chrome tab restores everything (panel_grid +
        // tag_manager + indicator_manager + alert_manager + panels_store + ...)
        // via the same path `LoadPreset` warm-cache uses.  Without this the
        // global session state stays bound to the OUTGOING preset id, the new
        // preset's reset stomps it in place, and clicking the outgoing tab
        // back finds nothing in `live_preset_cache` -> falls through to a cold
        // load that has to read from disk + can't replay live bars.  Owner
        // repro 2026-06-26: tab 0 and tab 1 showed the SAME chart because tab
        // 1 was created without parking tab 0 first.
        //
        // park_active_preset is a no-op if the outgoing id is empty / default,
        // and `LoadPreset` will still re-park properly when the user switches.
        // We park unconditionally here because `set_layout_single` below
        // replaces panel_grid in place (resets to one container), so without
        // parking first the outgoing layout / bubbles are lost wholesale.
        let outgoing_id = self.panel_app.active_preset_id.clone();
        if !outgoing_id.is_empty() && outgoing_id != "__default__" {
            self.park_active_preset(&outgoing_id);
            sglog!("[execute_new_chart_with_name] parked outgoing preset '{}'", outgoing_id);
        }

        // After park_active_preset the OUTGOING preset's own bubbles/tags/
        // indicators/alerts/drawings have been extracted out of `self.core`
        // into `live_preset_cache` (scoped extraction, shared-BackendCore
        // arc §5.3 — NOT a whole-manager clear; other presets' bubbles in
        // `self.core` are untouched). The reset below creates a brand-new
        // auto-bubble for the new chart rather than relying on any
        // clear-the-world state, so no `bubble.clear()` etc is needed either
        // way.
        self.panel_app.panel_grid.set_layout_single();
        self.panel_app.panel_grid.reassign_active_chart_id();
        self.panel_app.indicator_overlay_states.clear();
        self.panel_app.leaf_color_tags.clear();
        self.sidebar_data_dirty = true;

        // Auto-create a default sync group for the single window so primitives
        // always go through the grouped path (standalone path is broken).
        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
            if let Some(chart_id) = self.panel_app.panel_grid.container_id_for_leaf(active_leaf) {
                // Auto groups get transparent color — never occupy palette slots.
                // S2.7.1e: read symbol/timeframe via branch_meta (B6);
                // no longer need a shared borrow of the window for these fields.
                if self.panel_app.panel_grid.container_for_leaf(active_leaf).is_none() {
                    eprintln!("[sync_group] new_pane: container_for_leaf({:?}) returned None — skipping auto group", active_leaf);
                    return;
                }
                let group_id = self.create_auto_bubble(
                    [0.0, 0.0, 0.0, 0.0],
                    default_symbol.clone(),
                    default_timeframe.clone(),
                );
                self.attach_chart_to_tag(chart_id, group_id);
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    // Mirrors seed_default_chart: react.rs writes BarsLoaded /
                    // *BarAppended events into `window.current_tag_id`.
                    // Without binding the new group here every incoming bar
                    // landed in the deleted preset's stale bubble id and the
                    // chart stayed empty (PresetNameInput modal then kept
                    // waiting for the chart to "load").
                    window.auto_tag_id = group_id;
                    window.current_tag_id = group_id;
                }
                self.core.bubbles_mut().set_bid_instrument_full(
                    group_id,
                    default_symbol.clone(),
                    digdigdig3::ExchangeId::Binance.as_str().to_string(),
                    "S".to_string(),
                    default_timeframe.clone(),
                );
                // Style store lives on the bubble itself (`DataBubble.last_used_style`);
                // symbol/exchange metadata lives on the bubble fields set above.
                // No per-window DM binding needed anymore (bubble is the source of truth).
                let exchange_name = digdigdig3::ExchangeId::Binance.as_str().to_string();
                // Reset window data provider and kick off data flow.
                self.active_exchange = digdigdig3::ExchangeId::Binance;
                let data_provider: mlc_core::SharedDataProvider = std::sync::Arc::new(
                    mlc_backend_data_bridge::LiveDataProvider::new(
                        digdigdig3::ExchangeId::Binance,
                        exchange_name.clone(),
                        digdigdig3::AccountType::Spot,
                        self.core.bridge_arc(),
                    ),
                );
                if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                    window.data_provider = data_provider;
                    // Cold data path: first BarsLoaded must replace-and-snap
                    // (set_bars), and the fresh backfill must snap once more —
                    // same contract as a cold preset restore.
                    window.pending_symbol_load = true;
                    window.needs_snap_after_backfill = true;
                }
                self.core.ensure_connector(digdigdig3::ExchangeId::Binance);
                let bar_count_new = self.core.user().profile().domain.bar_count as usize;
                sglog!(
                    "[execute_new_chart_with_name] calling bridge.request_bars Binance BTCUSDT tf={:?} bars={} force=true",
                    default_timeframe, bar_count_new
                );
                self.core.request_kline(
                    digdigdig3::ExchangeId::Binance,
                    "BTCUSDT",
                    &default_timeframe,
                    digdigdig3::AccountType::Spot,
                    None,
                    Some(bar_count_new),
                    true,
                );
                sglog!(
                    "[execute_new_chart_with_name] Auto-created auto group {:?} chart_id={:?} window.current_tag_id={:?}",
                    group_id, chart_id,
                    self.panel_app.panel_grid.containers().get(&chart_id).map(|w| w.current_tag_id)
                );
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
        // Bound first so the `presets()` read guard drops before the body runs
        // (clippy::significant_drop_in_scrutinee).
        let new_preset = self.core.presets().get(&new_id).cloned();
        if let Some(preset) = new_preset {
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
        use mlc_core::preset::preset::ChartPreset;
        use mlc_core::preset::snapshots::*;

        // Hard guard against the `7a9d1d2` failure mode. Whoever calls this
        // mid cold-hydrate would otherwise snapshot a placeholder grid into
        // `core.presets_mut()` (and from there the next autosave / Upsert
        // drains it to disk). Nothing legitimate calls this during a cold
        // hydrate — refuse and bail.
        if self.load_state.is_loading() {
            eprintln!(
                "[ChartApp] collect_state_into_preset refused for id={} — load_state={:?}",
                id, self.load_state,
            );
            return;
        }

        // Reuse existing metadata if updating an existing slot.
        let (preset_id, preset_name, created_at) =
            if let Some(existing) = self.core.presets().get(id) {
                (existing.id.clone(), existing.name.clone(), existing.created_at)
            } else {
                (id.to_string(), name.to_string(), mlc_core::preset::preset::unix_now_secs())
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
        let snapshot_pairs: Vec<(mlc_core::LeafId, mlc_core::ChartId)> = self.panel_app.panel_grid
            .iter_containers()
            .map(|(lid, w)| (lid, w.id))
            .collect();
        for (leaf_id, chart_id) in snapshot_pairs {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf(leaf_id) {
                let gid = self.bubble_for_chart(chart_id)
                    .map(|g| g.0);
                // Read stash data from the chart's permanent auto-bubble.
                let auto_bid = window.auto_tag_id;
                let (ptids, stashed_ch, stashed_prims_snap) = self.with_bubble(auto_bid, |auto_bub| {
                    let ptids = auto_bub.pre_tag_indicator_ids.clone();
                    // The auto_bubble holds the stashed command_history only while
                    // the chart is IN a color group (joined). When ungrouped the
                    // history lives back on the window; persist a clone of the
                    // current auto_bubble history (may be empty for ungrouped charts).
                    let stashed_ch = if auto_bub.command_history.size() > 0 || auto_bub.command_history.can_redo() {
                        Some(auto_bub.command_history.clone())
                    } else {
                        None
                    };
                    let prims_snap: Vec<mlc_core::preset::snapshots::PrimitiveSnapshot> =
                        auto_bub.primitives.iter()
                            .map(|p| mlc_core::preset::snapshots::PrimitiveSnapshot::from_primitive(p.as_ref()))
                            .collect();
                    (ptids, stashed_ch, prims_snap)
                }).unwrap_or_else(|| (Vec::new(), None, Vec::new()));
                // Phase 4 B: read instrument fields from branch_meta for snapshot.
                let snap_symbol = self.symbol_for_chart(chart_id)
                    .map(String::from).unwrap_or_default();
                let snap_timeframe = self.timeframe_for_chart(chart_id)
                    .unwrap_or_else(mlc_core::state::Timeframe::h1);
                let snap_exchange = self.exchange_for_chart(chart_id)
                    .map(String::from).unwrap_or_default();
                let snap_account_type = self.account_type_for_chart(chart_id)
                    .map(|at| at.to_string())
                    .unwrap_or_else(|| "S".to_string());
                // chart_type/param/interval now live on the bubble, symmetric
                // with timeframe (moved off ChartPanelState).
                let snap_chart_type = self.chart_type_for_chart(chart_id)
                    .unwrap_or("candles").to_string();
                let snap_chart_type_param = self.chart_type_param_for_chart(chart_id)
                    .unwrap_or(0.0);
                let snap_chart_type_param_mode = self.chart_type_param_mode_for_chart(chart_id)
                    .unwrap_or_default();
                let snap_chart_type_interval = self.chart_type_interval_for_chart(chart_id)
                    .unwrap_or("1m").to_string();
                // Phase 4 C: panel state (18 display/series fields) is in panels_store.
                let snap_panel_id = window.chart_panel_id;
                let snap_default_panel = crate::chart_panel_state::ChartPanelState::new(snap_panel_id);
                let snap_panel_state = self.panels_store.chart_panel
                    .get(&snap_panel_id)
                    .unwrap_or(&snap_default_panel);
                let dreg_snap = self.core.drawings();
                let snap_prims = dreg_snap.primitives(window.current_tag_id);
                preset.windows.push(crate::persistence::build_chart_window_snapshot(
                    window, snap_panel_state, snap_prims, leaf_id.0, gid, ptids, stashed_ch, stashed_prims_snap,
                    snap_symbol, snap_timeframe, snap_exchange, snap_account_type,
                    snap_chart_type, snap_chart_type_param, snap_chart_type_param_mode, snap_chart_type_interval,
                ));
            }
        }

        // Snapshot sync groups (with synced_panels cross-reference so each
        // group's persisted synced_panel_ids list survives restore).
        let tags_for_snapshot = self.core.tags();
        let synced_snapshot = tags_for_snapshot.synced_panels_snapshot();
        // Bound first so the `bubbles()` read guard drops before the loop body
        // runs (clippy::significant_drop_in_scrutinee).
        let bubble_snapshots = self.core.bubbles().snapshot_all(&tags_for_snapshot, &synced_snapshot);
        for group_snap in bubble_snapshots {
            preset.bubbles.push(group_snap);
        }

        // Snapshot indicators (sorted by display order so restore preserves sequence).
        {
            let im = self.core.indicators();
            let mut sorted_instances: Vec<_> = im.all_instances();
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
                    bubble_id: inst.bubble_id,
                    origin_id: inst.origin_id,
                    signals_enabled: inst.signals_enabled,
                    timeframe_visibility: inst.timeframe_visibility.clone(),
                    values: (*inst.values).clone(),
                });
            }
        }

        // Snapshot alerts — flatten across all bubbles for the legacy flat-list preset field.
        preset.alerts = self.core.alerts().items_all();

        // Snapshot leaf_color_tags
        preset.leaf_color_tags = self.panel_app.leaf_color_tags
            .iter()
            .map(|(lid, color)| (lid.0, *color))
            .collect();

        // Snapshot per-slot Panel docking layouts.
        // Each entry is a LayoutSnapshot JSON string, or None if the slot is empty.
        // `slot_leaves` carries the per-leaf kind + panel_id for state restoration.
        // A preset loaded from an older save may carry fewer slots — widen up.
        {
            let n = mlc_app_sidebar_content::free_slot::SLOT_COUNT;
            if preset.slot_layouts.len() < n {
                preset.slot_layouts.resize(n, None);
            }
            if preset.slot_leaves.len() < n {
                preset.slot_leaves.resize_with(n, Vec::new);
            }
        }
        for i in 0..mlc_app_sidebar_content::free_slot::SLOT_COUNT {
            let tree = self.sidebar_state.slot_dockings[i].inner().tree();
            preset.slot_layouts[i] = uzor::panels::serialize::LayoutSnapshot::from_tree(tree, "slot")
                .to_json()
                .ok();

            // Collect persisted leaf descriptors.
            let persist_source = |source: &mlc_app_panels::trading::SymbolSource| -> mlc_core::preset::preset::PersistedSymbolSource {
                match source {
                    mlc_app_panels::trading::SymbolSource::HyperFocus => mlc_core::preset::preset::PersistedSymbolSource::HyperFocus,
                    mlc_app_panels::trading::SymbolSource::Fixed { symbol, exchange, account_type } => mlc_core::preset::preset::PersistedSymbolSource::Fixed {
                        symbol: symbol.clone(),
                        exchange: exchange.clone(),
                        account_type: account_type.clone(),
                    },
                    mlc_app_panels::trading::SymbolSource::BoundToChart { leaf_id } => mlc_core::preset::preset::PersistedSymbolSource::BoundToChart {
                        leaf_id: *leaf_id,
                    },
                }
            };
            let leaves_desc: Vec<mlc_core::preset::preset::PersistedFreeLeaf> = tree
                .leaves()
                .into_iter()
                .filter_map(|leaf| {
                    let item = leaf.panels.get(leaf.active_tab)?;
                    let panel_id = item.panel_id().0;
                    use mlc_core::state::Panel;
                    use mlc_core::preset::preset::PersistedFreeItemKind;
                    // Helper: derive a PersistedSymbolSource from a panel's exchange/symbol/account_type
                    // fields (market-data panels no longer carry a SymbolSource field).
                    let persist_source_from_fields = |symbol: &str, exchange: &str, account_type: &str|
                        -> mlc_core::preset::preset::PersistedSymbolSource
                    {
                        if exchange.is_empty() {
                            mlc_core::preset::preset::PersistedSymbolSource::HyperFocus
                        } else {
                            mlc_core::preset::preset::PersistedSymbolSource::Fixed {
                                symbol: symbol.to_string(),
                                exchange: exchange.to_string(),
                                account_type: account_type.to_string(),
                            }
                        }
                    };

                    let kind = match item {
                        Panel::Dom(id) => {
                            let state = self.panels_store.dom.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::Dom {
                                source,
                                tick_size: state.tick_size,
                                levels_displayed: state.levels_displayed,
                                center_price: state.center_price,
                                multi_mode: state.multi_mode,
                                agg_mode: state.agg_mode,
                                multi_exchanges: state.multi.config.exchanges.clone(),
                                multi_grid_size: state.multi.config.grid_size,
                                multi_depth: state.multi.config.depth_per_side,
                                multi_show_imbalance: state.multi.config.show_imbalance,
                                multi_per_column_norm: state.multi.config.per_column_norm,
                                multi_show_depth_bounds: state.multi.config.show_depth_bounds,
                                multi_depth_bounds_pct: state.multi.config.depth_bounds_pct,
                                dom_depth_wave: state.show_depth_wave,
                                dom_big_markers: state.show_big_markers,
                            }
                        }
                        Panel::Footprint(id) => {
                            let state = self.panels_store.footprint.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::Footprint {
                                source,
                                tick_size: state.tick_size,
                            }
                        }
                        Panel::VolumeProfile(id) => {
                            let state = self.panels_store.volume_profile.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::VolumeProfile {
                                source,
                                tick_size: state.tick_size,
                            }
                        }
                        Panel::LiquidityHeatmap(id) => {
                            let state = self.panels_store.liquidity_heatmap.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::LiquidityHeatmap {
                                source,
                                tick_size: state.tick_size,
                                snapshot_interval_ms: state.snapshot_interval_ms,
                            }
                        }
                        Panel::LiqTape(id) => {
                            let state = self.panels_store.liq_tape.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::LiqTape {
                                source,
                                multi_entries: state.multi.entries.clone(),
                                size_threshold: state.size_threshold,
                                notional_threshold: state.notional_threshold,
                                multi_mode: state.multi_mode,
                                agg_mode: state.agg_mode,
                            }
                        }
                        Panel::BigTrades(id) => {
                            let state = self.panels_store.big_trades.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::BigTrades {
                                source,
                                multi_mode: state.multi_mode,
                                multi_entries: state.multi.entries.clone(),
                                agg_mode: state.agg_mode,
                            }
                        }
                        Panel::L2Tape(id) => {
                            let state = self.panels_store.l2_tape.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::L2Tape {
                                source,
                                multi_mode: state.multi_mode,
                                multi_entries: state.multi.entries.clone(),
                                show_imbalance: state.show_imbalance,
                                imb_window_min: state.imb_window_min,
                                show_adds: state.show_adds,
                                show_mods: state.show_mods,
                                show_cxls: state.show_cxls,
                                show_execs: state.show_execs,
                            }
                        }
                        Panel::TradeTape(id) => {
                            let state = self.panels_store.trade_tape.get(id)?;
                            let source = persist_source_from_fields(&state.symbol, &state.exchange, &state.account_type);
                            PersistedFreeItemKind::TradeTape {
                                source,
                                multi_mode: state.multi_mode,
                                multi_entries: state.multi.entries.clone(),
                                show_imbalance: state.show_imbalance,
                                imb_window_min: state.imb_window_min,
                            }
                        }
                        Panel::OrderEntry(id) => {
                            let state = self.panels_store.order_entry.get(id)?;
                            let source = persist_source(&state.source);
                            PersistedFreeItemKind::OrderEntry {
                                source,
                            }
                        }
                        Panel::PositionManager(_) => PersistedFreeItemKind::PositionManager,
                        Panel::TradeLog(_) => PersistedFreeItemKind::TradeLog,
                        Panel::RiskCalculator(_) => PersistedFreeItemKind::RiskCalculator,
                        Panel::TradingContainer(id) => {
                            let state = self.panels_store.trading_container.get(id)?;
                            let source = persist_source(&state.source);
                            use mlc_app_panels::trading::trading::trading_container::BottomTab;
                            let active_tab = match state.active_tab {
                                BottomTab::Positions => mlc_core::preset::preset::PersistedBottomTab::Positions,
                                BottomTab::Orders => mlc_core::preset::preset::PersistedBottomTab::Orders,
                                BottomTab::Trades => mlc_core::preset::preset::PersistedBottomTab::Trades,
                            };
                            PersistedFreeItemKind::TradingContainer {
                                source,
                                tick_size: state.tick_size,
                                market_price: state.market_price,
                                right_ratio: state.right_ratio,
                                bottom_ratio: state.bottom_ratio,
                                right_split: state.right_split,
                                active_tab,
                                timeframe: state.timeframe.clone(),
                                center_kind: state.center_kind.wire_id().to_string(),
                                layout_mode: state.layout_mode.wire_id().to_string(),
                                free_tree: state.free_tree_json(),
                                rc_overlay: state.rc_overlay,
                                focus_bar: state.focus_style_bar,
                            }
                        }
                        Panel::IndicatorPanel(id) => {
                            let state = self.panels_store.indicator_panel.get(id)?;
                            PersistedFreeItemKind::IndicatorPanel {
                                symbol: state.symbol.clone(),
                                exchange: state.exchange.clone(),
                                account_type: state.account_type.clone(),
                                instance_id: state.instance_id,
                                indicator_config_id: state.indicator_config_id,
                                price_min: state.price_min,
                                price_max: state.price_max,
                                auto_scale: state.auto_scale,
                                bar_spacing: state.bar_spacing,
                                view_position: state.view_position,
                                height_ratio: state.height_ratio,
                            }
                        }
                        // Chart persistence is wired in Stage 8 (panels_store
                        // ownership of ChartWindows + ChartWindowSnapshot).
                        // For now skip — chart leaf drops from preset and
                        // is recreated next session.
                        Panel::Chart(_) => return None,
                    };
                    Some(mlc_core::preset::preset::PersistedFreeLeaf {
                        leaf_id: leaf.id.0,
                        panel_id,
                        kind,
                    })
                })
                .collect();
            preset.slot_leaves[i] = leaves_desc;
        }

        self.core.presets_mut().insert(id.to_string(), preset);
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
    state: &mut mlc_app_layout::ui::modal_settings::TagsTabsState,
) -> &mut mlc_core::state::scroll_state::ScrollState {
    use mlc_app_layout::ui::modal_settings::{TagsTabsSidebar, TagsTabsTagsTab};
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
    use mlc_core::i18n::{MenuKey, current_language};
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
