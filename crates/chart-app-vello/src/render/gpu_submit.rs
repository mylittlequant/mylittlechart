//! Submit GPU render commands to wgpu surface, optional pipelined GPU thread variant.

use vello::AaConfig;
use vello::util::RenderContext;
use crate::{
    chrome,
    screenshot,
    PerWindowState,
};

/// Phase 2 of rendering: submit the built scene to the GPU and present.
///
/// Runs `render_to_texture`, handles screenshot captures, blits to the swap
/// chain surface, and calls `present()`.  Must run on the same thread that
/// owns the wgpu surface (GPU work is inherently sequential across windows).
///
/// Returns the GPU-submit wall-clock microseconds and sets `*close_all` on
/// catastrophic surface error.
///
/// Note: kept for reference. The pipelined path uses
/// [`submit_window_gpu_from_gpu_scene`] which renders `pw.gpu_scene` instead.
#[allow(dead_code)]
pub(crate) fn submit_window_gpu(pw: &mut PerWindowState, render_cx: &RenderContext, close_all: &mut bool, msaa_samples: u8) -> u64 {
    let width = pw.surface.config.width;
    let height = pw.surface.config.height;

    if width == 0 || height == 0 {
        return 0;
    }

    let dev_id = pw.surface.dev_id;
    let device = &render_cx.devices[dev_id].device;
    let queue = &render_cx.devices[dev_id].queue;

    let base_color = vello::peniko::color::AlphaColor::from_rgba8(0x13, 0x17, 0x22, 0xff);

    let t0 = std::time::Instant::now();

    pw.renderer
        .render_to_texture(
            device,
            queue,
            &pw.scene,
            &pw.surface.target_view,
            &vello::RenderParams {
                base_color,
                width,
                height,
                antialiasing_method: match msaa_samples {
                    0 => AaConfig::Area,
                    8 => AaConfig::Msaa8,
                    _ => AaConfig::Msaa16,
                },
            },
        )
        .expect("render failed");

    let render_tex_us = t0.elapsed().as_micros() as u64;

    // Screenshot capture
    if pw.screenshot_pending {
        pw.screenshot_pending = false;
        let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
        let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
        match screenshot::capture_screenshot(device, queue, &pw.surface, crop) {
            Some((pixels, img_width, img_height)) => {
                match arboard::Clipboard::new() {
                    Ok(mut clipboard) => {
                        let img = arboard::ImageData {
                            width: img_width as usize,
                            height: img_height as usize,
                            bytes: std::borrow::Cow::Borrowed(&pixels),
                        };
                        if let Err(e) = clipboard.set_image(img) {
                            eprintln!("[Screenshot] Clipboard error: {e}");
                        } else {
                            eprintln!("[Screenshot] Copied to clipboard");
                        }
                    }
                    Err(e) => eprintln!("[Screenshot] Failed to open clipboard: {e}"),
                }
                if let Some(png_bytes) = screenshot::encode_png(&pixels, img_width, img_height) {
                    let filename = format!("screenshot_{}.png", screenshot::timestamp_for_filename());
                    let path = screenshot::screenshot_save_dir().join(&filename);
                    match std::fs::write(&path, &png_bytes) {
                        Ok(_) => eprintln!("[Screenshot] Saved {} bytes to: {}", png_bytes.len(), path.display()),
                        Err(e) => eprintln!("[Screenshot] Failed to write file: {e}"),
                    }
                }
            }
            None => eprintln!("[Screenshot] Capture failed"),
        }
    }

    // Alert screenshot capture — attach PNG bytes to pending delivery events.
    if pw.chart.pending_alert_screenshot {
        pw.chart.pending_alert_screenshot = false;
        if !pw.chart.pending_delivery_events.is_empty() {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match screenshot::capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, img_width, img_height)) => {
                    match screenshot::encode_png(&pixels, img_width, img_height) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AlertScreenshot] Captured {}x{} PNG ({} bytes) for {} alert(s)",
                                img_width, img_height, png_bytes.len(),
                                pw.chart.pending_delivery_events.len()
                            );
                            for event in pw.chart.pending_delivery_events.iter_mut() {
                                event.screenshot = Some(png_bytes.clone());
                            }
                        }
                        None => eprintln!("[AlertScreenshot] PNG encoding failed"),
                    }
                }
                None => eprintln!("[AlertScreenshot] GPU capture failed"),
            }
        }
    }

    // Agent screenshot requests — respond to pending HTTP handler waiters.
    if !pw.pending_agent_screenshots.is_empty() {
        let agent_screenshots = std::mem::take(&mut pw.pending_agent_screenshots);
        for (chart_id, tx) in agent_screenshots {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match screenshot::capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, w, h)) => {
                    match screenshot::encode_png(&pixels, w, h) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AgentScreenshot] Captured {}x{} PNG ({} bytes) for chart {}",
                                w, h, png_bytes.len(), chart_id
                            );
                            let _ = tx.send(Ok(zengeld_server::state::ScreenshotData {
                                png_bytes,
                                width: w,
                                height: h,
                            }));
                        }
                        None => {
                            eprintln!("[AgentScreenshot] PNG encoding failed for chart {}", chart_id);
                            let _ = tx.send(Err("PNG encoding failed".to_string()));
                        }
                    }
                }
                None => {
                    eprintln!("[AgentScreenshot] GPU capture failed for chart {}", chart_id);
                    let _ = tx.send(Err("screenshot capture failed".to_string()));
                }
            }
        }
    }

    // Present
    let present_t0 = std::time::Instant::now();
    let surface_texture = match pw.surface.surface.get_current_texture() {
        Ok(t) => t,
        Err(vello::wgpu::SurfaceError::OutOfMemory) => {
            *close_all = true;
            return render_tex_us;
        }
        Err(_) => return render_tex_us,
    };

    let surface_view = surface_texture
        .texture
        .create_view(&vello::wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&vello::wgpu::CommandEncoderDescriptor {
        label: Some("blit"),
    });
    pw.surface
        .blitter
        .copy(device, &mut encoder, &pw.surface.target_view, &surface_view);
    queue.submit([encoder.finish()]);
    surface_texture.present();
    let present_us = present_t0.elapsed().as_micros() as u64;

    render_tex_us + present_us
}

