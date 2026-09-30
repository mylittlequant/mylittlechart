//! process_event — platform-neutral port of mlc-app-shell-vello/src/window/event_handler.rs.
//!
//! Direct transcription: `pw.window.*` → `platform.*`,
//! `pw.chrome_state` → `chrome`, `pw.chart_dirty` → `dirty.chart_dirty`,
//! `pw.mouse_pressed` → `state.mouse_pressed`, etc.

use mlc_core::{
    CursorStyle, TOP_TOOLBAR_HEIGHT, BOTTOM_TOOLBAR_HEIGHT,
    LEFT_TOOLBAR_WIDTH, RIGHT_TOOLBAR_WIDTH,
};
use mlc_app_sidebar_content::state::{MIN_SIDEBAR_WIDTH, RightSidebarPanel};

use crate::chrome::{self, ChromeHit, ChromeMenuAction};
use crate::events::{LogicalKey, MouseBtn, PhysicalKey, ResizeDir, ShellEvent};
use crate::platform::{DispatchOut, Platform};
use crate::state::{DirtyFlags, InputState};
use crate::tooltip::TooltipState;

/// Hover on the share globe or the camera opens that button's menu
/// instead of a tooltip. Click still fires the button's own action.
fn sync_share_hover_menu(chart: &mut mlc_app::ChartApp) {
    if chart.panel_app.share_settings_state.is_open || chart.panel_app.share_open_state.is_open {
        return;
    }
    let tb = &mut chart.panel_app.toolbar_state;
    let hover = tb.hovered_top_toolbar_id.clone();
    let over_menu = tb.hovered_dropdown_item.is_some();
    let open = tb.open_dropdown_id.clone();
    let want = match hover.as_deref() {
        Some("share_chart") | Some("screenshot") => hover,
        _ => None,
    };
    if let Some(id) = want {
        let replace = open.is_none()
            || open.as_deref() == Some("share_chart")
            || open.as_deref() == Some("screenshot");
        if replace && open.as_deref() != Some(id.as_str()) {
            tb.open_dropdown_id = Some(id);
        }
    } else if matches!(open.as_deref(), Some("share_chart") | Some("screenshot")) && !over_menu {
        tb.open_dropdown_id = None;
    }
}

