//! `AgentHost` — abstract surface used by snapshot + command code so the
//! same plumbing serves both the GPU renderer (`mlc-app-shell-vello`) and the
//! upcoming headless backend.

use mlc_app::ChartApp;
use mlc_backend_shell_http::state::ScreenshotData;

/// One window owned by the host.
pub struct WindowRef<'a> {
    pub window_id: &'a str,
    pub chart: &'a ChartApp,
}

/// One window owned by the host, mutably.
pub struct WindowRefMut<'a> {
    pub window_id: &'a str,
    pub chart: &'a mut ChartApp,
}

/// Surface the snapshot builders + command dispatcher rely on.
///
/// Implementors must provide:
///
/// - `windows` / `windows_mut` — iterate the host's open windows. Each
///   window is `(window_id, ChartApp)`. The GPU host has many; the headless
///   host has one (for now).
/// - `request_screenshot` — schedule a chart screenshot. The GPU host
///   pushes into its pending queue + asks winit for a redraw. The headless
///   host short-circuits with an error.
pub trait AgentHost {
    fn windows(&self) -> Vec<WindowRef<'_>>;
    fn windows_mut(&mut self) -> Vec<WindowRefMut<'_>>;

    /// Default: not supported. Override in renderer hosts.
    fn request_screenshot(
        &mut self,
        _window_id: &str,
        _chart_id: u64,
        response_tx: tokio::sync::oneshot::Sender<Result<ScreenshotData, String>>,
    ) {
        let _ = response_tx.send(Err("screenshot not supported in this host".to_string()));
    }
}
