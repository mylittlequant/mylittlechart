use gate4agent::pty::PtySession;
use gate4agent::pipe::{PipeSession, PipeProcessOptions};
use gate4agent::{AgentEvent, SessionConfig};
use tokio::sync::broadcast;

use super::snapshot::{
    AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid,
};

/// Convert a `vt100::Color` to an `[u8; 3]` RGB triple.
///
/// `vt100::Color::Default` maps to a fallback color chosen by the caller.
/// `vt100::Color::Idx` maps 8-bit ANSI indices to standard RGB values.
/// `vt100::Color::Rgb` passes through directly.
fn vt100_color_to_rgb(color: vt100::Color, default_rgb: [u8; 3]) -> [u8; 3] {
    match color {
        vt100::Color::Default => default_rgb,
        vt100::Color::Rgb(r, g, b) => [r, g, b],
        vt100::Color::Idx(idx) => ansi_idx_to_rgb(idx),
    }
}

/// Map a standard ANSI 256-color index to an approximate RGB triple.
fn ansi_idx_to_rgb(idx: u8) -> [u8; 3] {
    // Standard 16 colors (0-15)
    const STANDARD_16: [[u8; 3]; 16] = [
        [0, 0, 0],       // 0 black
        [128, 0, 0],     // 1 dark red
        [0, 128, 0],     // 2 dark green
        [128, 128, 0],   // 3 dark yellow
        [0, 0, 128],     // 4 dark blue
        [128, 0, 128],   // 5 dark magenta
        [0, 128, 128],   // 6 dark cyan
        [192, 192, 192], // 7 light gray
        [128, 128, 128], // 8 dark gray
        [255, 0, 0],     // 9 bright red
        [0, 255, 0],     // 10 bright green
        [255, 255, 0],   // 11 bright yellow
        [0, 0, 255],     // 12 bright blue
        [255, 0, 255],   // 13 bright magenta
        [0, 255, 255],   // 14 bright cyan
        [255, 255, 255], // 15 white
    ];
    if (idx as usize) < STANDARD_16.len() {
        return STANDARD_16[idx as usize];
    }
    // 6x6x6 color cube: indices 16-231
    if idx >= 16 && idx <= 231 {
        let v = idx - 16;
        let b = v % 6;
        let g = (v / 6) % 6;
        let r = v / 36;
        let to_u8 = |x: u8| if x == 0 { 0 } else { 55 + x * 40 };
        return [to_u8(r), to_u8(g), to_u8(b)];
    }
    // Grayscale: indices 232-255
    let gray = 8 + (idx - 232) * 10;
    [gray, gray, gray]
}

/// Manages a single agent session (either PTY or Pipe mode).
///
/// Designed for use in a synchronous render loop. Call `drain_events` every
/// frame to process incoming events. Call `snapshot` to obtain a
/// clone-safe rendering snapshot with no OS handles.
///
/// Session spawning is async and requires an active Tokio runtime.
/// Use `tokio::runtime::Handle::current().block_on(...)` or call from an
/// async context.
pub struct AgentSessionManager {
    pty_session: Option<PtySession>,
    pipe_session: Option<PipeSession>,
    pty_rx: Option<broadcast::Receiver<AgentEvent>>,
    pipe_rx: Option<broadcast::Receiver<AgentEvent>>,
    pty_parser: vt100::Parser,
    chat_messages: Vec<ChatMessage>,
    cols: u16,
    rows: u16,
    session_active: bool,
}

impl AgentSessionManager {
    pub fn new() -> Self {
        let cols: u16 = 80;
        let rows: u16 = 24;
        Self {
            pty_session: None,
            pipe_session: None,
            pty_rx: None,
            pipe_rx: None,
            pty_parser: vt100::Parser::new(rows, cols, 0),
            chat_messages: Vec::new(),
            cols,
            rows,
            session_active: false,
        }
    }

    /// Start a PTY session using the given `SessionConfig`.
    ///
    /// Must be called from within a Tokio async context (e.g. via
    /// `Handle::current().block_on`).
    pub async fn start_pty(&mut self, config: SessionConfig) -> Result<(), String> {
        if self.session_active {
            return Err("Session already active".into());
        }
        match PtySession::spawn_with_size(config, self.rows, self.cols).await {
            Ok(session) => {
                self.pty_rx = Some(session.subscribe());
                // Reset parser for new session
                self.pty_parser = vt100::Parser::new(self.rows, self.cols, 0);
                self.pty_session = Some(session);
                self.session_active = true;
                Ok(())
            }
            Err(e) => Err(format!("Failed to spawn PTY: {}", e)),
        }
    }

    /// Start a Pipe/Chat session using the given `SessionConfig` and initial prompt.
    ///
    /// Must be called from within a Tokio async context.
    pub async fn start_pipe(
        &mut self,
        config: SessionConfig,
        prompt: &str,
    ) -> Result<(), String> {
        if self.session_active {
            return Err("Session already active".into());
        }
        self.chat_messages.push(ChatMessage {
            role: ChatRole::User,
            content: prompt.to_string(),
            tool_name: None,
        });
        match PipeSession::spawn(config, prompt, PipeProcessOptions::default()).await {
            Ok(session) => {
                self.pipe_rx = Some(session.subscribe());
                self.pipe_session = Some(session);
                self.session_active = true;
                Ok(())
            }
            Err(e) => Err(format!("Failed to spawn pipe: {}", e)),
        }
    }