/// Process one platform-neutral window event.
///
/// GPU resize (`render_cx.resize_surface`) is NOT done here — the vello shell
/// calls it before forwarding the `Resized` event.
pub fn process_event(
    state:              &mut InputState,
    chrome_st:          &mut chrome::ChromeState,
    dirty:              &mut DirtyFlags,
    chart:              &mut mlc_app::ChartApp,
    platform:           &dyn Platform,
    event:              ShellEvent,
    toolbar_tooltip:    &mut TooltipState,
    screenshot_pending: &mut bool,
    out:                &mut DispatchOut,
) {
    match event {
        // ─── OS close ─────────────────────────────────────────────────────
        ShellEvent::CloseRequested => {
            out.close_requested = true;
        }

        // ─── Focus gain ───────────────────────────────────────────────────
        ShellEvent::Focused => {
            dirty.chart_dirty = true;
        }

        // ─── Resize ───────────────────────────────────────────────────────
        ShellEvent::Resized { w, h } => {
            if w > 0 && h > 0 {
                let chrome_px = (state.chrome_height * 1.0) as u32; // logical: chrome_height already logical
                chart.resize(w, h.saturating_sub(chrome_px));

                // Preventive sidebar guard.
                if chart.sidebar_state.is_right_open() {
                    let window_w = chart.width as f64;
                    let right_toolbar_left_x = window_w - RIGHT_TOOLBAR_WIDTH;
                    let min_chart_w = chart.panel_app.panel_grid
                        .min_sidebar_chart_width() as f64;
                    if right_toolbar_left_x < MIN_SIDEBAR_WIDTH + min_chart_w {
                        chart.sidebar_state.set_right_panel(RightSidebarPanel::None);
                    } else {
                        let max_sidebar = right_toolbar_left_x - min_chart_w;
                        let cur = chart.sidebar_state.right_sidebar_width;
                        if cur > max_sidebar {
                            if max_sidebar < MIN_SIDEBAR_WIDTH {
                                chart.sidebar_state.set_right_panel(RightSidebarPanel::None);
                            } else {
                                chart.sidebar_state.set_right_width(max_sidebar);
                            }
                        }
                    }
                }

                chrome_st.is_maximized = platform.is_maximized();

                if !state.skeleton {
                    chart.profile_geometry_dirty = true;
                }
                dirty.toolbar_dirty       = true;
                dirty.sidebar_dirty_scene = true;
                dirty.chart_dirty         = true;

                // D1 (viewport doctrine audit 2026-07-07): the restore-from-
                // minimize re-snap branch that lived here was gated
                // `!true && ...` — permanently dead. Deleted; `was_minimized`
                // still resets here so the vello shell resumes ticking this
                // window (see `mlc-app-shell-vello/main.rs:4099`).
                state.was_minimized = false;
            } else {
                state.was_minimized = true;
            }
        }

        // ─── Window moved ─────────────────────────────────────────────────
        ShellEvent::Moved => {
            if !state.skeleton {
                chart.profile_geometry_dirty = true;
            }
        }

        // ─── Mouse move ───────────────────────────────────────────────────
        ShellEvent::CursorMoved { x, y } => {
            state.last_mouse_pos = (x, y);

            // Tab drag&drop reorder (chrome strip): armed on tab press,
            // activates past a 4 px threshold; while active it owns the move.
            if let Some(drag) = chrome_st.tab_drag.as_mut() {
                drag.cur_x = x;
                if !drag.active && (x - drag.press_x).abs() > 4.0 {
                    drag.active = true;
                }
                if drag.active {
                    dirty.chart_dirty   = true;
                    dirty.overlay_dirty = true;
                    return;
                }
            }

            // Text-select drags are owned by the store (it tracks the drag
            // itself via `drag_active`). A drag that started on the chrome
            // strip (inline tab rename) never arms `mouse_pressed`, so the
            // chart path below never forwards these moves — do it here
            // whenever the store says a text drag is live.
            if !state.mouse_pressed {
                let mut coord = chart.input_coordinator.borrow_mut();
                if coord.text_fields().drag_active() {
                    coord.text_fields_mut().on_drag_move(x);
                    dirty.chart_dirty = true;
                    return;
                }
            }

            if state.mouse_pressed {
                if let Some((last_x, last_y)) = state.last_drag_pos {
                    let dx = x - last_x;
                    let dy = y - last_y;
                    let chart_y = y - state.chrome_height;
                    // Capture the inline-bar drag flag BEFORE on_drag_move so we
                    // can mark the toolbar dirty on the very same frame the drag
                    // started. Reading it AFTER on_drag_move would still work for
                    // sustained drags, but checking before keeps the path uniform
                    // with other drag flags.
                    let inline_bar_dragging =
                        chart.panel_app.toolbar_state.floating_inline_bar.dragging;
                    chart.on_drag_move(x, chart_y, dx, dy);
                    if chart.is_sidebar_separator_dragging() {
                        dirty.sidebar_dirty_scene = true;
                        dirty.toolbar_dirty = true;
                    }
                    // Floating inline-config bar lives inside the toolbar
                    // sub-scene; without this mark, dragging only updates
                    // bar.x/bar.y in state but reuses the cached toolbar
                    // scene → the bar jitters / lags by N frames until some
                    // other event happens to dirty the toolbar.
                    if inline_bar_dragging
                        || chart.panel_app.toolbar_state.floating_inline_bar.dragging
                    {
                        dirty.toolbar_dirty = true;
                    }
                    dirty.overlay_dirty = true;
                    dirty.chart_dirty   = true;
                }
                state.last_drag_pos = Some((x, y));
                return;
            }

            if chrome_st.context_menu.open {
                chrome::context_menu_hover(&mut chrome_st.context_menu, x, y);
            }
            if chrome_st.tab_list_open {
                let (win_w, _) = platform.window_size();
                chrome::tab_list_hover(chrome_st, x, y, win_w as f64);
            }
            #[cfg(feature = "cloud")]
            if chrome_st.account_menu_open {
                let (win_w, _) = platform.window_size();
                chrome::account_menu_hover(chrome_st, x, y, win_w as f64);
            }

            let (win_w, win_h) = platform.window_size();
            let hit = chrome::hit_test(x, y, win_w as f64, win_h as f64, chrome_st);

            // Hover follows the DIM, not a special case. Any modal that
            // dims the backdrop also dims the chrome strip
            // (`render_backdrop_dim`), and a control painted under a veil
            // must not light up when the cursor crosses it — the owner
            // called this out on 2026-09-02: the dimming is right, the hover
            // leaking through it is not.
            //
            // The system buttons are the deliberate exception, both here and
            // in the painter: `render_backdrop_dim` leaves
            // `SYSTEM_BUTTONS_WIDTH` undimmed (owner's ruling 2026-08-31,
            // minimise/maximise/close stay live while the window is dimmed),
            // so they keep their hover too. Everything the veil covers goes
            // to `ChromeHit::None`.
            //
            // This used to key off `profile_manager_mandatory` — the
            // non-dismissible migration page, which no longer exists (see
            // `mlc-app-shell-vello`'s startup). Keying off the dim itself is
            // what the rule was always trying to say.
            let dimmed = chart.panel_app.dim_backdrop_color().is_some();
            let prev_hovered = chrome_st.hovered;
            chrome_st.hovered = if dimmed && chrome::is_veiled_hit(hit) {
                ChromeHit::None
            } else {
                hit
            };

            // Hover onto the "⋯" overflow button opens the tab list (the
            // transition check keeps a click-closed list from springing back
            // open while the cursor is still parked on the button).
            if chrome_st.hovered == ChromeHit::TabOverflow && prev_hovered != ChromeHit::TabOverflow {
                chrome_st.tab_list_open = true;
            }

            // ...and leaving it closes it again. Opening on hover without a
            // matching close leaves the list parked over the chart until an
            // unrelated click dismisses it. The region it closes outside of
            // carries a slack and reaches the top of the window, so walking
            // from the button into the list never crosses out of it.
            if chrome_st.tab_list_open
                && chrome::tab_list_should_close(chrome_st, x, y, win_w as f64)
            {
                chrome_st.tab_list_open = false;
                chrome_st.tab_list_hovered = -1;
            }

            {
                let time_ms = state.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                chrome::update_tooltip(chrome_st, x, y, time_ms);
            }

            // Hover refresh — every cursor move outside a drag must request a
            // redraw so the chrome (and overlay) repaint with the updated
            // `chrome_st.hovered` value. Pre-carveout `event_handler.rs` set
            // `pw.overlay_dirty = pw.chart_dirty = true` unconditionally inside
            // its `CursorMoved` branch; the carveout (commit f5517a58) ported
            // the state writes here but dropped the dirty marks, so hover
            // highlights on the chrome strip and the crosshair stopped
            // updating until a click forced a frame. Drag path above already
            // sets its own dirty flags — keep this AFTER it, AFTER the early
            // `return` for drags.
            dirty.overlay_dirty = true;
            dirty.chart_dirty   = true;

            match hit {
                ChromeHit::ResizeTop | ChromeHit::ResizeBottom => {
                    platform.set_cursor_visible(true);
                    platform.set_cursor(CursorStyle::NsResize);
                    return;
                }
                ChromeHit::ResizeLeft | ChromeHit::ResizeRight => {
                    platform.set_cursor_visible(true);
                    platform.set_cursor(CursorStyle::EwResize);
                    return;
                }
                ChromeHit::ResizeTopLeft | ChromeHit::ResizeBottomRight => {
                    platform.set_cursor_visible(true);
                    platform.set_cursor(CursorStyle::NwseResize);
                    return;
                }
                ChromeHit::ResizeTopRight | ChromeHit::ResizeBottomLeft => {
                    platform.set_cursor_visible(true);
                    platform.set_cursor(CursorStyle::NeswResize);
                    return;
                }
                ChromeHit::Caption
                | ChromeHit::MinimizeButton
                | ChromeHit::MaximizeButton
                | ChromeHit::CloseButton
                | ChromeHit::CloseWindowButton
                | ChromeHit::MascotButton
                | ChromeHit::MenuButton
                | ChromeHit::LoginButton
                | ChromeHit::NewsButton
                | ChromeHit::DownloadButton
                | ChromeHit::Tab(_)
                | ChromeHit::TabClose(_)
                | ChromeHit::TabRename(_)
                | ChromeHit::TabOverflow
                | ChromeHit::NewTabButton
                | ChromeHit::NewWindowButton => {
                    platform.set_cursor_visible(true);
                    platform.set_cursor(CursorStyle::Default);
                    toolbar_tooltip.clear();
                    return;
                }
                ChromeHit::None => {
                    chrome_st.tooltip.clear();
                }
            }

            let chart_y = y - state.chrome_height;
            if chart_y < 0.0 {
                return;
            }

            dirty.overlay_dirty = true;
            dirty.chart_dirty   = true;

            {
                let chart_w = chart.width as f64;
                let chart_h = chart.height as f64;
                let in_toolbar_zone =
                    chart_y < TOP_TOOLBAR_HEIGHT
                    || chart_y > chart_h - BOTTOM_TOOLBAR_HEIGHT
                    || x < LEFT_TOOLBAR_WIDTH
                    || x > chart_w - RIGHT_TOOLBAR_WIDTH;
                if in_toolbar_zone {
                    dirty.toolbar_dirty = true;
                }

                if chart.panel_app.toolbar_state.has_open_dropdown() {
                    dirty.toolbar_dirty = true;
                }

                if chart.panel_app.toolbar_state.floating_inline_bar.dragging {
                    dirty.toolbar_dirty = true;
                }

                if chart.sidebar_state.is_right_open() {
                    let sidebar_w    = chart.sidebar_state.right_sidebar_width;
                    let sidebar_left  = chart_w - RIGHT_TOOLBAR_WIDTH - sidebar_w;
                    let sidebar_right = chart_w - RIGHT_TOOLBAR_WIDTH;
                    if x >= sidebar_left && x < sidebar_right {
                        const ROW_HEIGHT: f64 = 8.0;
                        let extra_header = match chart.sidebar_state.right_panel {
                            RightSidebarPanel::Watchlist => 23.0,
                            _ => 0.0,
                        };
                        let sidebar_top = state.chrome_height + 40.0 + extra_header;
                        let scroll_offset = chart.sidebar_state
                            .current_right_scroll()
                            .offset;
                        let row_index = (((y - sidebar_top) + scroll_offset) / ROW_HEIGHT)
                            .max(0.0) as usize;
                        if state.last_sidebar_hover_row != Some(row_index) {
                            state.last_sidebar_hover_row = Some(row_index);
                            dirty.sidebar_dirty_scene = true;
                        }
                    } else if state.last_sidebar_hover_row.is_some() {
                        state.last_sidebar_hover_row = None;
                        dirty.sidebar_dirty_scene = true;
                    }
                } else if state.last_sidebar_hover_row.is_some() {
                    state.last_sidebar_hover_row = None;
                }
            }

            if state.mouse_pressed {
                if let Some((last_x, last_y)) = state.last_drag_pos {
                    let dx = x - last_x;
                    let dy = y - last_y;
                    let last_chart_y = last_y - state.chrome_height;
                    let inline_bar_dragging =
                        chart.panel_app.toolbar_state.floating_inline_bar.dragging;
                    chart.on_drag_move(x, chart_y, dx, dy);
                    let _ = last_chart_y;
                    if chart.is_sidebar_separator_dragging() {
                        dirty.sidebar_dirty_scene = true;
                        dirty.toolbar_dirty = true;
                    }
                    // See twin callsite above — toolbar caches the inline-bar
                    // position; must mark it dirty for every drag move so the
                    // bar redraws at 60 Hz instead of jittering once per
                    // unrelated dirty event.
                    if inline_bar_dragging
                        || chart.panel_app.toolbar_state.floating_inline_bar.dragging
                    {
                        dirty.toolbar_dirty = true;
                    }
                }
                state.last_drag_pos = Some((x, y));
            } else {
                chart.on_mouse_move(x, chart_y);

                if chart.check_agent_hover(x, chart_y) {
                    dirty.sidebar_dirty_scene = true;
                }

                let time_ms = state.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                sync_share_hover_menu(chart);
                // A chart-toolbar dropdown open answers the same hover a
                // toolbar-item tooltip would explain — see
                // `ChartToolbarState::has_open_dropdown` doc comment.
                let dropdown_open = chart.panel_app.toolbar_state.has_open_dropdown();
                let modal_category_tip = indicator_modal_category_tooltip(chart, x, chart_y);
                let hovered_id = chart.panel_app.toolbar_state.hovered_top_toolbar_id.as_deref()
                    .or(chart.panel_app.toolbar_state.hovered_left_toolbar_id.as_deref())
                    .or(chart.panel_app.toolbar_state.hovered_right_toolbar_id.as_deref())
                    .or(chart.panel_app.toolbar_state.hovered_bottom_toolbar_id.as_deref());
                if let Some((wid_str, tip_text)) = modal_category_tip {
                    // Indicator-search modal category rail / "Sets" button —
                    // takes priority since the modal sits above the toolbar
                    // and the chart when open.
                    let wid = uzor::WidgetId::from(wid_str);
                    toolbar_tooltip.update(Some(wid.clone()), time_ms, dropdown_open);
                    toolbar_tooltip.request_tooltip(wid, tip_text.to_string(), (x, y), time_ms);
                } else if let Some(btn_id) = hovered_id {
                    if btn_id == "share_chart" || btn_id == "screenshot" {
                        toolbar_tooltip.update(None, time_ms, true);
                    } else {
                        let wid = uzor::WidgetId::from(format!("toolbar:{}", btn_id));
                        toolbar_tooltip.update(Some(wid.clone()), time_ms, dropdown_open);
                        // The camcorder explains its own state: off, connecting,
                        // live, viewing, error.
                        let tip = if btn_id == "share_stream" {
                            Some(chart.share_stream_tooltip())
                        } else {
                            mlc_core::toolbar::find_toolbar_tooltip(btn_id)
                        };
                        if let Some(text) = tip {
                            toolbar_tooltip.request_tooltip(wid, text.to_string(), (x, y), time_ms);
                        }
                    }
                } else {
                    let mut sidebar_tip = false;
                    if chart.sidebar_state.is_right_open() {
                        let panel   = chart.sidebar_state.right_panel;
                        let is_agents = panel == RightSidebarPanel::Agents;
                        let is_watchlist = panel == RightSidebarPanel::Watchlist;
                        let is_slot = matches!(
                            panel,
                            RightSidebarPanel::SlotTrading
                                | RightSidebarPanel::SlotDom
                                | RightSidebarPanel::SlotTape
                                | RightSidebarPanel::SlotL2
                                | RightSidebarPanel::SlotBigTrades
                        );
                        if is_agents || is_slot || is_watchlist {
                            if let Some(ref sr) = chart.last_sidebar_result {
                                for (wid_str, wrect) in &sr.item_rects {
                                    if x >= wrect.x && x < wrect.x + wrect.width
                                        && chart_y >= wrect.y && chart_y < wrect.y + wrect.height
                                    {
                                        let tip_text = if is_agents {
                                            mlc_app_sidebar_content::render::find_agent_tooltip(wid_str)
                                        } else if is_watchlist {
                                            mlc_app_sidebar_content::render::find_watchlist_tooltip(wid_str)
                                        } else {
                                            mlc_app_sidebar_content::render::find_free_slot_tooltip(wid_str)
                                        };
                                        if let Some(tip_text) = tip_text {
                                            let wid = uzor::WidgetId::from(wid_str.as_str());
                                            toolbar_tooltip.update(Some(wid.clone()), time_ms, dropdown_open);
                                            toolbar_tooltip.request_tooltip(wid, tip_text.to_string(), (x, y), time_ms);
                                            sidebar_tip = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !sidebar_tip {
                        toolbar_tooltip.update(None, time_ms, dropdown_open);
                    }
                }

                if chart.is_magnet_snapped() {
                    platform.set_cursor_visible(false);
                } else {
                    platform.set_cursor_visible(true);
                    platform.set_cursor(chart.get_cursor(x, chart_y));
                }
            }
        }

        // ─── Cursor left window ───────────────────────────────────────────
        ShellEvent::CursorLeft => {
            if state.drawing_capture {
                return;
            }
            dirty.toolbar_dirty       = true;
            dirty.sidebar_dirty_scene = true;
            dirty.chart_dirty         = true;
            chrome_st.tooltip.clear();
            toolbar_tooltip.clear();
            chart.on_mouse_leave();
            chart.agent_pty_hover_focused = false;
        }

        // ─── Mouse buttons ────────────────────────────────────────────────
        ShellEvent::MouseInput { button, pressed, x, y } => {
            dirty.toolbar_dirty       = true;
            dirty.sidebar_dirty_scene = true;
            dirty.chart_dirty         = true;

            // The text-field store owns selection drags end-to-end; every
            // left release ends one. Chrome-space releases never reach the
            // chart path (chart_y < 0 drops them), and the chart path
            // forwards this same call — `on_drag_end` is a no-op when no
            // text drag is live, so calling it twice is harmless.
            if button == MouseBtn::Left && !pressed {
                chart.input_coordinator.borrow_mut().text_fields_mut().on_drag_end();

                // Finalize a chrome tab drag&drop: apply the reorder (the
                // drop slot counts resting tabs, so past the dragged tab the
                // post-removal index is one less). An active drag swallows
                // the release — it must not fall into the chart click path.
                if let Some(drag) = chrome_st.tab_drag.take() {
                    if drag.active {
                        let drop_idx = chrome::tab_drop_index(chrome_st, drag.cur_x);
                        let to = if drop_idx > drag.index { drop_idx - 1 } else { drop_idx };
                        if to != drag.index {
                            if let Some(tab) = chrome_st.tabs.get(drag.index) {
                                let id = tab.id.clone();
                                chart.move_tab(&id, to);
                            }
                        }
                        dirty.chart_dirty = true;
                        platform.request_redraw();
                        return;
                    }
                }
            }

            if button == MouseBtn::Left && pressed {
                // Tab-list dropdown (from the "⋯" overflow button): a press
                // on a row activates that tab and scrolls it into view; a
                // press anywhere else just closes the list.
                if chrome_st.tab_list_open {
                    let (win_w, _) = platform.window_size();
                    let sel = chrome::tab_list_hit_test(chrome_st, x, y, win_w as f64);
                    chrome_st.tab_list_open = false;
                    if let Some(idx) = sel {
                        if let Some(tab) = chrome_st.tabs.get(idx) {
                            let tab_id = tab.id.clone();
                            chart.load_preset(&tab_id);
                        }
                        chrome::scroll_tab_into_view(chrome_st, win_w as f64, idx);
                    }
                    return;
                }

                // Account menu under the header avatar: a press on a row does
                // that row; any press closes it. A press on the account button
                // itself only closes (the button is the menu's toggle).
                #[cfg(feature = "cloud")]
                if chrome_st.account_menu_open {
                    let (win_w, win_h) = platform.window_size();
                    let sel = chrome::account_menu_hit_test(chrome_st, x, y, win_w as f64);
                    let on_account_button =
                        chrome::hit_test(x, y, win_w as f64, win_h as f64, chrome_st) == ChromeHit::LoginButton;
                    chrome_st.account_menu_open = false;
                    chrome_st.account_menu_hovered = -1;
                    dirty.chart_dirty = true;
                    platform.request_redraw();
                    match sel {
                        Some(chrome::AccountMenuItem::UpgradePlans) => {
                            chart.open_checkout();
                            return;
                        }
                        Some(chrome::AccountMenuItem::AccountSettings) => {
                            open_account_settings(chart);
                            return;
                        }
                        None if on_account_button => return,
                        None => {}
                    }
                }

                if chrome_st.context_menu.open {
                    if let Some(action) = chrome::context_menu_hit_test(&chrome_st.context_menu, x, y) {
                        chrome_st.context_menu.close();
                        match action {
                            ChromeMenuAction::CloseWindow => {
                                out.close_window_requested = true;
                            }
                            ChromeMenuAction::DeleteWindow => {
                                out.delete_window_requested = true;
                            }
                        }
                        return;
                    } else {
                        chrome_st.context_menu.close();
                        return;
                    }
                }

                let (win_w, win_h) = platform.window_size();
                let hit = chrome::hit_test(x, y, win_w as f64, win_h as f64, chrome_st);

                // A press on chrome the modal veil covers puts the modal
                // away instead of firing the control (owner 2026-09-02).
                // Same `is_veiled_hit` list the hover suppression uses, so
                // "does not light up" and "does not act" can never disagree
                // — before this, a veiled button stayed dark and still fired.
                // The system cluster, the caption drag and the resize edges
                // are outside that list and keep working: a dimmed window
                // must still be movable, resizable and closable.
                if chart.panel_app.dim_backdrop_color().is_some() && chrome::is_veiled_hit(hit) {
                    chart.dismiss_dimmed_modal();
                    return;
                }

                // Inline tab rename routing: a press INSIDE the renaming tab
                // is caret placement in the shared text store (geometry pushed
                // by the render hook every frame; store coords are chart-local,
                // hence the chrome-height shift); a press anywhere else commits
                // the edit first — mirrors the modal flows' click-away commit —
                // and then falls through to whatever the press hit.
                if let Some(rename_idx) = chrome_st.renaming_tab {
                    if hit == ChromeHit::Tab(rename_idx) {
                        // The store hit-tests its own field rects — the same
                        // call the chart-area press path makes for every
                        // registered text field.
                        chart.input_coordinator.borrow_mut().text_fields_mut()
                            .on_drag_start(x, y - state.chrome_height);
                        return;
                    }
                    commit_tab_rename(chart, chrome_st);
                }

                match hit {
                    ChromeHit::Caption => {
                        platform.drag_window();
                        return;
                    }
                    ChromeHit::MinimizeButton => {
                        platform.set_minimized(true);
                        return;
                    }
                    ChromeHit::MaximizeButton => {
                        let maximized = platform.is_maximized();
                        platform.set_maximized(!maximized);
                        chrome_st.is_maximized = !maximized;
                        return;
                    }
                    ChromeHit::CloseButton => {
                        out.close_requested = true;
                        return;
                    }
                    ChromeHit::Tab(idx) => {
                        let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                        if !skeleton_active {
                            // Arm the drag&drop reorder — it activates past a
                            // 4 px move and applies on release (CursorMoved /
                            // the release path above own it from there).
                            let grab_dx = x - chrome::tab_left_x(chrome_st, idx);
                            chrome_st.tab_drag = Some(chrome::TabDrag {
                                index: idx,
                                press_x: x,
                                grab_dx,
                                cur_x: x,
                                active: false,
                            });
                            // Tab double-click (same 400 ms / 5 px rule as the
                            // chart-area path below) opens the rename flow. The
                            // chart-area `last_click` logic is unreachable for
                            // chrome hits because every chrome press returns
                            // early, hence the dedicated `last_tab_click`.
                            let now = web_time::Instant::now();
                            let is_double_click = if let Some((last_time, last_idx, last_x, last_y)) = state.last_tab_click {
                                let elapsed = now.duration_since(last_time).as_millis();
                                let dist = ((x - last_x).powi(2) + (y - last_y).powi(2)).sqrt();
                                last_idx == idx && elapsed < 400 && dist < 5.0
                            } else {
                                false
                            };
                            if is_double_click {
                                state.last_tab_click = None;
                                start_tab_rename(chart, chrome_st, idx);
                            } else {
                                state.last_tab_click = Some((now, idx, x, y));
                                if let Some(tab) = chrome_st.tabs.get(idx) {
                                    let tab_id = tab.id.clone();
                                    chart.load_preset(&tab_id);
                                }
                            }
                            dirty.chart_dirty = true;
                            platform.request_redraw();
                        }
                        return;
                    }
                    ChromeHit::TabRename(idx) => {
                        let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                        if !skeleton_active {
                            open_tab_rename(chart, chrome_st, idx);
                            dirty.chart_dirty = true;
                            platform.request_redraw();
                        }
                        return;
                    }
                    ChromeHit::TabClose(idx) => {
                        let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                        if !skeleton_active {
                            if let Some(tab) = chrome_st.tabs.get(idx).cloned() {
                                chart.close_tab(&tab.id);
                            }
                        }
                        return;
                    }
                    ChromeHit::NewTabButton => {
                        let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                        if !skeleton_active {
                            let already_open = chart.panel_app.toolbar_state
                                .open_dropdown_id.as_deref() == Some("new_tab_menu");
                            if already_open {
                                let ts = &mut chart.panel_app.toolbar_state;
                                ts.open_dropdown_id = None;
                                ts.open_dropdown_position = None;
                            } else {
                                let btn_x = chrome::new_tab_button_x(chrome_st, chart.width as f64);
                                // R23 (owner ruling 2026-09-23): the "+" menu
                                // must not paint on top of a still-open
                                // floating settings modal — same choke point
                                // `mlc-app`'s own dropdown-open paths use.
                                chart.close_modals_for_toolbar_dropdown();
                                let ts = &mut chart.panel_app.toolbar_state;
                                ts.open_dropdown_id = Some("new_tab_menu".to_string());
                                ts.open_dropdown_position = Some((btn_x, 0.0));
                            }
                            eprintln!(
                                "[Chrome] + clicked, new_tab_menu open={}",
                                chart.panel_app.toolbar_state.open_dropdown_id.is_some()
                            );
                        }
                        return;
                    }
                    ChromeHit::TabOverflow => {
                        // Click toggles the list (hover already opens it);
                        // the hover-enter guard keys on the hovered
                        // TRANSITION, so a click-close while parked on the
                        // button stays closed.
                        chrome_st.tab_list_open = !chrome_st.tab_list_open;
                        dirty.chart_dirty = true;
                        platform.request_redraw();
                        return;
                    }
                    ChromeHit::MenuButton => {
                        let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                        if !skeleton_active {
                            chart.open_user_settings();
                        }
                        return;
                    }
                    ChromeHit::LoginButton => {
                        #[cfg(feature = "cloud")]
                        {
                            let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                            if !skeleton_active {
                                if chrome_st.account_label.is_some() {
                                    // Signed in: the header's own account
                                    // button opens the account menu (Upgrade /
                                    // Plans, Account settings) on a shell with
                                    // a checkout driver; without one it opens
                                    // straight onto User Settings' Account
                                    // tab (owner ruling 2026-09-09) — the
                                    // login overlay's own job (choose a
                                    // provider, or watch the sign-in
                                    // complete) is already done. Toggle
                                    // closed if it is already open there, the
                                    // same "click again to close" the
                                    // overlay itself used to give this
                                    // button.
                                    if mlc_app_layout::ui::checkout::entry_points_enabled() {
                                        chrome_st.account_menu_open = true;
                                        chrome_st.account_menu_hovered = -1;
                                    } else {
                                        toggle_account_settings(chart);
                                    }
                                    // The overlay's own "sign out" stays for
                                    // the signed-out→signed-in TRANSITION
                                    // screens only (owner ruling) — once
                                    // already signed in, this button never
                                    // opens it, so close it here rather than
                                    // leaving it stacked behind Settings.
                                    chart.panel_app.cloud_login_state.close();
                                } else {
                                    use mlc_app_layout::ui::modal_settings::CloudLoginPhase;
                                    chart.panel_app.cloud_login_state.toggle();
                                    // Which flow the modal opens into is a
                                    // property of the shell, not of the
                                    // click. Native signs in through a
                                    // loopback redirect and has no
                                    // device-code driver at all, so opening
                                    // on `RequestCode` would sit at
                                    // "Requesting sign-in code…" forever.
                                    if chrome_st.flavor == crate::ChromeFlavor::Native
                                        && chart.panel_app.cloud_login_state.is_open
                                    {
                                        chart.panel_app.cloud_login_state.phase = CloudLoginPhase::ChooseProvider;
                                    }
                                }
                                chart.panel_app.cloud_overlay_state.close();
                                dirty.chart_dirty = true;
                                platform.request_redraw();
                            }
                        }
                        return;
                    }
                    ChromeHit::NewsButton | ChromeHit::DownloadButton => {
                        #[cfg(feature = "cloud")]
                        {
                            use mlc_app_layout::ui::modal_settings::CloudOverlayKind;
                            let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                            if !skeleton_active {
                                let kind = match hit {
                                    ChromeHit::NewsButton => CloudOverlayKind::News,
                                    _ => CloudOverlayKind::Download,
                                };
                                // While an installed update is waiting, this
                                // control is the restart, not a way into the
                                // catalogue. Pressing what says "Restart" must
                                // restart.
                                if hit == ChromeHit::DownloadButton
                                    && chrome_st.update == chrome::ChromeUpdate::Restart
                                {
                                    out.restart_requested = true;
                                    return;
                                }
                                chart.panel_app.cloud_overlay_state.toggle(kind);
                                chart.panel_app.cloud_login_state.close();
                                dirty.chart_dirty = true;
                                platform.request_redraw();
                            }
                        }
                        return;
                    }
                    ChromeHit::CloseWindowButton => {
                        out.close_window_requested = true;
                        return;
                    }
                    ChromeHit::MascotButton => {
                        eprintln!("[Chrome] Mascot clicked — future modal");
                        return;
                    }
                    ChromeHit::NewWindowButton => {
                        let skeleton_active = chart.panel_app.user_settings_state.profile_manager_mandatory;
                        if !skeleton_active {
                            out.spawn_new_window = true;
                        }
                        return;
                    }
                    ChromeHit::ResizeTop => {
                        platform.drag_resize_window(ResizeDir::North);
                        return;
                    }
                    ChromeHit::ResizeBottom => {
                        platform.drag_resize_window(ResizeDir::South);
                        return;
                    }
                    ChromeHit::ResizeLeft => {
                        platform.drag_resize_window(ResizeDir::West);
                        return;
                    }
                    ChromeHit::ResizeRight => {
                        platform.drag_resize_window(ResizeDir::East);
                        return;
                    }
                    ChromeHit::ResizeTopLeft => {
                        platform.drag_resize_window(ResizeDir::NorthWest);
                        return;
                    }
                    ChromeHit::ResizeTopRight => {
                        platform.drag_resize_window(ResizeDir::NorthEast);
                        return;
                    }
                    ChromeHit::ResizeBottomLeft => {
                        platform.drag_resize_window(ResizeDir::SouthWest);
                        return;
                    }
                    ChromeHit::ResizeBottomRight => {
                        platform.drag_resize_window(ResizeDir::SouthEast);
                        return;
                    }
                    ChromeHit::None => {}
                }
            }

            // Right-click on chrome → open context menu
            if button == MouseBtn::Right && pressed && y < state.chrome_height {
                chrome_st.context_menu.open_at(x, y);
                return;
            }

            let chart_y = y - state.chrome_height;
            if chart_y < 0.0 {
                return;
            }

            match (button, pressed) {
                (MouseBtn::Left, true) => {
                    state.mouse_pressed  = true;
                    state.last_drag_pos  = Some((x, y));
                    state.drag_start_pos = Some((x, y));
                    let dismissed = chart.on_drag_start(x, chart_y);
                    if dismissed {
                        chart.on_drag_end(x, chart_y);
                        state.mouse_pressed  = false;
                        state.last_drag_pos  = None;
                        state.drag_start_pos = None;
                    }
                }
                (MouseBtn::Left, false) => {
                    if state.mouse_pressed {
                        if let Some((sx, sy)) = state.drag_start_pos {
                            let dist = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt();
                            if dist < 5.0 {
                                let now = web_time::Instant::now();
                                let is_double_click = if let Some((last_time, last_x, last_y)) = state.last_click {
                                    let elapsed = now.duration_since(last_time).as_millis();
                                    let dist2 = ((x - last_x).powi(2) + (y - last_y).powi(2)).sqrt();
                                    elapsed < 400 && dist2 < 5.0
                                } else {
                                    false
                                };

                                if is_double_click {
                                    state.last_click = None;
                                    let (win_w, win_h) = platform.window_size();
                                    let hit = chrome::hit_test(x, y, win_w as f64, win_h as f64, chrome_st);
                                    if hit == ChromeHit::Caption {
                                        let maximized = platform.is_maximized();
                                        platform.set_maximized(!maximized);
                                        chrome_st.is_maximized = !maximized;
                                    } else {
                                        chart.on_double_click(x, chart_y);
                                    }
                                } else {
                                    state.last_click = Some((now, x, y));
                                    chart.on_click(x, chart_y);
                                    // A context-menu "Copy" click (the
                                    // messenger UI-2b DM message menu) has
                                    // no clipboard access of its own inside
                                    // `ChartApp` — it stages the text here,
                                    // the same "core sets a flag, shell
                                    // performs the platform call" split
                                    // `drain_pending_screenshot` already
                                    // uses.
                                    if let Some(text) = chart.drain_pending_clipboard_text() {
                                        platform.clipboard_write(&text);
                                    }
                                }
                            }
                        }
                        let drag_chart_y = y - state.chrome_height;
                        chart.on_drag_end(x, drag_chart_y.max(0.0));
                    }
                    state.mouse_pressed  = false;
                    state.last_drag_pos  = None;
                    state.drag_start_pos = None;

                    #[cfg(target_os = "windows")]
                    {
                        state.drawing_capture = chart.is_drawing();
                    }
                }
                (MouseBtn::Right, true) => {
                    chart.on_right_click(x, chart_y);
                }
                _ => {}
            }
        }

        // ─── Scroll ───────────────────────────────────────────────────────
        ShellEvent::Scroll { dx, dy } => {
            let (x, y) = state.last_mouse_pos;
            let chart_y = y - state.chrome_height;
            if chart_y < 0.0 {
                // Wheel over the chrome strip scrolls the tab strip
                // horizontally when tabs overflow (chevrons in render.rs
                // signal the hidden tabs).
                let d = if dy.abs() >= dx.abs() { dy } else { dx };
                if d != 0.0 {
                    let w = chart.width as f64;
                    chrome::scroll_tab_strip(chrome_st, w, d);
                    dirty.chart_dirty = true;
                }
                return;
            }
            chart.on_scroll(x, chart_y, dx, dy, state.modifiers.ctrl);
            dirty.sidebar_dirty_scene = true;
            dirty.chart_dirty         = true;
        }

        // ─── Modifier keys ────────────────────────────────────────────────
        ShellEvent::ModifiersChanged(mods) => {
            state.modifiers = mods;
            chart.set_shift_held(mods.shift);
        }

        // ─── Keyboard ─────────────────────────────────────────────────────
        ShellEvent::KeyPressed { logical, physical } => {
            dirty.toolbar_dirty       = true;
            dirty.sidebar_dirty_scene = true;
            dirty.chart_dirty         = true;

            // Inline tab rename owns Enter/Escape while active — commit/cancel
            // in place, no modal. Every other key falls through to the shared
            // text field (chars via `dispatch_text_char`, editing keys via
            // `dispatch_text_key_late`, field id CHROME_TAB_RENAME).
            if chrome_st.renaming_tab.is_some() {
                match logical {
                    LogicalKey::Enter => {
                        commit_tab_rename(chart, chrome_st);
                        return;
                    }
                    LogicalKey::Escape => {
                        cancel_tab_rename(chart, chrome_st);
                        return;
                    }
                    _ => {}
                }
            }

            if state.modifiers.ctrl {
                match physical {
                    PhysicalKey::KeyS => {
                        *screenshot_pending = true;
                        eprintln!("[Screenshot] Capture requested via Ctrl+S");
                        return;
                    }
                    PhysicalKey::KeyA => {
                        chart.on_key_press(mlc_app::KeyPress::SelectAll);
                        return;
                    }
                    PhysicalKey::KeyC => {
                        {
                            let chat_text = chart.chat_selection_text();
                            if let Some((leaf_id, text)) = chat_text {
                                platform.clipboard_write(&text);
                                chart.sidebar_state.agent_chat_selections.remove(&leaf_id);
                                dirty.sidebar_dirty_scene = true;
                                return;
                            }
                        }
                        #[cfg(feature = "cloud")]
                        {
                            let dm_text = chart.dm_selection_text();
                            if let Some(text) = dm_text {
                                platform.clipboard_write(&text);
                                chart.panel_app.community_state.dm.selection = None;
                                return;
                            }
                        }
                        #[cfg(feature = "cloud")]
                        {
                            let forum_text = chart.forum_selection_text();
                            if let Some(text) = forum_text {
                                platform.clipboard_write(&text);
                                chart.panel_app.community_state.forum2.selection = None;
                                return;
                            }
                        }
                        if chart.is_agent_pty_focused() {
                            let sel_text = chart.pty_selection_text();
                            if !sel_text.is_empty() {
                                platform.clipboard_write(&sel_text);
                                chart.clear_pty_selection();
                            } else {
                                chart.on_key_press(mlc_app::KeyPress::CtrlC);
                            }
                            return;
                        }
                        if let Some(text) = chart.on_copy_selection() {
                            platform.clipboard_write(&text);
                        } else {
                            // Nothing text-selectable consumed the key — fall
                            // back to copying the selected drawing primitive.
                            chart.on_key_press(mlc_app::KeyPress::CopyPrimitive);
                        }
                        return;
                    }
                    PhysicalKey::KeyV => {
                        let mut consumed_by_pty = false;
                        if let Some(text) = platform.clipboard_read() {
                            if chart.is_agent_pty_focused() {
                                chart.paste_to_pty(&text);
                                consumed_by_pty = true;
                            } else {
                                chart.on_key_press(mlc_app::KeyPress::Paste(text));
                            }
                        }
                        if !consumed_by_pty {
                            // Primitive paste is independent of the OS
                            // clipboard (in-RAM chart clipboard) — attempt it
                            // whenever the key wasn't consumed by the PTY.
                            // ChartApp-side guards make this a no-op when a
                            // text field is focused or nothing was copied.
                            chart.on_key_press(mlc_app::KeyPress::PastePrimitive);
                        }
                        return;
                    }
                    PhysicalKey::KeyX => {
                        // Cut = copy selection, then delete via Backspace.
                        // uzor's TextFieldStore handles Backspace only through
                        // `on_char('\x08')` (apply_key returns false for the
                        // Backspace KeyPress variant — see uzor store.rs:762),
                        // so we route through `on_char_input`, not
                        // `on_key_press(Backspace)`.
                        //
                        // Mirrors the KeyC fallback above (owner spec
                        // 2026-07-25): when there was no text selection to
                        // cut, fall back to cutting the selected drawing
                        // primitive instead.
                        let mut cut_text = false;
                        if let Some(text) = chart.on_copy_selection() {
                            if !text.is_empty() {
                                platform.clipboard_write(&text);
                                chart.on_char_input('\x08');
                                cut_text = true;
                            }
                        }
                        if !cut_text {
                            chart.on_key_press(mlc_app::KeyPress::CutPrimitive);
                        }
                        return;
                    }
                    PhysicalKey::KeyZ => {
                        if state.modifiers.shift {
                            chart.on_key_press(mlc_app::KeyPress::Redo);
                        } else {
                            chart.on_key_press(mlc_app::KeyPress::Undo);
                        }
                        return;
                    }
                    PhysicalKey::KeyY => {
                        chart.on_key_press(mlc_app::KeyPress::Redo);
                        return;
                    }
                    PhysicalKey::KeyB => {
                        eprintln!("[ChartApp] Ctrl+B: switching to Binance");
                        chart.switch_to_exchange(mlc_app::ExchangeId::Binance);
                        return;
                    }
                    _ => {}
                }
            }

            // PTY-first routing
            if chart.is_agent_pty_focused() {
                let pty_key = match &logical {
                    LogicalKey::Escape    => Some(mlc_app::KeyPress::Escape),
                    LogicalKey::Enter     => Some(mlc_app::KeyPress::Enter),
                    LogicalKey::Tab       => Some(mlc_app::KeyPress::Tab),
                    LogicalKey::Backspace => Some(mlc_app::KeyPress::Backspace),
                    LogicalKey::Delete    => Some(mlc_app::KeyPress::Delete),
                    LogicalKey::ArrowLeft  => Some(mlc_app::KeyPress::ArrowLeft),
                    LogicalKey::ArrowRight => Some(mlc_app::KeyPress::ArrowRight),
                    LogicalKey::ArrowUp    => Some(mlc_app::KeyPress::ArrowUp),
                    LogicalKey::ArrowDown  => Some(mlc_app::KeyPress::ArrowDown),
                    LogicalKey::Home      => Some(mlc_app::KeyPress::Home),
                    LogicalKey::End       => Some(mlc_app::KeyPress::End),
                    LogicalKey::PageUp    => Some(mlc_app::KeyPress::PageUp),
                    LogicalKey::PageDown  => Some(mlc_app::KeyPress::PageDown),
                    _ => None,
                };
                if let Some(k) = pty_key {
                    chart.on_key_press(k);
                    return;
                }
            }

            match &logical {
                LogicalKey::Escape => {
                    chart.on_escape();
                    #[cfg(target_os = "windows")]
                    {
                        state.drawing_capture = false;
                    }
                }
                LogicalKey::Backspace => {
                    chart.on_char_input('\x08');
                    // Primitive-delete fallback (owner spec 2026-07-25):
                    // independent of the char path above. ChartApp-side
                    // guards (text focus / blocking modal) make this a
                    // no-op whenever a text field is actually focused —
                    // uzor's TextFieldStore treats the Backspace KeyPress
                    // variant as a no-op for regular fields (real deletion
                    // happens via the '\x08' char above), so sending both
                    // never double-deletes a character.
                    chart.on_key_press(mlc_app::KeyPress::Backspace);
                }
                LogicalKey::Enter => {
                    chart.on_char_input('\n');
                }
                LogicalKey::Space => {
                    chart.on_char_input(' ');
                }
                LogicalKey::Tab => {
                    chart.on_char_input('\x09');
                }
                LogicalKey::Delete => {
                    chart.on_key_press(mlc_app::KeyPress::Delete);
                }
                LogicalKey::ArrowLeft => {
                    if state.modifiers.shift {
                        chart.on_key_press(mlc_app::KeyPress::ShiftLeft);
                    } else {
                        chart.on_key_press(mlc_app::KeyPress::ArrowLeft);
                    }
                }
                LogicalKey::ArrowRight => {
                    if state.modifiers.shift {
                        chart.on_key_press(mlc_app::KeyPress::ShiftRight);
                    } else {
                        chart.on_key_press(mlc_app::KeyPress::ArrowRight);
                    }
                }
                LogicalKey::Home => {
                    if state.modifiers.shift {
                        chart.on_key_press(mlc_app::KeyPress::ShiftHome);
                    } else {
                        chart.on_key_press(mlc_app::KeyPress::Home);
                    }
                }
                LogicalKey::End => {
                    if state.modifiers.shift {
                        chart.on_key_press(mlc_app::KeyPress::ShiftEnd);
                    } else {
                        chart.on_key_press(mlc_app::KeyPress::End);
                    }
                }
                LogicalKey::Character(text) => {
                    if !state.modifiers.ctrl {
                        for ch in text.chars() {
                            chart.on_char_input(ch);
                        }
                    }
                }
                _ => {}
            }
        }

        // ─── IME commit ───────────────────────────────────────────────────
        ShellEvent::ImeCommit(text) => {
            for ch in text.chars() {
                chart.on_char_input(ch);
            }
        }
    }
}

/// Activate the tab if it is not active yet, then open the preset-rename
/// modal for it. `OpenPresetRename` targets the active preset, hence the
/// `load_preset` first for background tabs.
fn open_tab_rename(chart: &mut mlc_app::ChartApp, chrome_st: &chrome::ChromeState, idx: usize) {
    if let Some(tab) = chrome_st.tabs.get(idx) {
        let tab_id = tab.id.clone();
        if !tab.active {
            chart.load_preset(&tab_id);
        }
        chart.open_preset_rename();
    }
}

/// Start the inline tab rename (double-click flow): seed the shared text
/// field with the tab's current name, focus it and select-all. A rename
/// already running on another tab commits first — one rename at a time.
///
/// The tab is NOT activated: `RenamePreset` targets the preset id directly
/// (and the first click of the double-click pair already activated it).
fn start_tab_rename(chart: &mut mlc_app::ChartApp, chrome_st: &mut chrome::ChromeState, idx: usize) {
    commit_tab_rename(chart, chrome_st);
    let Some(tab) = chrome_st.tabs.get(idx) else { return };
    let seed = tab.name.clone();
    let wid = uzor::WidgetId::from(mlc_app::text_input::CHROME_TAB_RENAME);
    {
        let mut coord = chart.input_coordinator.borrow_mut();
        let store = coord.text_fields_mut();
        store.register(wid.clone(), uzor::input::TextFieldConfig::text());
        store.set_text(&wid, &seed);
        store.begin_edit(&wid);
        store.focus(wid.clone());
        // No select-all: the caret sits at the end (set_text leaves it
        // there); a full-selection highlight only obscures the tab.
    }
    chrome_st.renaming_tab = Some(idx);
}

/// Commit the active inline tab rename: fire `RenamePreset` for the tab's
/// own preset id, then blur + unregister the shared text field and clear
/// the rename state. An empty/whitespace name is dropped without renaming
/// (same guard as the modal's `text_action_preset_name`).
fn commit_tab_rename(chart: &mut mlc_app::ChartApp, chrome_st: &mut chrome::ChromeState) {
    let Some(idx) = chrome_st.renaming_tab.take() else { return };
    let wid = uzor::WidgetId::from(mlc_app::text_input::CHROME_TAB_RENAME);
    let text = chart.input_coordinator.borrow().text_fields().text(&wid).trim().to_string();
    {
        let mut coord = chart.input_coordinator.borrow_mut();
        coord.text_fields_mut().blur();
        coord.text_fields_mut().unregister(&wid);
    }
    if !text.is_empty() {
        if let Some(tab) = chrome_st.tabs.get(idx) {
            let tab_id = tab.id.clone();
            chart.rename_preset(&tab_id, &text);
        }
    }
}

/// Cancel the active inline tab rename WITHOUT committing — Escape's path.
/// Clears state and blurs DIRECTLY, never routes through commit (the same
/// hard invariant as `cancel_inline_prim_text_edit`).
fn cancel_tab_rename(chart: &mut mlc_app::ChartApp, chrome_st: &mut chrome::ChromeState) {
    if chrome_st.renaming_tab.take().is_some() {
        let wid = uzor::WidgetId::from(mlc_app::text_input::CHROME_TAB_RENAME);
        let mut coord = chart.input_coordinator.borrow_mut();
        coord.text_fields_mut().blur();
        coord.text_fields_mut().unregister(&wid);
    }
}

/// Hit-test the indicator-search modal's category rail (and the "Sets"
/// button below it) against the cursor, returning `(widget_id, tooltip_text)`
/// when hovering a tab. Mirrors the right-sidebar tooltip branch above —
/// same cached per-frame hit-test shape (`ModalSearchResult` instead of
/// `RightSidebarResult`). `None` whenever the search overlay isn't the
/// indicator-search variant: `sidebar_rect` is only populated by
/// `render_search_overlay` for `OpenModal::IndicatorSearch` (owner review
/// 2026-09-23, item 2 — the rail was icon-only with no hover text).
fn indicator_modal_category_tooltip(
    chart: &mlc_app::ChartApp,
    x: f64,
    y: f64,
) -> Option<(String, &'static str)> {
    let smr = chart.search_modal_result.as_ref()?;
    let sidebar_rect = smr.sidebar_rect?;
    let in_sidebar = x >= sidebar_rect.x && x < sidebar_rect.x + sidebar_rect.width
        && y >= sidebar_rect.y && y < sidebar_rect.y + sidebar_rect.height;
    if !in_sidebar {
        return None;
    }
    for (idx, rect) in &smr.category_rects {
        if x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height {
            let filter = mlc_core::state::modal_state::IndicatorCategoryFilter::from_index(*idx)?;
            return Some((format!("modal_search:category:{}", idx), filter.tooltip()));
        }
    }
    if let Some(sets_rect) = smr.sets_rect {
        if x >= sets_rect.x && x < sets_rect.x + sets_rect.width
            && y >= sets_rect.y && y < sets_rect.y + sets_rect.height
        {
            return Some((
                "ind_search:sets_tab".to_string(),
                mlc_core::i18n::t_modal(mlc_core::i18n::ModalKey::IndicatorSetsTooltip),
            ));
        }
    }
    None
}

/// Open User Settings on the Account tab (the account menu's "Account
/// settings" row). Never toggles closed: a menu row means "take me there".
#[cfg(feature = "cloud")]
fn open_account_settings(chart: &mut mlc_app::ChartApp) {
    use mlc_app_layout::ui::modal_settings::UserSettingsTab;
    let uss = &mut chart.panel_app.user_settings_state;
    uss.open();
    uss.set_tab(UserSettingsTab::Account);
    chart.panel_app.cloud_login_state.close();
    chart.panel_app.cloud_overlay_state.close();
}

/// Open User Settings on the Account tab, or close it if it is already there.
#[cfg(feature = "cloud")]
fn toggle_account_settings(chart: &mut mlc_app::ChartApp) {
    use mlc_app_layout::ui::modal_settings::UserSettingsTab;
    let uss = &mut chart.panel_app.user_settings_state;
    if uss.is_open && uss.active_tab == UserSettingsTab::Account {
        uss.close();
    } else {
        uss.open();
        uss.set_tab(UserSettingsTab::Account);
    }
}