/// Pipelined variant of [`submit_window_gpu`] used by the GPU render thread.
///
/// Identical to `submit_window_gpu` except that it renders `pw.gpu_scene`
/// instead of `pw.scene`.  `gpu_scene` holds the scene built by the main
/// thread during the *previous* frame; `scene` is the one the main thread is
/// building for the *current* frame while this function runs concurrently.
pub(crate) fn submit_window_gpu_from_gpu_scene(
    pw: &mut PerWindowState,
    render_cx: &RenderContext,
    close_all: &mut bool,
    msaa_samples: u8,
) -> u64 {
    let width = pw.surface.config.width;
    let height = pw.surface.config.height;

    if width == 0 || height == 0 {
        return 0;
    }

    let dev_id = pw.surface.dev_id;
    let device = &render_cx.devices[dev_id].device;
    let queue = &render_cx.devices[dev_id].queue;

    let t0 = std::time::Instant::now();

    use sidebar_content::state::RenderBackend as RB;
    let is_vello_gpu = pw.render_backend == RB::VelloGpu;
    let is_instanced = pw.render_backend == RB::InstancedWgpu;
    let is_cpu_backend = matches!(pw.render_backend, RB::VelloCpu | RB::TinySkia);
    let base_color = vello::peniko::color::AlphaColor::from_rgba8(0x13, 0x17, 0x22, 0xff);

    if is_vello_gpu {
        // ── VelloGpu: full scene rendered by vello ──────────────────────────
        pw.renderer
            .render_to_texture(
                device,
                queue,
                &pw.gpu_scene,
                &pw.surface.target_view,
                &vello::RenderParams {
                    base_color,
                    width,
                    height,
                    antialiasing_method: match msaa_samples {
                        0  => AaConfig::Area,
                        8  => AaConfig::Msaa8,
                        16 => AaConfig::Msaa16,
                        _  => AaConfig::Msaa8, // safe fallback
                    },
                },
            )
            .expect("render failed");
    } else if is_cpu_backend {
        // ── CPU backends: write full-screen pixels directly to target texture ──
        // The CPU context rendered everything (chrome + chart + overlays) so we
        // write the entire pixel buffer at origin (0,0).
        if !pw.gpu_cpu_chart_pixels.is_empty() {
            let (cw, ch) = pw.gpu_cpu_chart_dims;
            if cw > 0 && ch > 0 && cw == width && ch == height {
                queue.write_texture(
                    vello::wgpu::TexelCopyTextureInfo {
                        texture: &pw.surface.target_texture,
                        mip_level: 0,
                        origin: vello::wgpu::Origin3d::ZERO,
                        aspect: vello::wgpu::TextureAspect::All,
                    },
                    &pw.gpu_cpu_chart_pixels,
                    vello::wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * cw),
                        rows_per_image: Some(ch),
                    },
                    vello::wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
                );
            }
        }
    }
    // InstancedWgpu and VelloHybrid: no texture upload here — instanced renders
    // directly to the swapchain surface below, hybrid uses its own renderer.

    let render_tex_us = t0.elapsed().as_micros() as u64;

    // Screenshot capture
    if pw.screenshot_pending {
        pw.screenshot_pending = false;
        let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
        let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
        match screenshot::capture_screenshot(device, queue, &pw.surface, crop) {
            Some((pixels, img_width, img_height)) => {
                match arboard::Clipboard::new() {
                    Ok(mut clipboard) => {
                        let img = arboard::ImageData {
                            width: img_width as usize,
                            height: img_height as usize,
                            bytes: std::borrow::Cow::Borrowed(&pixels),
                        };
                        if let Err(e) = clipboard.set_image(img) {
                            eprintln!("[Screenshot] Clipboard error: {e}");
                        } else {
                            eprintln!("[Screenshot] Copied to clipboard");
                        }
                    }
                    Err(e) => eprintln!("[Screenshot] Failed to open clipboard: {e}"),
                }
                if let Some(png_bytes) = screenshot::encode_png(&pixels, img_width, img_height) {
                    let filename = format!("screenshot_{}.png", screenshot::timestamp_for_filename());
                    let path = screenshot::screenshot_save_dir().join(&filename);
                    match std::fs::write(&path, &png_bytes) {
                        Ok(_) => eprintln!("[Screenshot] Saved {} bytes to: {}", png_bytes.len(), path.display()),
                        Err(e) => eprintln!("[Screenshot] Failed to write file: {e}"),
                    }
                }
            }
            None => eprintln!("[Screenshot] Capture failed"),
        }
    }

    // Alert screenshot capture — attach PNG bytes to pending delivery events.
    if pw.chart.pending_alert_screenshot {
        pw.chart.pending_alert_screenshot = false;
        if !pw.chart.pending_delivery_events.is_empty() {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match screenshot::capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, img_width, img_height)) => {
                    match screenshot::encode_png(&pixels, img_width, img_height) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AlertScreenshot] Captured {}x{} PNG ({} bytes) for {} alert(s)",
                                img_width, img_height, png_bytes.len(),
                                pw.chart.pending_delivery_events.len()
                            );
                            for event in pw.chart.pending_delivery_events.iter_mut() {
                                event.screenshot = Some(png_bytes.clone());
                            }
                        }
                        None => eprintln!("[AlertScreenshot] PNG encoding failed"),
                    }
                }
                None => eprintln!("[AlertScreenshot] GPU capture failed"),
            }
        }
    }

    // Agent screenshot requests — respond to pending HTTP handler waiters.
    if !pw.pending_agent_screenshots.is_empty() {
        let agent_screenshots = std::mem::take(&mut pw.pending_agent_screenshots);
        for (chart_id, tx) in agent_screenshots {
            let (cx, cy, cw, ch) = pw.chart.screenshot_rect();
            let crop = Some((cx, cy + chrome::CHROME_HEIGHT as u32, cw, ch));
            match screenshot::capture_screenshot(device, queue, &pw.surface, crop) {
                Some((pixels, w, h)) => {
                    match screenshot::encode_png(&pixels, w, h) {
                        Some(png_bytes) => {
                            eprintln!(
                                "[AgentScreenshot] Captured {}x{} PNG ({} bytes) for chart {}",
                                w, h, png_bytes.len(), chart_id
                            );
                            let _ = tx.send(Ok(zengeld_server::state::ScreenshotData {
                                png_bytes,
                                width: w,
                                height: h,
                            }));
                        }
                        None => {
                            eprintln!("[AgentScreenshot] PNG encoding failed for chart {}", chart_id);
                            let _ = tx.send(Err("PNG encoding failed".to_string()));
                        }
                    }
                }
                None => {
                    eprintln!("[AgentScreenshot] GPU capture failed for chart {}", chart_id);
                    let _ = tx.send(Err("screenshot capture failed".to_string()));
                }
            }
        }
    }

    // Present
    let present_t0 = std::time::Instant::now();
    let surface_texture = match pw.surface.surface.get_current_texture() {
        Ok(t) => t,
        Err(vello::wgpu::SurfaceError::OutOfMemory) => {
            *close_all = true;
            return render_tex_us;
        }
        Err(e) => {
            eprintln!("[GPU] Surface error: {:?}, reconfiguring", e);
            pw.surface.surface.configure(device, &pw.surface.config);
            // Reconfiguring the surface rebuilds the swapchain, which bumps the
            // resource generation. The cached `target_texture`/`target_view` were
            // created against the old generation; vello's next frame would bind a
            // now-dead TextureView ("no longer alive", gen 7 vs 8) and panic the
            // GPU thread. Recreate the target so the next frame binds a live view.
            crate::screenshot::add_copy_src_to_target_texture(&mut pw.surface, device);
            return render_tex_us;
        }
    };

    let surface_view = surface_texture
        .texture
        .create_view(&vello::wgpu::TextureViewDescriptor::default());

    if is_vello_gpu || is_cpu_backend {
        // ── VelloGpu / CPU: blit target_texture → swapchain surface ──────────
        // For VelloGpu: vello rendered into target_view above.
        // For CPU: pixel buffer was written into target_texture above.
        let mut encoder = device.create_command_encoder(&vello::wgpu::CommandEncoderDescriptor {
            label: Some("blit"),
        });
        pw.surface
            .blitter
            .copy(device, &mut encoder, &pw.surface.target_view, &surface_view);
        queue.submit([encoder.finish()]);
    }

    if is_instanced {
        // ── Instanced backend: render all instances directly to swapchain ─────
        // Everything (chrome + chart + overlays) is in the instance buffers.
        // Use clear_color = background so the surface is cleared first.
        if pw.instanced_renderer.is_none() {
            pw.instanced_renderer = Some(
                uzor_render_wgpu_instanced::InstancedRenderer::new(
                    device,
                    queue,
                    surface_texture.texture.format(),
                )
            );
        }
        if let Some(ref mut inst_renderer) = pw.instanced_renderer {
            let clear = vello::wgpu::Color { r: 0.075, g: 0.09, b: 0.133, a: 1.0 };
            inst_renderer.render(
                device,
                queue,
                &surface_view,
                width,
                height,
                &pw.gpu_instanced_commands,
                Some(clear),  // LoadOp::Clear — full frame, no vello content underneath
                None,         // no scissor — everything is in instances
            );
        }
    }

    // ── VelloHybrid backend: render scene onto swapchain ─────────────────────
    if pw.render_backend == RB::VelloHybrid {
        if let Some(ref hybrid_ctx) = pw.gpu_hybrid_ctx {
            // Lazily create the vello_hybrid::Renderer.
            if pw.hybrid_renderer.is_none() {
                pw.hybrid_renderer = Some(vello_hybrid::Renderer::new(
                    device,
                    &vello_hybrid::RenderTargetConfig {
                        format: surface_texture.texture.format(),
                        width,
                        height,
                    },
                ));
            }
            if let Some(ref mut renderer) = pw.hybrid_renderer {
                let mut encoder = device.create_command_encoder(&vello::wgpu::CommandEncoderDescriptor {
                    label: Some("vello_hybrid"),
                });
                let _ = hybrid_ctx.render(renderer, device, queue, &mut encoder, &surface_view);
                queue.submit([encoder.finish()]);
            }
        }
    }

    surface_texture.present();
    let present_us = present_t0.elapsed().as_micros() as u64;

    render_tex_us + present_us
}