    /// Drain events from the active session. Call every frame.
    pub fn drain_events(&mut self) {
        // Drain PTY events
        if let Some(ref mut rx) = self.pty_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => match event {
                        AgentEvent::PtyRaw { data } => {
                            self.pty_parser.process(data.as_bytes());
                        }
                        AgentEvent::Exited { .. } => {
                            self.session_active = false;
                        }
                        _ => {}
                    },
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        self.session_active = false;
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                }
            }
        }

        // Drain Pipe events
        if let Some(ref mut rx) = self.pipe_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => match event {
                        AgentEvent::PipeText { text, .. } => {
                            if let Some(last) = self.chat_messages.last_mut() {
                                if last.role == ChatRole::Assistant {
                                    last.content.push_str(&text);
                                    continue;
                                }
                            }
                            self.chat_messages.push(ChatMessage {
                                role: ChatRole::Assistant,
                                content: text,
                                tool_name: None,
                            });
                        }
                        AgentEvent::PipeToolStart { name, .. } => {
                            self.chat_messages.push(ChatMessage {
                                role: ChatRole::Tool,
                                content: format!("Running {}...", name),
                                tool_name: Some(name),
                            });
                        }
                        AgentEvent::PipeToolResult { id: _, output: _, is_error: _, .. } => {
                            // Tool result arrived — mark the tool message as done if present
                            if let Some(last) = self.chat_messages.last_mut() {
                                if last.role == ChatRole::Tool {
                                    if let Some(ref name) = last.tool_name.clone() {
                                        last.content = format!("{}: done", name);
                                    }
                                }
                            }
                        }
                        AgentEvent::PipeThinking { text } => {
                            self.chat_messages.push(ChatMessage {
                                role: ChatRole::Thinking,
                                content: text,
                                tool_name: None,
                            });
                        }
                        AgentEvent::Error { message } => {
                            self.chat_messages.push(ChatMessage {
                                role: ChatRole::Error,
                                content: message,
                                tool_name: None,
                            });
                        }
                        AgentEvent::PipeTurnComplete { .. } => {
                            // Turn done — ready for next input
                        }
                        AgentEvent::Exited { .. } => {
                            self.session_active = false;
                        }
                        _ => {}
                    },
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        self.session_active = false;
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                }
            }
        }
    }

    /// Write a string to the active PTY session. Must be called from async context.
    pub async fn write_pty(&self, text: &str) -> Result<(), String> {
        if let Some(ref session) = self.pty_session {
            session
                .write(text)
                .await
                .map_err(|e| format!("PTY write error: {}", e))
        } else {
            Err("No active PTY session".into())
        }
    }

    /// Send a chat prompt to the active pipe session. Must be called from async context.
    pub async fn send_chat(&mut self, prompt: &str) -> Result<(), String> {
        self.chat_messages.push(ChatMessage {
            role: ChatRole::User,
            content: prompt.to_string(),
            tool_name: None,
        });
        if let Some(ref session) = self.pipe_session {
            session
                .send_prompt(prompt)
                .await
                .map_err(|e| format!("Pipe send error: {}", e))
        } else {
            Err("No active pipe session".into())
        }
    }

    /// Stop the current session. Must be called from async context.
    pub async fn stop(&mut self) {
        if let Some(session) = self.pty_session.take() {
            let _ = session.kill().await;
        }
        if let Some(session) = self.pipe_session.take() {
            let _ = session.kill().await;
        }
        self.pty_rx = None;
        self.pipe_rx = None;
        self.session_active = false;
    }

    /// Resize the PTY terminal. Must be called from async context.
    pub async fn resize(&mut self, cols: u16, rows: u16) {
        if self.cols == cols && self.rows == rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.pty_parser.set_size(rows, cols);
        if let Some(ref session) = self.pty_session {
            // resize takes (rows, cols) per the PTY convention
            let _ = session.resize(rows, cols).await;
        }
    }

    /// Build a snapshot of the current state for rendering.
    ///
    /// No OS handles are included — safe to clone and send to a render thread.
    pub fn snapshot(&self) -> AgentRenderSnapshot {
        if !self.session_active {
            return AgentRenderSnapshot {
                mode: AgentSnapshotMode::Idle,
                session_active: false,
            };
        }

        if self.pty_session.is_some() {
            let screen = self.pty_parser.screen();
            let mut grid = TermGrid::empty(self.cols, self.rows);
            for row in 0..self.rows {
                for col in 0..self.cols {
                    if let Some(cell) = screen.cell(row, col) {
                        let fg =
                            vt100_color_to_rgb(cell.fgcolor(), [204, 204, 204]);
                        let bg = vt100_color_to_rgb(cell.bgcolor(), [0, 0, 0]);
                        grid.cells[row as usize][col as usize] = TermCell {
                            ch: cell.contents().chars().next().unwrap_or(' '),
                            fg,
                            bg,
                            bold: cell.bold(),
                        };
                    }
                }
            }
            let (cur_row, cur_col) = screen.cursor_position();
            grid.cursor_row = cur_row;
            grid.cursor_col = cur_col;
            AgentRenderSnapshot {
                mode: AgentSnapshotMode::Pty(grid),
                session_active: true,
            }
        } else {
            AgentRenderSnapshot {
                mode: AgentSnapshotMode::Chat(self.chat_messages.clone()),
                session_active: true,
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.session_active
    }
}

impl Default for AgentSessionManager {
    fn default() -> Self {
        Self::new()
    }
}
