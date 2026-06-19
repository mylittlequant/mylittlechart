//! Agent API integration for the GPU host.
//!
//! Real snapshot + command logic lives in `mlc-headless-core` so the
//! headless backend uses the same code path. This module only wires
//! `App<'_>` into the `AgentHost` trait and forwards through.

use mlc_agentic_server::AgentState;
use mlc_agentic_server::state::ScreenshotData;
use mlc_headless_core::{AgentHost, commands, snapshots};
use mlc_headless_core::host::{WindowRef, WindowRefMut};

use crate::App;

impl AgentHost for App<'_> {
    fn windows(&self) -> Vec<WindowRef<'_>> {
        self.windows.values()
            .map(|pw| WindowRef { window_id: &pw.window_id, chart: &pw.chart })
            .collect()
    }

    fn windows_mut(&mut self) -> Vec<WindowRefMut<'_>> {
        self.windows.values_mut()
            .map(|pw| WindowRefMut { window_id: &pw.window_id, chart: &mut pw.chart })
            .collect()
    }

    fn request_screenshot(
        &mut self,
        window_id: &str,
        chart_id: u64,
        response_tx: tokio::sync::oneshot::Sender<Result<ScreenshotData, String>>,
    ) {
        if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
            pw.pending_agent_screenshots.push((chart_id, response_tx));
            pw.window.request_redraw();
            eprintln!("[AgentCommand] RequestScreenshot queued: window={}, chart={}", window_id, chart_id);
        } else {
            let _ = response_tx.send(Err(format!("window not found: {}", window_id)));
            eprintln!("[AgentCommand] RequestScreenshot: window not found: {}", window_id);
        }
    }
}

impl App<'_> {
    pub(crate) fn drain_agent_commands(&mut self) {
        let Some(agent_state) = self.agent_state.clone() else { return; };
        commands::drain_agent_commands(self, &agent_state);
    }

    pub(crate) fn update_terminal_snapshot(&self, agent_state: &AgentState) {
        snapshots::update_terminal_snapshot(self, agent_state);
    }

    pub(crate) fn update_indicator_snapshot(&self, agent_state: &AgentState) {
        snapshots::update_indicator_snapshot(self, agent_state);
    }

    pub(crate) fn update_watchlist_snapshot(&self, agent_state: &AgentState) {
        snapshots::update_watchlist_snapshot(self, agent_state);
    }

    pub(crate) fn update_connector_snapshot(&self, agent_state: &AgentState) {
        snapshots::update_connector_snapshot(agent_state);
    }
}
