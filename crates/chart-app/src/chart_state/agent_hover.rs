//! Agent panel hover routing — tracks which agent leaf the cursor is over
//! and updates per-leaf focus/hover state for chat and PTY modes.

use crate::{text_input, ChartApp};

impl ChartApp {
    pub fn check_agent_hover(&mut self, chart_x: f64, chart_y: f64) -> bool {
        use sidebar_content::state::RightSidebarPanel;

        let agents_open = self.sidebar_state.is_right_open()
            && self.sidebar_state.right_panel == RightSidebarPanel::Agents;

        if !agents_open {
            if self.agent_pty_hover_focused {
                self.agent_pty_hover_focused = false;
                return true;
            }
            return false;
        }

        let inside = self
            .sidebar_state
            .agent_terminal_rect
            .map(|(rx, ry, rw, rh)| {
                let rx = rx as f64;
                let ry = ry as f64;
                let rw = rw as f64;
                let rh = rh as f64;
                chart_x >= rx && chart_x < rx + rw && chart_y >= ry && chart_y < ry + rh
            })
            .unwrap_or(false);

        if inside != self.agent_pty_hover_focused {
            self.agent_pty_hover_focused = inside;
            // Focus PTY field on hover if focused leaf is in PTY mode.
            let is_pty_leaf = self.sidebar_state.focused_agent_leaf
                .and_then(|id| self.sidebar_state.agent_leaves.get(&id))
                .map(|d| d.mode == gate4agent::InstanceMode::Pty)
                .unwrap_or(false);
            if inside && is_pty_leaf {
                self.input_coordinator.borrow_mut().text_fields_mut().focus(text_input::AGENT_PTY);
            }
            // Do NOT blur on cursor-leave — blur only on click outside. Otherwise
            // any tiny mouse movement during typing steals PTY focus mid-keystroke.
            return true;
        }
        false
    }
}
