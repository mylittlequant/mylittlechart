//! Build CPU-side Vello Scene from per-window chart + UI state.

use vello_context::VelloGpuRenderContext;
use sidebar_content::state::RenderBackend;
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
pub(crate) fn build_window_scene(pw: &mut PerWindowState, active_toasts: &[alert_delivery::ToastNotification], frame_time: u64) -> u64 {
    let width = pw.surface.config.width;
    let height = pw.surface.config.height;

    if width == 0 || height == 0 {
        return 0;
    }

    let t0 = std::time::Instant::now();

    pw.scene.reset();

    // Sync chrome colours from the chart theme.
    #[cfg(target_os = "windows")]
    let dwm_border_color: String;
    {
        let theme = pw.chart.panel_app.theme_manager.current();
        pw.chrome_state.colors.background = theme.chart.background.clone();
        pw.chrome_state.colors.icon_normal = theme.colors.text_primary.clone();
        pw.chrome_state.colors.icon_hover  = theme.colors.accent.clone();
        pw.chrome_state.colors.button_hover = theme.colors.button_bg_hover.clone();
        pw.chrome_state.colors.close_hover = theme.colors.danger.clone();
        pw.chrome_state.colors.separator   = theme.colors.toolbar_divider.clone();
        pw.chrome_state.colors.tab_accent  = theme.colors.accent.clone();
        pw.chrome_state.colors.tooltip_bg  = theme.colors.button_bg.clone();
        pw.chrome_state.colors.tooltip_text = theme.colors.text_primary.clone();
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
        let mut tabs: Vec<chrome::Tab> = open_tabs
            .iter()
            .filter_map(|tab_id| {
                pw.chart.panel_app.presets.get(tab_id).map(|p| chrome::Tab {
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
            let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
            let mut chrome_ctx =
                VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::update_tab_widths(&mut chrome_ctx, &mut pw.chrome_state);
            chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64, skeleton_active);
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
        }
        pw.scene.append(&pw.chart_scene, None);

        let sidebar_is_open = pw.chart.sidebar_state.is_right_open();

        // In skeleton mode (profile manager / welcome wizard active) the sidebar
        // and overlay are hidden — skip compositing both so no stale pixels appear.
        let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
            || pw.chart.panel_app.user_settings_state.show_welcome_wizard;

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

        // Panel overlay popups — drawn after sidebar and toolbar so they appear
        // on top of both.  These include the sync color grid (panel target) and
        // the panel sync menu (gear-icon dropdown on panel headers).
        {
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
        if pw.chrome_state.context_menu.open {
            let mut overlay_ctx = VelloGpuRenderContext::new(&mut pw.scene, 0.0, 0.0, None, None);
            chrome::render_context_menu(&mut overlay_ctx, &pw.chrome_state.context_menu, &pw.chrome_state.colors);
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
                let mut chrome_ctx = instanced_context::InstancedChartRenderContext::new(
                    width as f32,
                    height as f32,
                    0.0,
                    0.0,
                    None,
                    None,
                );
                chrome::update_tab_widths(&mut chrome_ctx, &mut pw.chrome_state);
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut chrome_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }

                // Render chart + toolbar + sidebar + modals at y_offset=CHROME_HEIGHT.
                let mut chart_ctx = instanced_context::InstancedChartRenderContext::new(
                    width as f32,
                    height as f32,
                    0.0,
                    chrome::CHROME_HEIGHT as f32,
                    None,
                    None,
                );
                pw.chart.render(&mut chart_ctx, frame_time, false);

                // Render chrome context menu overlay (at y_offset=0, absolute coords).
                if pw.chrome_state.context_menu.open {
                    let mut overlay_ctx = instanced_context::InstancedChartRenderContext::new(
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
                    chart_ctx.inner_mut().draw_commands.extend_from_slice(&overlay_ctx.inner().draw_commands);
                }

                // Render toast overlays.
                if !active_toasts.is_empty() {
                    let mut toast_ctx = instanced_context::InstancedChartRenderContext::new(
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
                    let mut tooltip_ctx = instanced_context::InstancedChartRenderContext::new(
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

                // Merge chrome instances into chart instances and store for GPU submission.
                let chrome_inner = chrome_ctx.inner();
                chart_ctx.inner_mut().draw_commands.extend_from_slice(&chrome_inner.draw_commands);

                pw.instanced_commands = chart_ctx.inner().draw_commands.clone();
            }
            RenderBackend::VelloCpu => {
                let mut cpu_ctx = vello_cpu_context::VelloCpuChartRenderContext::new(
                    1.0, // dpr
                    None,
                    None,
                );
                cpu_ctx.inner_mut().begin_frame(width, height);
                // Render chrome at y=0 (no offset).
                chrome::update_tab_widths(&mut cpu_ctx, &mut pw.chrome_state);
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut cpu_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }
                // Render chart + toolbar + sidebar + modals offset by CHROME_HEIGHT.
                cpu_ctx.save();
                cpu_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut cpu_ctx, frame_time, false);
                cpu_ctx.restore();
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open {
                    chrome::render_context_menu(
                        &mut cpu_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut cpu_ctx, active_toasts, width as f64, height as f64);
                }
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut cpu_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut cpu_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
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
                let mut skia_ctx = tiny_skia_context::TinySkiaChartRenderContext::new(
                    width, height, 1.0, None, None,
                );
                // Clear to background color (Pixmap::new() initializes to transparent).
                skia_ctx.set_fill_color("#131722");
                skia_ctx.fill_rect(0.0, 0.0, width as f64, height as f64);
                // Render chrome at y=0.
                chrome::update_tab_widths(&mut skia_ctx, &mut pw.chrome_state);
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut skia_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }
                skia_ctx.save();
                skia_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut skia_ctx, frame_time, false);
                skia_ctx.restore();
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open {
                    chrome::render_context_menu(
                        &mut skia_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut skia_ctx, active_toasts, width as f64, height as f64);
                }
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut skia_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut skia_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
                let pixels = skia_ctx.inner().pixels();
                pw.cpu_chart_dims = (width, height);
                pw.cpu_chart_pixels = pixels.to_vec();
            }
            RenderBackend::VelloHybrid => {
                let mut hybrid_ctx = vello_hybrid_context::VelloHybridChartRenderContext::new(
                    1.0, None, None,
                );
                hybrid_ctx.inner_mut().begin_frame(width, height);
                // Render chrome at y=0.
                chrome::update_tab_widths(&mut hybrid_ctx, &mut pw.chrome_state);
                {
                    let skeleton_active = pw.chart.panel_app.user_settings_state.show_profile_manager
                        || pw.chart.panel_app.user_settings_state.show_welcome_wizard;
                    chrome::render(&mut hybrid_ctx, &pw.chrome_state, width as f64, skeleton_active);
                }
                hybrid_ctx.translate(0.0, chrome::CHROME_HEIGHT);
                pw.chart.render(&mut hybrid_ctx, frame_time, false);
                hybrid_ctx.translate(0.0, -chrome::CHROME_HEIGHT);
                // Render chrome context menu overlay.
                if pw.chrome_state.context_menu.open {
                    chrome::render_context_menu(
                        &mut hybrid_ctx,
                        &pw.chrome_state.context_menu,
                        &pw.chrome_state.colors,
                    );
                }
                // Render toast overlays.
                if !active_toasts.is_empty() {
                    render_toasts(&mut hybrid_ctx, active_toasts, width as f64, height as f64);
                }
                // Render tooltips (top-most layer).
                chrome::render_tooltip(&mut hybrid_ctx, &pw.chrome_state, width as f64, height as f64);
                chrome::render_tooltip_themed(&mut hybrid_ctx, &pw.toolbar_tooltip, &pw.chrome_state.colors.tooltip_bg, &pw.chrome_state.colors.tooltip_text, width as f64, height as f64);
                // Move the inner uzor context (owns the vello_hybrid::Scene)
                // into PerWindowState for GPU submission.
                let mut inner = uzor_render_vello_hybrid::VelloHybridRenderContext::new(1.0);
                std::mem::swap(&mut inner, hybrid_ctx.inner_mut());
                pw.hybrid_ctx = Some(inner);
            }
            RenderBackend::VelloGpu => unreachable!("handled above"),
        }
    }

    t0.elapsed().as_micros() as u64
}
