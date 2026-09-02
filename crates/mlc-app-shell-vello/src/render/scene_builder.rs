//! Build CPU-side Vello Scene from per-window chart + UI state.

use mlc_app_render_context_vello::VelloGpuRenderContext;
use mlc_app_sidebar_content::state::RenderBackend;
use crate::{
    chrome,
    PerWindowState,
    render_toasts,
};

/// Phase 1 of rendering: build the Vello scene for a single window (CPU-only).
///
/// Performs all vector-graphics work: chrome sync, toolbar/sidebar cache
/// management, chart render, overlay compositing.  No GPU calls are made here
/// so multiple windows can run this phase concurrently via
/// `std::thread::scope`.
///
/// Returns the wall-clock microseconds spent building the scene so callers can
/// track the parallel phase duration correctly.
pub(crate) fn build_window_scene(pw: &mut PerWindowState, active_toasts: &[mlc_alert_delivery::ToastNotification], frame_time: u64) -> u64 {
    let width = pw.surface.config.width;
    let height = pw.surface.config.height;

    if width == 0 || height == 0 {
        return 0;
    }

    let t0 = web_time::Instant::now();

    let _reset_start = web_time::Instant::now();
    pw.scene.reset();
    let reset_us = _reset_start.elapsed().as_micros() as u64;

    // Sync chrome colours from the chart theme.
    #[cfg(target_os = "windows")]
    let dwm_border_color: String;
    // Spinner colours pulled from the active theme so the loading indicator
    // doesn't look hand-painted against whichever skin the user picked.
    let spinner_active_color: String;
    let spinner_backdrop_color: String;
    {
        let theme = pw.chart.panel_app.theme_manager.current();
        pw.chrome_state.colors.background = theme.chart.background.clone();
        pw.chrome_state.colors.icon_normal = theme.colors.text_primary.clone();
        pw.chrome_state.colors.icon_hover  = theme.colors.accent.clone();
        pw.chrome_state.colors.button_hover = theme.colors.button_bg_hover.clone();
        pw.chrome_state.colors.close_hover = theme.colors.danger.clone();
        pw.chrome_state.colors.separator   = theme.colors.toolbar_divider.clone();
        pw.chrome_state.colors.tab_accent  = theme.colors.accent.clone();
        pw.chrome_state.colors.cta_bg      = theme.colors.accent.clone();
        pw.chrome_state.colors.cta_bg_hover = theme.colors.accent_hover.clone();
        pw.chrome_state.colors.tooltip_bg  = theme.colors.button_bg.clone();
        pw.chrome_state.colors.tooltip_text = theme.colors.text_primary.clone();
        spinner_active_color = theme.colors.accent.clone();
        spinner_backdrop_color = theme.chart.scale_border.clone();
        #[cfg(target_os = "windows")]
        { dwm_border_color = theme.colors.ui_border.clone(); }
    }
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = pw.hwnd {
        crate::platform::win32::win32_border::set_dwm_border_color(hwnd, &dwm_border_color);
    }

    // Sync tabs from open_tabs order.
    {
        let open_tabs = &pw.chart.panel_app.open_tabs;
        let active_id = &pw.chart.panel_app.active_preset_id;
        let presets_guard_sb = pw.chart.core.presets();
        let mut tabs: Vec<chrome::Tab> = open_tabs
            .iter()
            .filter_map(|tab_id| {
                presets_guard_sb.get(tab_id).map(|p| chrome::Tab {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    active: &p.id == active_id,
                })
            })
            .collect();
        if pw.skeleton {
            // Skeleton: no tabs at all, chrome draws only + and system buttons.
            tabs.clear();
        } else if tabs.is_empty() {
            // Safety net: should not happen after open_tabs is always populated.
            eprintln!("[Chrome] WARNING: tabs empty — injecting fallback Untitled");
            tabs.push(chrome::Tab {
                id: "__fallback__".to_string(),
                name: "Untitled".to_string(),
                active: true,
            });
        } else if !tabs.iter().any(|t| t.active) {
            if let Some(first) = tabs.first_mut() {
                first.active = true;
            }
        }
        pw.chrome_state.tabs = tabs;
    }

    let is_vello_gpu = pw.render_backend == RenderBackend::VelloGpu;

    if is_vello_gpu {
        // ── VelloGpu: everything renders into pw.scene via vello ──────────────

        // Render chrome strip
        {
            // Only the migration page (no live chart behind it yet) still
            // suppresses tabs/buttons — an ordinary profile-manager/welcome
            // modal dims instead (see the backdrop-dim block below) and
            // leaves the chrome interactive, same as every other modal
            // (owner's ruling 2026-08-31).
            let skeleton_active = pw.chart.panel_app.user_settings_state.profile_manager_mandatory;
            let mut chrome_ctx =
                VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::update_tab_widths(&mut chrome_ctx, &mut pw.chrome_state, width as f64, &pw.chart.input_coordinator);
            chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64, skeleton_active, &pw.chart.input_coordinator);
            if let Some(backdrop) = pw.chart.panel_app.dim_backdrop_color() {
                chrome::render_backdrop_dim(&mut chrome_ctx, width as f64, pw.chrome_state.flavor, &backdrop);
            }
        }

        // Toolbar dirty-cache
        if pw.toolbar_dirty {
            pw.toolbar_scene.reset();
            let mut tb_ctx = VelloGpuRenderContext::new(
                &mut pw.toolbar_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render_toolbar_only(&mut tb_ctx);
            pw.toolbar_dirty = false;
        }

        // Chart content (chart panels, modals) — toolbar skipped because
        // it is composited from the cached toolbar_scene below.
        // The chart scene is cached: only rebuild when chart_dirty is set.
        let chart_rebuilt = pw.chart_dirty;
        if pw.chart_dirty {
            pw.chart_scene.reset();
            let mut render_ctx = VelloGpuRenderContext::new(
                &mut pw.chart_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render(&mut render_ctx, frame_time, true);
            pw.chart_dirty = false;
            // First live chart frame after the promote: the chart scene is now
            // built and about to composite. BUT retire the big startup
            // spinner ONLY if the chart actually has bars to show — otherwise
            // the next frame falls into render_full_chart_panel's empty-bars
            // branch (3 scale border lines + per-chart spinner) and the
            // owner sees "big spinner → empty scaffold → small spinner →
            // real chart" instead of "big spinner → real chart". Holding
            // the big spinner across the empty interval bridges that gap.
            if pw.awaiting_first_live_frame {
                let chart_populated = pw
                    .chart
                    .panel_app
                    .panel_grid
                    .iter_containers()
                    .any(|(_, w)| !pw.chart.core.bubbles().bars(w.current_tag_id).is_empty());
                if chart_populated {
                    pw.awaiting_first_live_frame = false;
                    pw.spinner_start = None;
                }
            }
        }
        let _composite_start = web_time::Instant::now();
        pw.scene.append(&pw.chart_scene, None);

        let sidebar_is_open = pw.chart.sidebar_state.is_right_open();

        // In skeleton mode (the mandatory migration page, which has no live
        // chart behind it) the sidebar and overlay are hidden — skip
        // compositing both so no stale pixels appear. An ordinary
        // profile-manager/welcome modal no longer counts: it dims instead of
        // hiding, and the chart underneath keeps rendering normally (owner's
        // ruling 2026-08-31).
        let skeleton_active = pw.chart.panel_app.user_settings_state.profile_manager_mandatory;

        // Overlay layer (crosshair / cursor labels / tooltip / drawing preview).
        // Rebuilt when chart geometry changed this frame OR overlay_dirty is set.
        // Composited on top of the chart content every frame (append is cheap).
        // Skipped entirely in skeleton mode so the crosshair never draws over the
        // loading/profile-unlock screen.
        if !skeleton_active {
            if chart_rebuilt || pw.overlay_dirty {
                pw.overlay_scene.reset();
                let mut overlay_ctx = VelloGpuRenderContext::new(
                    &mut pw.overlay_scene,
                    0.0,
                    chrome::CHROME_HEIGHT,
                    None,
                    None,
                );
                pw.chart.render_overlay(&mut overlay_ctx);
                pw.overlay_dirty = false;
            }
            pw.scene.append(&pw.overlay_scene, None);

            // Per-chart loading spinner overlay — drawn directly into pw.scene
            // (NOT cached) so it animates every frame. Goes on top of the
            // overlay layer so it sits above the crosshair too.
            // Suppress per-chart spinners while the global startup spinner
            // is up (`spinner_start.is_some()`) — owner sees one centred
            // ring instead of "big ring + tiny ring in the middle of every
            // empty pane" at startup.
            // Per-chart spinner suppressed when ANY of these is true:
            //   - big startup spinner up (`spinner_start`)
            //   - still holding the spinner across the first-live-frame gap
            //     (`awaiting_first_live_frame`)
            //   - welcome wizard / profile manager screen is up
            // Owner wants exactly one ring on screen at a time.
            let _skel_active_for_spin = pw.chart.panel_app.user_settings_state.show_profile_manager
                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
            if !pw.chart.last_loading_spinners.is_empty()
                && pw.spinner_start.is_none()
                && !pw.awaiting_first_live_frame
                && !_skel_active_for_spin
            {
                let elapsed = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let mut spin_ctx = VelloGpuRenderContext::new(
                    &mut pw.scene, 0.0, chrome::CHROME_HEIGHT, None, None,
                );
                render_loading_spinners(
                    &mut spin_ctx,
                    &pw.chart.last_loading_spinners,
                    elapsed,
                    &spinner_active_color,
                    &spinner_backdrop_color,
                );
            }
        }

        // Sidebar scene rebuild (after render, within the same input frame)
        if sidebar_is_open && !skeleton_active && pw.sidebar_dirty_scene {
            pw.sidebar_scene.reset();
            let mut sb_ctx = VelloGpuRenderContext::new(
                &mut pw.sidebar_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render_sidebar_only(&mut sb_ctx);
            pw.sidebar_dirty_scene = false;
        }

        // Composite cached sidebar on top of chart content (below toolbar).
        if sidebar_is_open && !skeleton_active {
            pw.scene.append(&pw.sidebar_scene, None);
        }

        // Composite cached toolbar on top of chart content.
        pw.scene.append(&pw.toolbar_scene, None);
        let composite_us = _composite_start.elapsed().as_micros() as u64;
        // Store composite timing (reset + core appends) in per-window state for the PERF panel.
        pw.chart.last_composite_us = reset_us + composite_us;

        // Modal layer — drawn AFTER overlay (crosshair) and toolbar so that modals
        // (Symbol Search, settings dialogs, color pickers, context menu, watchlist,
        // clock popup, AND the profile-manager / welcome-wizard / skeleton-fill that
        // make up the loading/login screen) always appear on top of everything else.
        // These are application-level UI, not chart content — so they belong here,
        // above the chart+overlay, never inside chart_scene.
        //
        // Drawn UNCONDITIONALLY every frame: render_modal_layer internally gates each
        // sub-modal on its own is_open() flag and draws nothing when closed (cheap —
        // a fresh empty Scene + a few branch checks). An external "any modal open"
        // gate was WRONG: it omitted the skeleton/profile/welcome screens, so the
        // login/wizard vanished and the chart bled through. Never gate this.
        //
        // Input-frame contract: render_modal_layer() does NOT call begin_frame() /
        // end_frame() — same contract as render_toolbar_only() and render_sidebar_only().
        // Widget registrations accumulate in input_coordinator.widgets and are cleared
        // by the NEXT frame's begin_frame(), which is correct: input events that arrive
        // between renders hit-test against the most-recently-registered modal zones.
        {
            let mut modal_scene = vello::Scene::new();
            let mut modal_ctx = VelloGpuRenderContext::new(
                &mut modal_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render_modal_layer(&mut modal_ctx, frame_time);
            pw.scene.append(&modal_scene, None);
        }

        // Panel overlay popups — drawn after sidebar and toolbar so they appear
        // on top of both.  These include the sync color grid (panel target) and
        // the panel sync menu (gear-icon dropdown on panel headers).
        // Skip the per-frame Scene allocation + append entirely when nothing is
        // open (the common case) — this block ran unconditionally every frame.
        if pw.chart.has_panel_overlay_popups() {
            let mut popup_scene = vello::Scene::new();
            let mut popup_ctx = VelloGpuRenderContext::new(
                &mut popup_scene,
                0.0,
                chrome::CHROME_HEIGHT,
                None,
                None,
            );
            pw.chart.render_panel_overlay_popups(&mut popup_ctx);
            pw.scene.append(&popup_scene, None);
        }

        // Render chrome context menu overlay
        if pw.chrome_state.context_menu.open || pw.chrome_state.tab_list_open {
            let mut overlay_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::render_context_menu(&mut overlay_ctx, &pw.chrome_state.context_menu, &pw.chrome_state.colors);
            chrome::render_tab_list(&mut overlay_ctx, &pw.chrome_state, width as f64);
        }

        // Render toast notification overlays (top-right corner)
        if !active_toasts.is_empty() {
            let mut toast_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            render_toasts(&mut toast_ctx, active_toasts, width as f64, height as f64);
        }

        // Render tooltips (top-most layer): chrome + toolbar
        {
            let mut tooltip_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::render_tooltip(&mut tooltip_ctx, &pw.chrome_state, width as f64, height as f64);
            chrome::render_tooltip_themed(&mut tooltip_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
        }

        // Spinner overlay — drawn topmost when the off-thread promote worker is running.
        if let Some(start) = pw.spinner_start {
            let mut spinner_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            render_spinner(&mut spinner_ctx, width as f64, height as f64, start.elapsed().as_millis() as u64, &spinner_active_color, &spinner_backdrop_color);
        }
    } else {
        // ── Non-VelloGpu: full backend switch ─────────────────────────────────
        // Everything renders through the selected backend only.
        // No vello scene is built — pw.scene stays empty (reset above).
        // pw.chart.render() renders EVERYTHING: chrome, toolbar, sidebar, modals.
        // We still need chrome hit-zone registration via update_tab_widths.
        //
        // Note: sidebar/toolbar dirty flags are NOT cleared here since those
        // caches are vello-only.  The alt backend re-renders everything each
        // frame so dirty tracking is not needed.

        // Bring uzor render traits into scope so that save/translate/restore/
        // set_fill_color/fill_rect etc. are callable on concrete context types.
        use uzor::render::{Painter as _, ShapeHelpers as _};

        match pw.render_backend {
            RenderBackend::InstancedWgpu => {
                // Render chrome at y_offset=0 into a temporary context.
                let mut chrome_ctx = mlc_app_render_context_instanced::InstancedChartRenderContext::new(
                    width as f32,
                    height as f32,
                    0.0,
                    0.0,
                    None,
                    None,
                );
                chrome::update_tab_widths(&mut chrome_ctx, &mut pw.chrome_state, width as f64, &pw.chart.input_coordinator);
                {
                    // See the VelloGpu branch above for why this is
                    // `profile_manager_mandatory` and not the old
                    // show_profile_manager||show_welcome_wizard pair.
                    let skeleton_active = pw.chart.panel_app.user_settings_state.profile_manager_mandatory;
                    chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64, skeleton_active, &pw.chart.input_coordinator);
                    if let Some(backdrop) = pw.chart.panel_app.dim_backdrop_color() {
                        chrome::render_backdrop_dim(&mut chrome_ctx, width as f64, pw.chrome_state.flavor, &backdrop);
                    }
                }

                // Render chart + toolbar + sidebar at y_offset=CHROME_HEIGHT.
                let mut chart_ctx = mlc_app_render_context_instanced::InstancedChartRenderContext::new(
                    width as f32,
                    height as f32,
                    0.0,
                    chrome::CHROME_HEIGHT as f32,
                    None,
                    None,
                );
                pw.chart.render(&mut chart_ctx, frame_time, false);
                // Render modal layer on top (same ctx — single flat draw list).
                // Unconditional: render_modal_layer self-gates each sub-modal AND
                // draws the profile/welcome/skeleton screens — gating it externally
                // hid the loading/login screen.
                pw.chart.render_modal_layer(&mut chart_ctx, frame_time);

                // Per-chart loading spinner overlay (animated every frame; the
                // host redraw pump in main.rs keeps frames flowing while the
                // spinner list is non-empty).
                // Suppress per-chart spinners while the global startup spinner
            // is up (`spinner_start.is_some()`) — owner sees one centred
            // ring instead of "big ring + tiny ring in the middle of every
            // empty pane" at startup.
            // Per-chart spinner suppressed when ANY of these is true:
            //   - big startup spinner up (`spinner_start`)
            //   - still holding the spinner across the first-live-frame gap
            //     (`awaiting_first_live_frame`)
            //   - welcome wizard / profile manager screen is up
            // Owner wants exactly one ring on screen at a time.
            let _skel_active_for_spin = pw.chart.panel_app.user_settings_state.show_profile_manager
                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
            if !pw.chart.last_loading_spinners.is_empty()
                && pw.spinner_start.is_none()
                && !pw.awaiting_first_live_frame
                && !_skel_active_for_spin
            {
                    let elapsed = web_time::SystemTime::now()
                        .duration_since(web_time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    render_loading_spinners(
                        &mut chart_ctx,
                        &pw.chart.last_loading_spinners,
                        elapsed,
                        &spinner_active_color,
                        &spinner_backdrop_color,
                    );
                }

                // Render chrome context menu overlay (at y_offset=0, absolute coords).
                if pw.chrome_state.context_menu.open || pw.chrome_state.tab_list_open {
                    let mut overlay_ctx = mlc_app_render_context_instanced::InstancedChartRenderContext::new(
                        width as f32,
                        height as f32,
                        0.0,
                        0.0,
                        None,
                        None,
                    );
                    chrome::render_context_menu(
                        &mut overlay_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                    chrome::render_tab_list(&mut overlay_ctx, &pw.chrome_state, width as f64);
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&overlay_ctx.inner().draw_commands);
                }

                // Render toast overlays.
                if !active_toasts.is_empty() {
                    let mut toast_ctx = mlc_app_render_context_instanced::InstancedChartRenderContext::new(
                        width as f32,
                        height as f32,
                        0.0,
                        0.0,
                        None,
                        None,
                    );
                    render_toasts(&mut toast_ctx, active_toasts, width as f64, height as f64);
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&toast_ctx.inner().draw_commands);
                }

                // Render chrome tooltip (top-most layer).
                {
                    let mut tooltip_ctx = mlc_app_render_context_instanced::InstancedChartRenderContext::new(
                        width as f32,
                        height as f32,
                        0.0,
                        0.0,
                        None,
                        None,
                    );
                    chrome::render_tooltip(&mut tooltip_ctx, &pw.chrome_state, width as f64, height as f64);
                    chrome::render_tooltip_themed(&mut tooltip_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&tooltip_ctx.inner().draw_commands);
                }

                // Spinner overlay (topmost layer, when promote worker is running).
                if let Some(start) = pw.spinner_start {
                    let mut spinner_ctx = mlc_app_render_context_instanced::InstancedChartRenderContext::new(
                        width as f32,
                        height as f32,
                        0.0,
                        0.0,
                        None,
                        None,
                    );
                    render_spinner(&mut spinner_ctx, width as f64, height as f64, start.elapsed().as_millis() as u64, &spinner_active_color, &spinner_backdrop_color);
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&spinner_ctx.inner().draw_commands);
                }

                // Merge chrome instances into chart instances and store for GPU submission.
                let chrome_inner = chrome_ctx.inner();
                chart_ctx.inner_mut().draw_commands.extend_from_slice(&chrome_inner.draw_commands);

                pw.instanced_commands = chart_ctx.inner().draw_commands.clone();
            }
            RenderBackend::VelloCpu => {
                let mut cpu_ctx = mlc_app_render_context_vello_cpu::VelloCpuChartRenderContext::new(
                    1.0, // dpr
                    None,
                    None,
                );
                cpu_ctx.inner_mut().begin_frame(width, height);
                // Render chrome at y=0 (no offset).
                chrome::update_tab_widths(&mut cpu_ctx, &mut pw.chrome_state, width as f64, &pw.chart.input_coordinator);
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.profile_manager_mandatory;
                    chrome::render(&mut cpu_ctx, &pw.chrome_state, width as f64, skeleton_active, &pw.chart.input_coordinator);
                    if let Some(backdrop) = pw.chart.panel_app.dim_backdrop_color() {
                        chrome::render_backdrop_dim(&mut cpu_ctx, width as f64, pw.chrome_state.flavor, &backdrop);
                    }
                }
                // Render chart + toolbar + sidebar offset by CHROME_HEIGHT.
                cpu_ctx.save();
                cpu_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut cpu_ctx, frame_time, false);
                // Render modal layer on top (still inside translate — modals use chart-relative coords).
                // Unconditional — draws profile/welcome/skeleton screens too.
                pw.chart.render_modal_layer(&mut cpu_ctx, frame_time);
                // Suppress per-chart spinners while the global startup spinner
            // is up (`spinner_start.is_some()`) — owner sees one centred
            // ring instead of "big ring + tiny ring in the middle of every
            // empty pane" at startup.
            // Per-chart spinner suppressed when ANY of these is true:
            //   - big startup spinner up (`spinner_start`)
            //   - still holding the spinner across the first-live-frame gap
            //     (`awaiting_first_live_frame`)
            //   - welcome wizard / profile manager screen is up
            // Owner wants exactly one ring on screen at a time.
            let _skel_active_for_spin = pw.chart.panel_app.user_settings_state.show_profile_manager
                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
            if !pw.chart.last_loading_spinners.is_empty()
                && pw.spinner_start.is_none()
                && !pw.awaiting_first_live_frame
                && !_skel_active_for_spin
            {
                    let elapsed = web_time::SystemTime::now()
                        .duration_since(web_time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    render_loading_spinners(
                        &mut cpu_ctx,
                        &pw.chart.last_loading_spinners,
                        elapsed,
                        &spinner_active_color,
                        &spinner_backdrop_color,
                    );
                }
                cpu_ctx.restore();
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open || pw.chrome_state.tab_list_open {
                    chrome::render_context_menu(
                        &mut cpu_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                chrome::render_tab_list(&mut cpu_ctx, &pw.chrome_state, width as f64);
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut cpu_ctx, active_toasts, width as f64, height as f64);
                }
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut cpu_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut cpu_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
                // Spinner overlay (topmost, when promote worker is running).
                if let Some(start) = pw.spinner_start {
                    render_spinner(&mut cpu_ctx, width as f64, height as f64, start.elapsed().as_millis() as u64, &spinner_active_color, &spinner_backdrop_color);
                }
                // Rasterize to pixel buffer.
                let pixel_count = (width as usize) * (height as usize) * 4;
                let mut pixels = vec![0u8; pixel_count];
                cpu_ctx.inner_mut().render_to_pixmap_rgba8(
                    &mut pixels,
                    width.min(u16::MAX as u32) as u16,
                    height.min(u16::MAX as u32) as u16,
                );
                pw.cpu_chart_dims = (width, height);
                pw.cpu_chart_pixels = pixels;
            }
            RenderBackend::TinySkia => {
                let mut skia_ctx = mlc_app_render_context_tiny_skia::TinySkiaChartRenderContext::new(
                    width, height, 1.0, None, None,
                );
                // Clear to background color (Pixmap::new() initializes to transparent).
                skia_ctx.set_fill_color("#131722");
                skia_ctx.fill_rect(0.0, 0.0, width as f64, height as f64);
                // Render chrome at y=0.
                chrome::update_tab_widths(&mut skia_ctx, &mut pw.chrome_state, width as f64, &pw.chart.input_coordinator);
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.profile_manager_mandatory;
                    chrome::render(&mut skia_ctx, &pw.chrome_state, width as f64, skeleton_active, &pw.chart.input_coordinator);
                    if let Some(backdrop) = pw.chart.panel_app.dim_backdrop_color() {
                        chrome::render_backdrop_dim(&mut skia_ctx, width as f64, pw.chrome_state.flavor, &backdrop);
                    }
                }
                skia_ctx.save();
                skia_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut skia_ctx, frame_time, false);
                // Render modal layer on top (still inside translate — modals use chart-relative coords).
                // Unconditional — draws profile/welcome/skeleton screens too.
                pw.chart.render_modal_layer(&mut skia_ctx, frame_time);
                // Suppress per-chart spinners while the global startup spinner
            // is up (`spinner_start.is_some()`) — owner sees one centred
            // ring instead of "big ring + tiny ring in the middle of every
            // empty pane" at startup.
            // Per-chart spinner suppressed when ANY of these is true:
            //   - big startup spinner up (`spinner_start`)
            //   - still holding the spinner across the first-live-frame gap
            //     (`awaiting_first_live_frame`)
            //   - welcome wizard / profile manager screen is up
            // Owner wants exactly one ring on screen at a time.
            let _skel_active_for_spin = pw.chart.panel_app.user_settings_state.show_profile_manager
                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
            if !pw.chart.last_loading_spinners.is_empty()
                && pw.spinner_start.is_none()
                && !pw.awaiting_first_live_frame
                && !_skel_active_for_spin
            {
                    let elapsed = web_time::SystemTime::now()
                        .duration_since(web_time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    render_loading_spinners(
                        &mut skia_ctx,
                        &pw.chart.last_loading_spinners,
                        elapsed,
                        &spinner_active_color,
                        &spinner_backdrop_color,
                    );
                }
                skia_ctx.restore();
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open || pw.chrome_state.tab_list_open {
                    chrome::render_context_menu(
                        &mut skia_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                chrome::render_tab_list(&mut skia_ctx, &pw.chrome_state, width as f64);
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut skia_ctx, active_toasts, width as f64, height as f64);
                }
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut skia_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut skia_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
                // Spinner overlay (topmost, when promote worker is running).
                if let Some(start) = pw.spinner_start {
                    render_spinner(&mut skia_ctx, width as f64, height as f64, start.elapsed().as_millis() as u64, &spinner_active_color, &spinner_backdrop_color);
                }
                let pixels = skia_ctx.inner().pixels();
                pw.cpu_chart_dims = (width, height);
                pw.cpu_chart_pixels = pixels.to_vec();
            }
            RenderBackend::VelloHybrid => {
                let mut hybrid_ctx = mlc_app_render_context_vello_hybrid::VelloHybridChartRenderContext::new(
                    1.0, None, None,
                );
                hybrid_ctx.inner_mut().begin_frame(width, height);
                // Render chrome at y=0.
                chrome::update_tab_widths(&mut hybrid_ctx, &mut pw.chrome_state, width as f64, &pw.chart.input_coordinator);
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.profile_manager_mandatory;
                    chrome::render(&mut hybrid_ctx, &pw.chrome_state, width as f64, skeleton_active, &pw.chart.input_coordinator);
                    if let Some(backdrop) = pw.chart.panel_app.dim_backdrop_color() {
                        chrome::render_backdrop_dim(&mut hybrid_ctx, width as f64, pw.chrome_state.flavor, &backdrop);
                    }
                }
                hybrid_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut hybrid_ctx, frame_time, false);
                // Render modal layer on top (still inside translate — modals use chart-relative coords).
                // Unconditional — draws profile/welcome/skeleton screens too.
                pw.chart.render_modal_layer(&mut hybrid_ctx, frame_time);
                // Suppress per-chart spinners while the global startup spinner
            // is up (`spinner_start.is_some()`) — owner sees one centred
            // ring instead of "big ring + tiny ring in the middle of every
            // empty pane" at startup.
            // Per-chart spinner suppressed when ANY of these is true:
            //   - big startup spinner up (`spinner_start`)
            //   - still holding the spinner across the first-live-frame gap
            //     (`awaiting_first_live_frame`)
            //   - welcome wizard / profile manager screen is up
            // Owner wants exactly one ring on screen at a time.
            let _skel_active_for_spin = pw.chart.panel_app.user_settings_state.show_profile_manager
                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
            if !pw.chart.last_loading_spinners.is_empty()
                && pw.spinner_start.is_none()
                && !pw.awaiting_first_live_frame
                && !_skel_active_for_spin
            {
                    let elapsed = web_time::SystemTime::now()
                        .duration_since(web_time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    render_loading_spinners(
                        &mut hybrid_ctx,
                        &pw.chart.last_loading_spinners,
                        elapsed,
                        &spinner_active_color,
                        &spinner_backdrop_color,
                    );
                }
                hybrid_ctx.translate(0.0, -chrome::CHROME_HEIGHT);
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open || pw.chrome_state.tab_list_open {
                    chrome::render_context_menu(
                        &mut hybrid_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                chrome::render_tab_list(&mut hybrid_ctx, &pw.chrome_state, width as f64);
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut hybrid_ctx, active_toasts, width as f64, height as f64);
                }
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut hybrid_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut hybrid_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
                // Spinner overlay (topmost, when promote worker is running).
                if let Some(start) = pw.spinner_start {
                    render_spinner(&mut hybrid_ctx, width as f64, height as f64, start.elapsed().as_millis() as u64, &spinner_active_color, &spinner_backdrop_color);
                }
                // Move the inner uzor context (owns the vello_hybrid::Scene)
                // into PerWindowState for GPU submission.
                let mut inner = uzor_render_vello_hybrid::VelloHybridRenderContext::new(1.0);
                std::mem::swap(&mut inner, hybrid_ctx.inner_mut());
                pw.hybrid_ctx = Some(inner);
            }
            RenderBackend::VelloGpu => unreachable!("handled above"),
            RenderBackend::Canvas2d => unreachable!("Canvas2d is wasm-only"),
        }
        // Non-VelloGpu backends render the whole chart each frame (no cached
        // chart_scene). Same retire rule as VelloGpu — hold the big spinner
        // until the chart has bars to draw, so the "big spinner → empty
        // scaffold → small spinner → real chart" cascade collapses to
        // "big spinner → real chart".
        if pw.awaiting_first_live_frame {
            let chart_populated = pw
                .chart
                .panel_app
                .panel_grid
                .iter_containers()
                .any(|(_, w)| !pw.chart.core.bubbles().bars(w.current_tag_id).is_empty());
            if chart_populated {
                pw.awaiting_first_live_frame = false;
                pw.spinner_start = None;
            }
        }
    }

    t0.elapsed().as_micros() as u64
}

