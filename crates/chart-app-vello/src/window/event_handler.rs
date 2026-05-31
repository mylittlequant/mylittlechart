//! ApplicationHandler::window_event — OS window event → App dispatch.

use crate::{chrome, cursor_style_to_winit, screenshot, App};
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    window::{CursorIcon, WindowId},
};

impl App<'_> {
    pub(crate) fn handle_window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        // OS-level close (Alt+F4, taskbar close) → shutdown entire app.
        if let WindowEvent::CloseRequested = event {
            if let Some(pw) = self.windows.get_mut(&id) {
                pw.close_requested = true; // triggers app shutdown in about_to_wait
            }
            return;
        }

        // Track focus before borrowing per-window state — updating self.last_focused
        // while holding a &mut to self.windows would be a borrow conflict.
        if let WindowEvent::Focused(true) = event {
            self.last_focused = Some(id);
        }

        // Resize touches pw.surface which the GPU render thread may be using.
        // Wait for the current GPU frame to finish before proceeding so we
        // don't race with submit_window_gpu_from_gpu_scene.
        if let WindowEvent::Resized(_) = event {
            self.wait_for_gpu_frame();
        }

        // All other events need the per-window state
        let Some(pw) = self.windows.get_mut(&id) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => unreachable!(), // handled above

            // ─── Resize ───────────────────────────────────────────────────
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    self.render_cx
                        .resize_surface(&mut pw.surface, size.width, size.height);
                    // resize_surface recreates target_texture without COPY_SRC;
                    // patch it again so screenshots continue to work after resize.
                    let device = &self.render_cx.devices[pw.surface.dev_id].device;
                    screenshot::add_copy_src_to_target_texture(&mut pw.surface, device);
                    let chrome_px = (chrome::CHROME_HEIGHT * pw.window.scale_factor()) as u32;
                    pw.chart
                        .resize(size.width, size.height.saturating_sub(chrome_px));

                    // Preventive sidebar guard: on resize, ensure the sidebar
                    // doesn't push the chart area below its minimum.
                    if pw.chart.sidebar_state.is_right_open() {
                        use zengeld_chart::RIGHT_TOOLBAR_WIDTH;
                        use sidebar_content::state::{MIN_SIDEBAR_WIDTH, RightSidebarPanel};
                        let window_w = pw.chart.width as f64;
                        let right_toolbar_left_x = window_w - RIGHT_TOOLBAR_WIDTH;
                        let min_chart_w = pw.chart.panel_app.panel_grid
                            .min_sidebar_chart_width() as f64;
                        if right_toolbar_left_x < MIN_SIDEBAR_WIDTH + min_chart_w {
                            // Window too narrow for sidebar + chart — close sidebar.
                            pw.chart.sidebar_state
                                .set_right_panel(RightSidebarPanel::None);
                        } else {
                            // Clamp sidebar width so chart area stays >= min_chart_w.
                            let max_sidebar = right_toolbar_left_x - min_chart_w;
                            let cur = pw.chart.sidebar_state.right_sidebar_width;
                            if cur > max_sidebar {
                                if max_sidebar < MIN_SIDEBAR_WIDTH {
                                    pw.chart.sidebar_state
                                        .set_right_panel(RightSidebarPanel::None);
                                } else {
                                    pw.chart.sidebar_state.set_right_width(max_sidebar);
                                }
                            }
                        }
                    }

                    // Sync the maximize icon when the window is snapped or
                    // maximized by the OS (e.g. via Win+Arrow keys).
                    pw.chrome_state.is_maximized = pw.window.is_maximized();

                    // Mark dirty so position/size is persisted on next save
                    // (skip skeleton — it's a loading screen, nothing to persist).
                    if !pw.skeleton {
                        pw.chart.profile_geometry_dirty = true;
                    }
                    // Toolbar and sidebar layout changes on resize — must rebuild both.
                    pw.toolbar_dirty = true;
                    pw.sidebar_dirty_scene = true;
                    pw.chart_dirty = true;

                    // Restore from minimize: tick was skipped while minimized,
                    // so viewport stayed at the pre-minimize position.  For
                    // Follow/Auto modes, snap ALL windows to end so the chart
                    // shows the latest bars.  Manual mode keeps user's position.
                    if pw.was_minimized {
                        pw.was_minimized = false;
                        for window in pw.chart.panel_app.panel_grid.windows_mut().values_mut() {
                            if !window.bars.is_empty()
                                && (window.price_scale.scale_mode.is_follow()
                                    || window.price_scale.scale_mode == zengeld_chart::ScaleMode::Auto)
                            {
                                window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                            }
                        }
                    }
                } else {
                    // Window minimized — size collapses to 0x0 on Windows.
                    pw.was_minimized = true;
                }
            }

            // ─── Window moved ─────────────────────────────────────────────
            WindowEvent::Moved(_) => {
                if !pw.skeleton {
                    pw.chart.profile_geometry_dirty = true;
                }
            }

            // ─── Mouse move ───────────────────────────────────────────────
            WindowEvent::CursorMoved { position, .. } => {
                let x = position.x;
                let y = position.y;
                pw.last_mouse_pos = (x, y);

                // Update context menu hover
                if pw.chrome_state.context_menu.open {
                    chrome::context_menu_hover(&mut pw.chrome_state.context_menu, x, y);
                    // Don't return — let other hover logic run too, it's harmless
                }

                // Update chrome hover state and handle chrome-area cursor/redraw.
                let size = pw.window.inner_size();
                let hit =
                    chrome::hit_test(x, y, size.width as f64, size.height as f64, &pw.chrome_state);
                // In skeleton mode, suppress hover for chrome buttons that are blocked.
                let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                    || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                pw.chrome_state.hovered = if skeleton_active {
                    match hit {
                        chrome::ChromeHit::NewTabButton
                        | chrome::ChromeHit::MenuButton
                        | chrome::ChromeHit::MascotButton
                        | chrome::ChromeHit::NewWindowButton
                        | chrome::ChromeHit::Tab(_)
                        | chrome::ChromeHit::TabClose(_) => chrome::ChromeHit::None,
                        other => other,
                    }
                } else {
                    hit
                };

                // Update chrome tooltip based on the (possibly skeleton-filtered) hover.
                {
                    let time_ms = pw.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                    chrome::update_tooltip(&mut pw.chrome_state, x, y, time_ms);
                }

                match hit {
                    chrome::ChromeHit::ResizeTop | chrome::ChromeHit::ResizeBottom => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::NsResize);
                        return;
                    }
                    chrome::ChromeHit::ResizeLeft | chrome::ChromeHit::ResizeRight => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::EwResize);
                        return;
                    }
                    chrome::ChromeHit::ResizeTopLeft | chrome::ChromeHit::ResizeBottomRight => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::NwseResize);
                        return;
                    }
                    chrome::ChromeHit::ResizeTopRight | chrome::ChromeHit::ResizeBottomLeft => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::NeswResize);
                        return;
                    }
                    chrome::ChromeHit::Caption
                    | chrome::ChromeHit::MinimizeButton
                    | chrome::ChromeHit::MaximizeButton
                    | chrome::ChromeHit::CloseButton
                    | chrome::ChromeHit::CloseWindowButton
                    | chrome::ChromeHit::MascotButton
                    | chrome::ChromeHit::MenuButton
                    | chrome::ChromeHit::Tab(_)
                    | chrome::ChromeHit::TabClose(_)
                    | chrome::ChromeHit::NewTabButton
                    | chrome::ChromeHit::NewWindowButton => {
                        pw.window.set_cursor_visible(true);
                        pw.window.set_cursor(CursorIcon::Default);
                        // Cursor is on chrome — clear toolbar tooltip
                        pw.toolbar_tooltip.clear();
                        // Do not forward to chart
                        return;
                    }
                    chrome::ChromeHit::None => {
                        // Cursor is below the chrome strip — clear tooltip.
                        pw.chrome_state.tooltip.clear();
                    }
                }

                // Only forward events in the chart area (below chrome strip).
                let chart_y = y - chrome::CHROME_HEIGHT;
                if chart_y < 0.0 {
                    return;
                }

                // Synchronous on_mouse_move — cheap because:
                //   • crosshair is in overlay_scene (no chart_scene rebuild for move)
                //   • build_extended_layout geometry is cached
                //   • MA indicators are deleted
                // chart_dirty=true is required so begin_frame/end_frame run and
                // hovered_widget() is fresh, keeping hover highlights correct.
                pw.overlay_dirty = true;
                pw.chart_dirty = true;

                // Mark toolbar dirty when the cursor enters or moves within a
                // toolbar band (top/bottom/left/right).  This covers hover-state
                // changes on toolbar buttons without rebuilding on every chart pan.
                {
                    use zengeld_chart::{TOP_TOOLBAR_HEIGHT, BOTTOM_TOOLBAR_HEIGHT,
                                       LEFT_TOOLBAR_WIDTH, RIGHT_TOOLBAR_WIDTH};
                    let chart_w = pw.chart.width as f64;
                    let chart_h = pw.chart.height as f64;
                    let in_toolbar_zone =
                        chart_y < TOP_TOOLBAR_HEIGHT
                        || chart_y > chart_h - BOTTOM_TOOLBAR_HEIGHT
                        || x < LEFT_TOOLBAR_WIDTH
                        || x > chart_w - RIGHT_TOOLBAR_WIDTH;
                    if in_toolbar_zone {
                        pw.toolbar_dirty = true;
                    }
                    // Rebuild the toolbar scene whenever the hovered button actually
                    // CHANGES (enter / leave / switch), not just while inside the
                    // zone. on_mouse_move (above) already recomputed the hovered
                    // toolbar id for this move; if it differs from what the cached
                    // toolbar scene was last drawn with, force a rebuild. This clears
                    // a stuck highlight on a fast exit into the chart (the zone check
                    // alone never fires on the way out) and — because it rebuilds
                    // ONLY on a real change — does not flicker.
                    {
                        let cur_hover = pw.chart.hovered_toolbar_id_snapshot();
                        if cur_hover != pw.last_drawn_toolbar_hover {
                            pw.toolbar_dirty = true;
                            pw.last_drawn_toolbar_hover = cur_hover;
                        }
                    }

                    // Dropdowns extend below the toolbar zone — always rebuild toolbar
                    // scene when any dropdown is open so hover highlights update.
                    if pw.chart.panel_app.toolbar_state.open_dropdown_id.is_some()
                        || pw.chart.panel_app.toolbar_state.open_inline_style_dropdown
                        || pw.chart.panel_app.toolbar_state.open_inline_width_dropdown
                    {
                        pw.toolbar_dirty = true;
                    }

                    // Inline bar dragging moves the toolbar — always rebuild while drag is active.
                    if pw.chart.panel_app.toolbar_state.floating_inline_bar.dragging {
                        pw.toolbar_dirty = true;
                    }

                    // Mark sidebar dirty only when the hovered row changes, not on
                    // every sub-pixel cursor movement within the same row.
                    // Row height is 36 px; the scroll offset shifts which row is
                    // visible so we incorporate it into the calculation.
                    if pw.chart.sidebar_state.is_right_open() {
                        let sidebar_w = pw.chart.sidebar_state.right_sidebar_width;
                        let sidebar_left = chart_w - RIGHT_TOOLBAR_WIDTH - sidebar_w;
                        let sidebar_right = chart_w - RIGHT_TOOLBAR_WIDTH;
                        if x >= sidebar_left && x < sidebar_right {
                            const ROW_HEIGHT: f64 = 8.0;
                            // Content area starts after sidebar header (40 px). The watchlist
                            // panel adds an extra 23 px column header; other panels do not.
                            let extra_header = match pw.chart.sidebar_state.right_panel {
                                sidebar_content::state::RightSidebarPanel::Watchlist => 23.0,
                                _ => 0.0,
                            };
                            let sidebar_top = chrome::CHROME_HEIGHT + 40.0 + extra_header;
                            let scroll_offset = pw.chart.sidebar_state
                                .current_right_scroll()
                                .offset;
                            let row_index = (((y - sidebar_top) + scroll_offset) / ROW_HEIGHT)
                                .max(0.0) as usize;
                            if pw.last_sidebar_hover_row != Some(row_index) {
                                pw.last_sidebar_hover_row = Some(row_index);
                                pw.sidebar_dirty_scene = true;
                            }
                        } else if pw.last_sidebar_hover_row.is_some() {
                            // Cursor left sidebar bounds — clear hover and redraw once.
                            pw.last_sidebar_hover_row = None;
                            pw.sidebar_dirty_scene = true;
                        }
                    } else if pw.last_sidebar_hover_row.is_some() {
                        pw.last_sidebar_hover_row = None;
                    }
                }

                if pw.mouse_pressed {
                    if let Some((last_x, last_y)) = pw.last_drag_pos {
                        let dx = x - last_x;
                        let dy = y - last_y;
                        let last_chart_y = last_y - chrome::CHROME_HEIGHT;
                        pw.chart.on_drag_move(x, chart_y, dx, dy);
                        let _ = last_chart_y; // suppress unused warning
                        // Sidebar separator drag: mark sidebar + toolbar dirty so
                        // the cached scenes rebuild every frame during resize.
                        if pw.chart.is_sidebar_separator_dragging() {
                            pw.sidebar_dirty_scene = true;
                            pw.toolbar_dirty = true;
                        }
                    }
                    pw.last_drag_pos = Some((x, y));
                } else {
                    // Full synchronous hover — cheap with cached geometry + overlay split.
                    pw.chart.on_mouse_move(x, chart_y);

                    // Auto-focus agent PTY terminal on hover.
                    if pw.chart.check_agent_hover(x, chart_y) {
                        pw.sidebar_dirty_scene = true;
                    }

                    // Update toolbar tooltip based on hovered toolbar button.
                    let time_ms = pw.chrome_tooltip_start.elapsed().as_secs_f64() * 1000.0;
                    let hovered_id = pw.chart.panel_app.toolbar_state.hovered_top_toolbar_id.as_deref()
                        .or(pw.chart.panel_app.toolbar_state.hovered_left_toolbar_id.as_deref())
                        .or(pw.chart.panel_app.toolbar_state.hovered_right_toolbar_id.as_deref())
                        .or(pw.chart.panel_app.toolbar_state.hovered_bottom_toolbar_id.as_deref());
                    if let Some(btn_id) = hovered_id {
                        let wid = uzor::WidgetId::from(format!("toolbar:{}", btn_id));
                        pw.toolbar_tooltip.update(Some(wid.clone()), time_ms);
                        if let Some(text) = zengeld_chart::toolbar::find_toolbar_tooltip(btn_id) {
                            pw.toolbar_tooltip.request_tooltip(wid, text.to_string(), (x, y), time_ms);
                        }
                    } else {
                        // No toolbar button hovered — check sidebar buttons (agents + free slots).
                        let mut sidebar_tip = false;
                        if pw.chart.sidebar_state.is_right_open() {
                            use sidebar_content::state::RightSidebarPanel;
                            let panel = pw.chart.sidebar_state.right_panel;
                            let is_agents = panel == RightSidebarPanel::Agents;
                            let is_slot = matches!(
                                panel,
                                RightSidebarPanel::Slot1
                                    | RightSidebarPanel::Slot2
                                    | RightSidebarPanel::Slot3
                                    | RightSidebarPanel::Slot4
                            );
                            if is_agents || is_slot {
                                if let Some(ref sr) = pw.chart.last_sidebar_result {
                                    for (wid_str, wrect) in &sr.item_rects {
                                        // item_rects are in chart-space (rendered with
                                        // translate(0, CHROME_HEIGHT)), so compare
                                        // against chart_y, not window y.
                                        if x >= wrect.x && x < wrect.x + wrect.width
                                            && chart_y >= wrect.y && chart_y < wrect.y + wrect.height
                                        {
                                            let tip_text = if is_agents {
                                                sidebar_content::render::find_agent_tooltip(wid_str)
                                            } else {
                                                sidebar_content::render::find_free_slot_tooltip(wid_str)
                                            };
                                            if let Some(tip_text) = tip_text {
                                                let wid = uzor::WidgetId::from(wid_str.as_str());
                                                pw.toolbar_tooltip.update(Some(wid.clone()), time_ms);
                                                // Tooltip renders in window-space (no
                                                // translate), so pass window y for position.
                                                pw.toolbar_tooltip.request_tooltip(wid, tip_text.to_string(), (x, y), time_ms);
                                                sidebar_tip = true;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !sidebar_tip {
                            pw.toolbar_tooltip.update(None, time_ms);
                        }
                    }

                    // Cursor style — computed from now-fresh hover state.
                    if pw.chart.is_magnet_snapped() {
                        pw.window.set_cursor_visible(false);
                    } else {
                        pw.window.set_cursor_visible(true);
                        pw.window
                            .set_cursor(cursor_style_to_winit(pw.chart.get_cursor(x, chart_y)));
                    }
                }
            }

            // ─── Cursor left window ───────────────────────────────────────
            WindowEvent::CursorLeft { .. } => {
                if pw.drawing_capture {
                    return;
                }
                // Hover state clears when cursor leaves — toolbar, sidebar and chart must redraw.
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;
                pw.chrome_state.tooltip.clear();
                pw.toolbar_tooltip.clear();
                pw.chart.on_mouse_leave();
                // Clear agent PTY hover focus when cursor leaves the window.
                pw.chart.agent_pty_hover_focused = false;
            }

            // ─── Mouse buttons ────────────────────────────────────────────
            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = pw.last_mouse_pos;

                // Any click may change toolbar button state (active drawing mode,
                // open dropdown, etc.) so mark the toolbar as dirty.
                // A click in the sidebar area also changes sidebar state (item
                // selection, delete, settings open, scroll) — always mark it dirty.
                // A click in the chart area can create/select drawings — mark chart dirty.
                pw.toolbar_dirty = true;
                pw.sidebar_dirty_scene = true;
                pw.chart_dirty = true;

                // Check chrome hit first for left-button press events.
                if button == MouseButton::Left && state == ElementState::Pressed {
                    // If context menu is open, handle click on it first
                    if pw.chrome_state.context_menu.open {
                        if let Some(action) = chrome::context_menu_hit_test(&pw.chrome_state.context_menu, x, y) {
                            pw.chrome_state.context_menu.close();
                            match action {
                                chrome::ChromeMenuAction::CloseWindow => {
                                    pw.close_window_requested = true;
                                }
                                chrome::ChromeMenuAction::DeleteWindow => {
                                    pw.delete_window_requested = true;
                                }
                            }
                            return;
                        } else {
                            // Clicked outside menu → close it
                            pw.chrome_state.context_menu.close();
                            return;
                        }
                    }

                    let size = pw.window.inner_size();
                    let hit = chrome::hit_test(
                        x,
                        y,
                        size.width as f64,
                        size.height as f64,
                        &pw.chrome_state,
                    );
                    match hit {
                        chrome::ChromeHit::Caption => {
                            let _ = pw.window.drag_window();
                            return;
                        }
                        chrome::ChromeHit::MinimizeButton => {
                            pw.window.set_minimized(true);
                            return;
                        }
                        chrome::ChromeHit::MaximizeButton => {
                            let maximized = pw.window.is_maximized();
                            pw.window.set_maximized(!maximized);
                            pw.chrome_state.is_maximized = !maximized;
                            return;
                        }
                        chrome::ChromeHit::CloseButton => {
                            // Chrome X = shutdown entire app (save all + exit in about_to_wait)
                            pw.close_requested = true;
                            return;
                        }
                        chrome::ChromeHit::Tab(idx) => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                if let Some(tab) = pw.chrome_state.tabs.get(idx) {
                                    let tab_id = tab.id.clone();
                                    pw.chart.load_preset(&tab_id);
                                }
                            }
                            return;
                        }
                        chrome::ChromeHit::TabClose(idx) => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                // Close the tab without deleting the preset.
                                // CloseTab handler in input.rs will switch to an adjacent tab automatically.
                                if let Some(tab) = pw.chrome_state.tabs.get(idx).cloned() {
                                    pw.chart.close_tab(&tab.id);
                                }
                            }
                            return;
                        }
                        chrome::ChromeHit::NewTabButton => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                // Toggle the new_tab_menu dropdown (uses the chart toolbar dropdown system).
                                let ts = &mut pw.chart.panel_app.toolbar_state;
                                if ts.open_dropdown_id.as_deref() == Some("new_tab_menu") {
                                    ts.open_dropdown_id = None;
                                    ts.open_dropdown_position = None;
                                } else {
                                    let btn_x = chrome::new_tab_button_x(&pw.chrome_state);
                                    // y=0 in chart-space (chart renders offset by CHROME_HEIGHT,
                                    // so 0 here = right below the chrome strip on screen).
                                    ts.open_dropdown_id = Some("new_tab_menu".to_string());
                                    ts.open_dropdown_position = Some((btn_x, 0.0));
                                }
                                eprintln!(
                                    "[Chrome] + clicked, new_tab_menu open={}",
                                    pw.chart.panel_app.toolbar_state.open_dropdown_id.is_some()
                                );
                            }
                            return;
                        }
                        chrome::ChromeHit::MenuButton => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                pw.chart.open_user_settings();
                            }
                            return;
                        }
                        chrome::ChromeHit::CloseWindowButton => {
                            pw.close_window_requested = true;
                            return;
                        }
                        chrome::ChromeHit::MascotButton => {
                            eprintln!("[Chrome] Mascot clicked — future modal");
                            return;
                        }
                        chrome::ChromeHit::NewWindowButton => {
                            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                            if !skeleton_active {
                                // Queue a new window spawn; it will be created in about_to_wait
                                // once pw is no longer borrowed.
                                pw.spawn_new_window = true;
                            }
                            return;
                        }
                        chrome::ChromeHit::ResizeTop => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::North,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeBottom => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::South,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeLeft => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::West,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeRight => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::East,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeTopLeft => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::NorthWest,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeTopRight => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::NorthEast,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeBottomLeft => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::SouthWest,
                            );
                            return;
                        }
                        chrome::ChromeHit::ResizeBottomRight => {
                            let _ = pw.window.drag_resize_window(
                                winit::window::ResizeDirection::SouthEast,
                            );
                            return;
                        }
                        chrome::ChromeHit::None => {}
                    }
                }

                // Right-click on chrome → open context menu
                if button == MouseButton::Right && state == ElementState::Pressed
                    && y < chrome::CHROME_HEIGHT {
                        pw.chrome_state.context_menu.open_at(x, y);
                        return;
                    }

                // Only forward to chart when in the chart area (below chrome).
                let chart_y = y - chrome::CHROME_HEIGHT;
                if chart_y < 0.0 {
                    return;
                }

                match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        pw.mouse_pressed = true;
                        pw.last_drag_pos = Some((x, y));
                        pw.drag_start_pos = Some((x, y));
                        let dismissed = pw.chart.on_drag_start(x, chart_y);
                        if dismissed {
                            // Popup was dismissed — synthetic drag-end cleans up
                            // ui_drag_active, drag_dismissed_popup, text_input state.
                            pw.chart.on_drag_end(x, chart_y);
                            pw.mouse_pressed = false;
                            pw.last_drag_pos = None;
                            pw.drag_start_pos = None;
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        if pw.mouse_pressed {
                            // Detect click vs drag (threshold: 5 pixels)
                            if let Some((sx, sy)) = pw.drag_start_pos {
                                let dist = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt();
                                if dist < 5.0 {
                                    // Double-click detection (400 ms, 5 px)
                                    let now = std::time::Instant::now();
                                    let is_double_click = if let Some((last_time, last_x, last_y)) =
                                        pw.last_click
                                    {
                                        let elapsed =
                                            now.duration_since(last_time).as_millis();
                                        let dist2 = ((x - last_x).powi(2)
                                            + (y - last_y).powi(2))
                                        .sqrt();
                                        elapsed < 400 && dist2 < 5.0
                                    } else {
                                        false
                                    };

                                    if is_double_click {
                                        pw.last_click = None;
                                        // Check if double-click is on caption — toggle maximize
                                        let size = pw.window.inner_size();
                                        let hit = chrome::hit_test(
                                            x,
                                            y,
                                            size.width as f64,
                                            size.height as f64,
                                            &pw.chrome_state,
                                        );
                                        if hit == chrome::ChromeHit::Caption {
                                            let maximized = pw.window.is_maximized();
                                            pw.window.set_maximized(!maximized);
                                            pw.chrome_state.is_maximized = !maximized;
                                        } else {
                                            pw.chart.on_double_click(x, chart_y);
                                        }
                                    } else {
                                        pw.last_click = Some((now, x, y));
                                        pw.chart.on_click(x, chart_y);
                                    }
                                }
                            }
                            let drag_chart_y = y - chrome::CHROME_HEIGHT;
                            pw.chart.on_drag_end(x, drag_chart_y.max(0.0));
                        }
                        pw.mouse_pressed = false;
                        pw.last_drag_pos = None;
                        pw.drag_start_pos = None;

                        // Track whether the chart is mid-drawing so about_to_wait
                        // can poll GetCursorPos outside the window boundary.
                        #[cfg(target_os = "windows")]
                        {
                            pw.drawing_capture = pw.chart.is_drawing();
                        }
                    }
                    (MouseButton::Right, ElementState::Pressed) => {
                        pw.chart.on_right_click(x, chart_y);
                    }
                    _ => {}
                }
            }

            // ─── Scroll ───────────────────────────────────────────────────
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (x as f64 * 20.0, y as f64 * 20.0)
                    }
                    winit::event::MouseScrollDelta::PixelDelta(pos) => (pos.x, pos.y),
                };
                let (x, y) = pw.last_mouse_pos;
                // Only scroll in the chart area; apply y-offset.
                let chart_y = y - chrome::CHROME_HEIGHT;
                if chart_y >= 0.0 {
                    pw.chart.on_scroll(x, chart_y, dx, dy, pw.modifiers.control_key());
                    // Scrolling inside the sidebar changes the visible content —
                    // always mark it dirty so the new scroll offset is rendered.
                    pw.sidebar_dirty_scene = true;
                    // Scrolling pans/zooms the chart — bars shift, price scale updates.
                    pw.chart_dirty = true;
                }
            }

            // ─── Modifier keys ────────────────────────────────────────────
            WindowEvent::ModifiersChanged(new_modifiers) => {
                pw.modifiers = new_modifiers.state();
            }

            // ─── Keyboard ─────────────────────────────────────────────────
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

                if event.state == ElementState::Pressed {
                    // Keyboard actions can change drawing mode (Escape, Delete, etc.)
                    // which is reflected in the left toolbar — mark it dirty.
                    // Delete can remove objects from the object tree — mark sidebar dirty.
                    // Keyboard can also modify chart state (drawings, mode) — mark chart dirty.
                    pw.toolbar_dirty = true;
                    pw.sidebar_dirty_scene = true;
                    pw.chart_dirty = true;

                    // ── Ctrl shortcuts — use physical_key so layout doesn't matter ──
                    // On a Russian keyboard, Ctrl+С (Cyrillic) still maps to
                    // PhysicalKey::Code(KeyCode::KeyC), matching the physical position.
                    if pw.modifiers.control_key() {
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::KeyS) => {
                                pw.screenshot_pending = true;
                                eprintln!("[Screenshot] Capture requested via Ctrl+S");
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyA) => {
                                pw.chart.on_key_press(chart_app::KeyPress::SelectAll);
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyC) => {
                                // Chat-first: if the focused leaf is a Chat leaf with an
                                // active selection, copy selected message lines to clipboard.
                                {
                                    let chat_text = pw.chart.chat_selection_text();
                                    if let Some((leaf_id, text)) = chat_text {
                                        if let Ok(mut cb) = arboard::Clipboard::new() {
                                            let _ = cb.set_text(text);
                                        }
                                        pw.chart.sidebar_state.agent_chat_selections.remove(&leaf_id);
                                        pw.sidebar_dirty_scene = true;
                                        return;
                                    }
                                }
                                // PTY-second: if there's a host-side PTY selection,
                                // copy it to clipboard and clear. Otherwise send \x03
                                // to the running CLI.
                                if pw.chart.is_agent_pty_focused() {
                                    let sel_text = pw.chart.pty_selection_text();
                                    if !sel_text.is_empty() {
                                        if let Ok(mut cb) = arboard::Clipboard::new() {
                                            let _ = cb.set_text(sel_text);
                                        }
                                        pw.chart.clear_pty_selection();
                                    } else {
                                        pw.chart.on_key_press(chart_app::KeyPress::CtrlC);
                                    }
                                    return;
                                }
                                if let Some(text) = pw.chart.on_copy_selection() {
                                    if let Ok(mut cb) = arboard::Clipboard::new() {
                                        let _ = cb.set_text(text);
                                    }
                                }
                                // No redraw needed — state unchanged.
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyV) => {
                                if let Ok(mut cb) = arboard::Clipboard::new() {
                                    if let Ok(text) = cb.get_text() {
                                        if pw.chart.is_agent_pty_focused() {
                                            pw.chart.paste_to_pty(&text);
                                        } else {
                                            pw.chart.on_key_press(chart_app::KeyPress::Paste(text));
                                        }
                                    }
                                }
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyZ) => {
                                if pw.modifiers.shift_key() {
                                    pw.chart.on_key_press(chart_app::KeyPress::Redo);
                                } else {
                                    pw.chart.on_key_press(chart_app::KeyPress::Undo);
                                }
                                return;
                            }
                            PhysicalKey::Code(KeyCode::KeyY) => {
                                pw.chart.on_key_press(chart_app::KeyPress::Redo);
                                return;
                            }
                            // ── Ctrl+B — test: switch to Binance live data ──────
                            PhysicalKey::Code(KeyCode::KeyB) => {
                                eprintln!("[ChartApp] Ctrl+B: switching to Binance");
                                pw.chart.switch_to_exchange(chart_app::ExchangeId::Binance);
                                return;
                            }
                            _ => {}
                        }
                    }

                    // ── PTY-first key routing ─────────────────────────────
                    // If the Agent PTY owns focus, translate named keys directly
                    // to KeyPress variants so TIM emits raw PTY bytes.
                    if pw.chart.is_agent_pty_focused() {
                        let pty_key = match &event.logical_key {
                            Key::Named(NamedKey::Escape) => Some(chart_app::KeyPress::Escape),
                            Key::Named(NamedKey::Enter) => Some(chart_app::KeyPress::Enter),
                            Key::Named(NamedKey::Tab) => Some(chart_app::KeyPress::Tab),
                            Key::Named(NamedKey::Backspace) => Some(chart_app::KeyPress::Backspace),
                            Key::Named(NamedKey::Delete) => Some(chart_app::KeyPress::Delete),
                            Key::Named(NamedKey::ArrowLeft) => Some(chart_app::KeyPress::ArrowLeft),
                            Key::Named(NamedKey::ArrowRight) => Some(chart_app::KeyPress::ArrowRight),
                            Key::Named(NamedKey::ArrowUp) => Some(chart_app::KeyPress::ArrowUp),
                            Key::Named(NamedKey::ArrowDown) => Some(chart_app::KeyPress::ArrowDown),
                            Key::Named(NamedKey::Home) => Some(chart_app::KeyPress::Home),
                            Key::Named(NamedKey::End) => Some(chart_app::KeyPress::End),
                            Key::Named(NamedKey::PageUp) => Some(chart_app::KeyPress::PageUp),
                            Key::Named(NamedKey::PageDown) => Some(chart_app::KeyPress::PageDown),
                            _ => None,
                        };
                        if let Some(k) = pty_key {
                            pw.chart.on_key_press(k);
                            return;
                        }
                        // Space + printable chars still go via on_char_input below.
                    }

                    let mut handled = true;
                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            pw.chart.on_escape();
                            // Escape cancels drawing — clear the polling flag.
                            #[cfg(target_os = "windows")]
                            {
                                pw.drawing_capture = false;
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            pw.chart.on_char_input('\x08');
                        }
                        Key::Named(NamedKey::Enter) => {
                            pw.chart.on_char_input('\n');
                        }
                        Key::Named(NamedKey::Space) => {
                            pw.chart.on_char_input(' ');
                        }
                        Key::Named(NamedKey::Tab) => {
                            pw.chart.on_char_input('\x09');
                        }
                        Key::Named(NamedKey::Delete) => {
                            pw.chart.on_key_press(chart_app::KeyPress::Delete);
                        }
                        Key::Named(NamedKey::ArrowLeft) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftLeft);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::ArrowLeft);
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftRight);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::ArrowRight);
                            }
                        }
                        Key::Named(NamedKey::Home) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftHome);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::Home);
                            }
                        }
                        Key::Named(NamedKey::End) => {
                            if pw.modifiers.shift_key() {
                                pw.chart.on_key_press(chart_app::KeyPress::ShiftEnd);
                            } else {
                                pw.chart.on_key_press(chart_app::KeyPress::End);
                            }
                        }
                        Key::Character(text) => {
                            // Do NOT forward characters when Ctrl or Alt is held —
                            // any Ctrl+key shortcut that reaches here was not matched
                            // above (e.g. an unhandled Ctrl combo) and must not produce
                            // visible text.  Alt combos (dead keys, AltGr) are kept as
                            // they are needed for some layouts.
                            if !pw.modifiers.control_key() {
                                for ch in text.chars() {
                                    pw.chart.on_char_input(ch);
                                }
                            }
                        }
                        _ => {
                            handled = false;
                        }
                    }
                    let _ = handled;
                }
            }

            // ─── IME commit ───────────────────────────────────────────────
            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                for ch in text.chars() {
                    pw.chart.on_char_input(ch);
                }
            }

            _ => {}
        }
    }
}
