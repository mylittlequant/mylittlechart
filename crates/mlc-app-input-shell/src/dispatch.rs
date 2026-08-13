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

            let (win_w, win_h) = platform.window_size();
            let hit = chrome::hit_test(x, y, win_w as f64, win_h as f64, chrome_st);

            let skeleton_active = chart.panel_app.user_settings_state.show_profile_manager
                || chart.panel_app.user_settings_state.show_welcome_wizard;
            chrome_st.hovered = if skeleton_active {
                match hit {
                    ChromeHit::NewTabButton
                    | ChromeHit::MenuButton
                    | ChromeHit::MascotButton
                    | ChromeHit::NewWindowButton
                    | ChromeHit::Tab(_)
                    | ChromeHit::TabClose(_) => ChromeHit::None,
                    other => other,
                }
            } else {
                hit
            };

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
                | ChromeHit::Tab(_)
                | ChromeHit::TabClose(_)
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

                if chart.panel_app.toolbar_state.open_dropdown_id.is_some()
                    || chart.panel_app.toolbar_state.open_inline_style_dropdown
                    || chart.panel_app.toolbar_state.open_inline_width_dropdown
                    || chart.panel_app.toolbar_state.open_inline_wave_dropdown
                {
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
                let hovered_id = chart.panel_app.toolbar_state.hovered_top_toolbar_id.as_deref()
                    .or(chart.panel_app.toolbar_state.hovered_left_toolbar_id.as_deref())
                    .or(chart.panel_app.toolbar_state.hovered_right_toolbar_id.as_deref())
                    .or(chart.panel_app.toolbar_state.hovered_bottom_toolbar_id.as_deref());
                if let Some(btn_id) = hovered_id {
                    let wid = uzor::WidgetId::from(format!("toolbar:{}", btn_id));
                    toolbar_tooltip.update(Some(wid.clone()), time_ms);
                    if let Some(text) = mlc_core::toolbar::find_toolbar_tooltip(btn_id) {
                        toolbar_tooltip.request_tooltip(wid, text.to_string(), (x, y), time_ms);
                    }
                } else {
                    let mut sidebar_tip = false;
                    if chart.sidebar_state.is_right_open() {
                        let panel   = chart.sidebar_state.right_panel;
                        let is_agents = panel == RightSidebarPanel::Agents;
                        let is_slot = matches!(
                            panel,
                            RightSidebarPanel::SlotTrading
                                | RightSidebarPanel::SlotDom
                                | RightSidebarPanel::SlotTape
                                | RightSidebarPanel::SlotL2
                                | RightSidebarPanel::SlotBigTrades
                        );
                        if is_agents || is_slot {
                            if let Some(ref sr) = chart.last_sidebar_result {
                                for (wid_str, wrect) in &sr.item_rects {
                                    if x >= wrect.x && x < wrect.x + wrect.width
                                        && chart_y >= wrect.y && chart_y < wrect.y + wrect.height
                                    {
                                        let tip_text = if is_agents {
                                            mlc_app_sidebar_content::render::find_agent_tooltip(wid_str)
                                        } else {
                                            mlc_app_sidebar_content::render::find_free_slot_tooltip(wid_str)
                                        };
                                        if let Some(tip_text) = tip_text {
                                            let wid = uzor::WidgetId::from(wid_str.as_str());
                                            toolbar_tooltip.update(Some(wid.clone()), time_ms);
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
                        toolbar_tooltip.update(None, time_ms);
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

            if button == MouseBtn::Left && pressed {
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
                        let skeleton_active = chart.panel_app.user_settings_state.show_profile_manager
                            || chart.panel_app.user_settings_state.show_welcome_wizard;
                        if !skeleton_active {
                            if let Some(tab) = chrome_st.tabs.get(idx) {
                                let tab_id = tab.id.clone();
                                chart.load_preset(&tab_id);
                                dirty.chart_dirty = true;
                                platform.request_redraw();
                            }
                        }
                        return;
                    }
                    ChromeHit::TabClose(idx) => {
                        let skeleton_active = chart.panel_app.user_settings_state.show_profile_manager
                            || chart.panel_app.user_settings_state.show_welcome_wizard;
                        if !skeleton_active {
                            if let Some(tab) = chrome_st.tabs.get(idx).cloned() {
                                chart.close_tab(&tab.id);
                            }
                        }
                        return;
                    }
                    ChromeHit::NewTabButton => {
                        let skeleton_active = chart.panel_app.user_settings_state.show_profile_manager
                            || chart.panel_app.user_settings_state.show_welcome_wizard;
                        if !skeleton_active {
                            let ts = &mut chart.panel_app.toolbar_state;
                            if ts.open_dropdown_id.as_deref() == Some("new_tab_menu") {
                                ts.open_dropdown_id = None;
                                ts.open_dropdown_position = None;
                            } else {
                                let btn_x = chrome::new_tab_button_x(chrome_st);
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
                    ChromeHit::MenuButton => {
                        let skeleton_active = chart.panel_app.user_settings_state.show_profile_manager
                            || chart.panel_app.user_settings_state.show_welcome_wizard;
                        if !skeleton_active {
                            chart.open_user_settings();
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
                        let skeleton_active = chart.panel_app.user_settings_state.show_profile_manager
                            || chart.panel_app.user_settings_state.show_welcome_wizard;
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
            if chart_y >= 0.0 {
                chart.on_scroll(x, chart_y, dx, dy, state.modifiers.ctrl);
                dirty.sidebar_dirty_scene = true;
                dirty.chart_dirty         = true;
            }
        }

        // ─── Modifier keys ────────────────────────────────────────────────
        ShellEvent::ModifiersChanged(mods) => {
            state.modifiers = mods;
        }

        // ─── Keyboard ─────────────────────────────────────────────────────
        ShellEvent::KeyPressed { logical, physical } => {
            dirty.toolbar_dirty       = true;
            dirty.sidebar_dirty_scene = true;
            dirty.chart_dirty         = true;

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