/// Draw an animated arc spinner centered in the window.
///
/// The spinner is a ~240° arc that rotates one full revolution every 1 000 ms.
/// It is drawn at the topmost layer over the skeleton modal screen so the user
/// sees activity while the off-thread promote worker builds the live ChartApp.
///
/// `elapsed_ms` is the milliseconds since `pw.spinner_start` was set.
/// Render the per-chart loading spinner overlay layer.
///
/// `spinners` carries `(leaf_id, (cx, cy, radius))` for every leaf currently
/// in loading state — produced by `ChartPanelRenderResult.loading_spinner`
/// during `pw.chart.render()` and persisted on `pw.chart.last_loading_spinners`.
/// Coordinates are in chart-content space (NOT including chrome offset) so
/// the caller MUST pass the same `y_offset` the chart was rendered with.
///
/// "Scene-ready" is implicit: the per-chart `bars.is_empty()` check in the
/// layout layer flips false the moment bars land, the next render produces
/// an empty `last_loading_spinners` list, and this fn paints nothing — the
/// transition is instant, on the SAME frame that draws the real chart. The
/// host redraw pump keeps the spinner animated while the list is non-empty.
fn render_loading_spinners<C>(
    ctx: &mut C,
    spinners: &[(mlc_core::LeafId, (f64, f64, f64))],
    elapsed_ms: u64,
    active_color: &str,
    backdrop_color: &str,
)
where
    C: uzor::render::Painter + ?Sized,
{
    if spinners.is_empty() {
        return;
    }
    let t = (elapsed_ms % 1200) as f64 / 1200.0;
    let start_angle = t * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
    let sweep = std::f64::consts::PI * 3.0 / 2.0;

    for (_leaf, (cx, cy, radius)) in spinners {
        let stroke_w = (radius * 0.16).clamp(2.0, 4.0);
        ctx.save();
        ctx.set_stroke_width(stroke_w);
        ctx.set_line_dash(&[]);
        // Backdrop ring.
        ctx.set_stroke_color(backdrop_color);
        ctx.begin_path();
        ctx.arc(*cx, *cy, *radius, 0.0, std::f64::consts::TAU);
        ctx.stroke();
        // Active 270° sweep.
        ctx.set_stroke_color(active_color);
        ctx.begin_path();
        ctx.arc(*cx, *cy, *radius, start_angle, start_angle + sweep);
        ctx.stroke();
        ctx.restore();
    }
}

