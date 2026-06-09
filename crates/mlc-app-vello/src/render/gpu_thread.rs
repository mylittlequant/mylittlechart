//! Pipelined GPU render thread: command/done channels and thread spawn.

/// Sent from the main thread to the persistent GPU render thread.
pub(crate) enum GpuCommand {
    /// Submit the scenes currently in `pw.gpu_scene` for each address.
    /// Each entry is `(pw_addr, msaa_samples)`.
    Submit {
        window_addrs: Vec<usize>,
        msaa_samples: u8,
        render_cx_addr: usize,
    },
    /// Shut down the render thread cleanly.
    Shutdown,
}

/// Sent from the GPU render thread back to the main thread after a frame.
pub(crate) struct GpuDone {
    /// Set to `true` if any window returned an OOM error from the surface.
    pub(crate) close_all: bool,
    /// Wall-clock µs spent on GPU submit across all windows.
    pub(crate) total_gpu_us: u64,
    /// µs spent in vello `render_to_texture` (compute encode + submit) — the
    /// scene-rendering half of the GPU work.
    pub(crate) render_tex_us: u64,
    /// µs spent in `get_current_texture` + blit + `present` — the swapchain
    /// acquire/present half. With AutoNoVsync this should be small; a large
    /// value points at swapchain back-pressure (GPU queue depth).
    pub(crate) present_us: u64,
}