fn render_spinner(
    ctx: &mut dyn uzor::render::Painter,
    window_width: f64,
    window_height: f64,
    elapsed_ms: u64,
    active_color: &str,
    backdrop_color: &str,
) {
    const RADIUS: f64 = 22.0;
    const STROKE_WIDTH: f64 = 3.0;
    const SWEEP: f64 = std::f64::consts::PI * 3.0 / 2.0; // 270 degrees — 3/4 ring

    // Centred on what the user can SEE, which is the window minus the chrome
    // strip along its top. Halving the raw window height put the ring 16px
    // high — the strip is counted as space the spinner may occupy, and it is
    // not. Owner's rule, 2026-09-03: a whole-app spinner sits on the centre of
    // the window "прям идеально", and the strip is chrome, not window.
    let cx = window_width / 2.0;
    let cy = chrome::CHROME_HEIGHT + (window_height - chrome::CHROME_HEIGHT) / 2.0;

    let t = (elapsed_ms % 1200) as f64 / 1200.0;
    let start_angle = t * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;

    ctx.save();
    ctx.set_stroke_width(STROKE_WIDTH);
    // Backdrop full circle (dim, theme.scale_border).
    ctx.set_stroke_color(backdrop_color);
    ctx.begin_path();
    ctx.arc(cx, cy, RADIUS, 0.0, std::f64::consts::TAU);
    ctx.stroke();
    // Active 270° arc (primary, theme.text_primary).
    ctx.set_stroke_color(active_color);
    ctx.begin_path();
    ctx.arc(cx, cy, RADIUS, start_angle, start_angle + SWEEP);
    ctx.stroke();
    ctx.restore();
}
