use std::fs::{self, OpenOptions};
use std::io::Write as IoWrite;
use std::path::PathBuf;

use gate4agent::pty::PtySession;
use gate4agent::pipe::{PipeSession, PipeProcessOptions};
use gate4agent::{AgentEvent, SessionConfig, CliTool};
use sidebar_content::state::AgentCli;
use tokio::sync::broadcast;

use super::snapshot::{
    AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid,
};

// =============================================================================
// Color helpers
// =============================================================================

/// Convert a `vt100::Color` to an `[u8; 3]` RGB triple.
fn vt100_color_to_rgb(color: vt100::Color, default_rgb: [u8; 3]) -> [u8; 3] {
    match color {
        vt100::Color::Default => default_rgb,
        vt100::Color::Rgb(r, g, b) => [r, g, b],
        vt100::Color::Idx(idx) => ansi_idx_to_rgb(idx),
    }
}

/// Map a standard ANSI 256-color index to an approximate RGB triple.
fn ansi_idx_to_rgb(idx: u8) -> [u8; 3] {
    const STANDARD_16: [[u8; 3]; 16] = [
        [0, 0, 0],
        [128, 0, 0],
        [0, 128, 0],
        [128, 128, 0],
        [0, 0, 128],
        [128, 0, 128],
        [0, 128, 128],
        [192, 192, 192],
        [128, 128, 128],
        [255, 0, 0],
        [0, 255, 0],
        [255, 255, 0],
        [0, 0, 255],
        [255, 0, 255],
        [0, 255, 255],
        [255, 255, 255],
    ];
    if (idx as usize) < STANDARD_16.len() {
        return STANDARD_16[idx as usize];
    }
    if idx >= 16 && idx <= 231 {
        let v = idx - 16;
        let b = v % 6;
        let g = (v / 6) % 6;
        let r = v / 36;
        let to_u8 = |x: u8| if x == 0 { 0 } else { 55 + x * 40 };
        return [to_u8(r), to_u8(g), to_u8(b)];
    }
    let gray = 8 + (idx - 232) * 10;
    [gray, gray, gray]
}

// =============================================================================
// Per-CLI state
// =============================================================================

/// State for a single CLI (Claude, Codex, or Gemini).
pub struct PerCliState {
    pub cli: AgentCli,
    pty_session: Option<PtySession>,
    pipe_session: Option<PipeSession>,
    pty_rx: Option<broadcast::Receiver<AgentEvent>>,
    pipe_rx: Option<broadcast::Receiver<AgentEvent>>,
    pty_parser: vt100::Parser,
    pub chat_messages: Vec<ChatMessage>,
    pub session_active: bool,
    pub pipe_session_id: Option<String>,
    /// Past session IDs found on disk at startup (newest first).
    pub past_sessions: Vec<String>,
    /// Index into `past_sessions` for the debug picker (cycles on button click).
    pub past_session_view_idx: usize,
}

impl PerCliState {
    fn new(cli: AgentCli, rows: u16, cols: u16) -> Self {
        Self {
            cli,
            pty_session: None,
            pipe_session: None,
            pty_rx: None,
            pipe_rx: None,
            pty_parser: vt100::Parser::new(rows, cols, 0),
            chat_messages: Vec::new(),
            session_active: false,
            pipe_session_id: None,
            past_sessions: Vec::new(),
            past_session_view_idx: 0,
        }
    }

    fn cli_name(&self) -> &'static str {
        match self.cli {
            AgentCli::Claude => "claude",
            AgentCli::Codex  => "codex",
            AgentCli::Gemini => "gemini",
        }
    }
}

// =============================================================================
// Persistence helpers
// =============================================================================

/// Returns `%APPDATA%/zengeld` on Windows, or the platform equivalent.
fn agent_data_dir() -> PathBuf {
    // Prefer the dirs crate path; fall back to a relative path.
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("zengeld");
        }
    }
    PathBuf::from("data").join("zengeld")
}

fn session_dir(cli_name: &str, session_id: &str) -> PathBuf {
    agent_data_dir()
        .join("agent-sessions")
        .join(cli_name)
        .join(session_id)
}

/// Append a single `ChatMessage` as a JSON line to the session's messages.ndjson.
fn persist_message(cli_name: &str, session_id: &str, msg: &ChatMessage) {
    let dir = session_dir(cli_name, session_id);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("[AgentManager] create_dir_all failed: {}", e);
        return;
    }
    let path = dir.join("messages.ndjson");
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            match serde_json::to_string(msg) {
                Ok(line) => {
                    let _ = writeln!(f, "{}", line);
                }
                Err(e) => eprintln!("[AgentManager] JSON serialize error: {}", e),
            }
        }
        Err(e) => eprintln!("[AgentManager] Failed to open messages.ndjson: {}", e),
    }
}

/// Scan disk for existing session directories for this CLI and return IDs
/// sorted newest-first (by directory modification time if available,
/// otherwise lexicographic descending).
fn load_past_session_ids(cli_name: &str) -> Vec<String> {
    let base = agent_data_dir().join("agent-sessions").join(cli_name);
    let entries = match fs::read_dir(&base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut ids: Vec<(std::time::SystemTime, String)> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let mtime = e.metadata().ok()?.modified().ok()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            Some((mtime, name))
        })
        .collect();
    ids.sort_by(|a, b| b.0.cmp(&a.0)); // newest first
    ids.into_iter().map(|(_, name)| name).collect()
}

/// Load all messages from a past session's messages.ndjson into a Vec.
pub fn load_session_messages(cli_name: &str, session_id: &str) -> Vec<ChatMessage> {
    let path = session_dir(cli_name, session_id).join("messages.ndjson");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

// =============================================================================
// Manager
// =============================================================================

/// Manages agent sessions for all 3 CLIs simultaneously.
///
/// Each CLI has independent PTY and pipe sessions. Call `drain_events` every
/// frame. Call `snapshot(cli)` to get rendering state for the active CLI.
pub struct AgentSessionManager {
    states: [PerCliState; 3],
    cols: u16,
    rows: u16,
}

impl AgentSessionManager {
    pub fn new() -> Self {
        let cols: u16 = 80;
        let rows: u16 = 24;
        let mut mgr = Self {
            states: [
                PerCliState::new(AgentCli::Claude, rows, cols),
                PerCliState::new(AgentCli::Codex,  rows, cols),
                PerCliState::new(AgentCli::Gemini, rows, cols),
            ],
            cols,
            rows,
        };
        // Scan disk for past sessions at startup.
        for state in &mut mgr.states {
            state.past_sessions = load_past_session_ids(state.cli_name());
        }
        mgr
    }

    fn idx(cli: AgentCli) -> usize {
        match cli {
            AgentCli::Claude => 0,
            AgentCli::Codex  => 1,
            AgentCli::Gemini => 2,
        }
    }

    fn state(&self, cli: AgentCli) -> &PerCliState {
        &self.states[Self::idx(cli)]
    }

    fn state_mut(&mut self, cli: AgentCli) -> &mut PerCliState {
        &mut self.states[Self::idx(cli)]
    }

    // =========================================================================
    // Autostart
    // =========================================================================

    /// Spawn a PTY session for each of the 3 CLIs at app launch.
    ///
    /// Failures are logged but do not abort — the app starts normally without
    /// the affected CLI.
    pub async fn autostart_all(&mut self) {
        for cli in [AgentCli::Claude, AgentCli::Codex, AgentCli::Gemini] {
            let tool = match cli {
                AgentCli::Claude => CliTool::ClaudeCode,
                AgentCli::Codex  => CliTool::Codex,
                AgentCli::Gemini => CliTool::Gemini,
            };
            let config = SessionConfig { tool, ..SessionConfig::default() };
            if let Err(e) = self.start_pty(cli, config).await {
                eprintln!("[AgentManager] autostart PTY {:?} failed: {}", cli, e);
            }
        }
    }

    // =========================================================================
    // Session lifecycle
    // =========================================================================

    /// Start a PTY session for the given CLI.
    pub async fn start_pty(&mut self, cli: AgentCli, config: SessionConfig) -> Result<(), String> {
        let rows = self.rows;
        let cols = self.cols;
        let st = self.state_mut(cli);
        if st.session_active {
            return Err(format!("{:?} session already active", cli));
        }
        match PtySession::spawn_with_size(config, rows, cols).await {
            Ok(session) => {
                st.pty_rx = Some(session.subscribe());
                st.pty_parser = vt100::Parser::new(rows, cols, 0);
                st.pty_session = Some(session);
                st.session_active = true;
                Ok(())
            }
            Err(e) => Err(format!("Failed to spawn PTY for {:?}: {}", cli, e)),
        }
    }

    /// Start a Pipe/Chat session for the given CLI.
    pub async fn start_pipe(
        &mut self,
        cli: AgentCli,
        config: SessionConfig,
        prompt: &str,
    ) -> Result<(), String> {
        let cli_name = match cli {
            AgentCli::Claude => "claude",
            AgentCli::Codex  => "codex",
            AgentCli::Gemini => "gemini",
        };
        let st = self.state_mut(cli);
        if st.session_active {
            return Err(format!("{:?} session already active", cli));
        }
        let msg = ChatMessage {
            role: ChatRole::User,
            content: prompt.to_string(),
            tool_name: None,
        };
        st.chat_messages.push(msg.clone());
        // Persist user message under a pending session id
        let pending_id = "pending".to_string();
        persist_message(cli_name, &pending_id, &msg);
        match PipeSession::spawn(config, prompt, PipeProcessOptions::default()).await {
            Ok(session) => {
                st.pipe_rx = Some(session.subscribe());
                st.pipe_session = Some(session);
                st.session_active = true;
                Ok(())
            }
            Err(e) => Err(format!("Failed to spawn pipe for {:?}: {}", cli, e)),
        }
    }

    /// Stop the session for a single CLI.
    pub async fn stop(&mut self, cli: AgentCli) {
        let st = self.state_mut(cli);
        if let Some(session) = st.pty_session.take() {
            let _ = session.kill().await;
        }
        if let Some(session) = st.pipe_session.take() {
            let _ = session.kill().await;
        }
        st.pty_rx = None;
        st.pipe_rx = None;
        st.session_active = false;
    }

    /// Stop all active sessions (called on app shutdown).
    pub async fn stop_all(&mut self) {
        for cli in [AgentCli::Claude, AgentCli::Codex, AgentCli::Gemini] {
            self.stop(cli).await;
        }
    }

    // =========================================================================
    // I/O
    // =========================================================================

    /// Write a string to the active PTY for the given CLI.
    pub async fn write_pty(&self, cli: AgentCli, text: &str) -> Result<(), String> {
        let st = self.state(cli);
        if let Some(ref session) = st.pty_session {
            session
                .write(text)
                .await
                .map_err(|e| format!("PTY write error ({:?}): {}", cli, e))
        } else {
            Err(format!("No active PTY session for {:?}", cli))
        }
    }

    /// Send a chat prompt to the active pipe session for the given CLI.
    pub async fn send_chat(&mut self, cli: AgentCli, prompt: &str) -> Result<(), String> {
        let cli_name = match cli {
            AgentCli::Claude => "claude",
            AgentCli::Codex  => "codex",
            AgentCli::Gemini => "gemini",
        };
        let msg = ChatMessage {
            role: ChatRole::User,
            content: prompt.to_string(),
            tool_name: None,
        };
        {
            let st = self.state_mut(cli);
            st.chat_messages.push(msg.clone());
            let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
            persist_message(cli_name, &sid, &msg);
        }
        let st = self.state(cli);
        if let Some(ref session) = st.pipe_session {
            session
                .send_prompt(prompt)
                .await
                .map_err(|e| format!("Pipe send error ({:?}): {}", cli, e))
        } else {
            Err(format!("No active pipe session for {:?}", cli))
        }
    }

    // =========================================================================
    // Resize
    // =========================================================================

    /// Resize all active PTY sessions to the new dimensions.
    pub async fn resize(&mut self, cols: u16, rows: u16) {
        if self.cols == cols && self.rows == rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        for st in &mut self.states {
            st.pty_parser.set_size(rows, cols);
            if let Some(ref session) = st.pty_session {
                let _ = session.resize(rows, cols).await;
            }
        }
    }

    // =========================================================================
    // Drain events (call every frame)
    // =========================================================================

    /// Drain events for all 3 CLIs. Returns `true` if any events were processed.
    pub fn drain_events(&mut self) -> bool {
        let mut had_events = false;
        for i in 0..3 {
            had_events |= Self::drain_one(&mut self.states[i]);
        }
        had_events
    }

    fn drain_one(st: &mut PerCliState) -> bool {
        let mut had_events = false;
        let cli_name = st.cli_name();

        // Drain PTY events
        if let Some(ref mut rx) = st.pty_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => {
                        had_events = true;
                        match event {
                            AgentEvent::PtyRaw { data } => {
                                st.pty_parser.process(&data);
                            }
                            AgentEvent::Exited { .. } => {
                                st.session_active = false;
                            }
                            _ => {}
                        }
                    }
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        st.session_active = false;
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                }
            }
        }

        // Drain Pipe events
        if let Some(ref mut rx) = st.pipe_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => {
                        had_events = true;
                        match event {
                            AgentEvent::PipeSessionStart { session_id, model, .. } => {
                                // Rename "pending" session dir to real session id
                                let pending_dir = session_dir(cli_name, "pending");
                                let real_dir = session_dir(cli_name, &session_id);
                                if pending_dir.exists() && !real_dir.exists() {
                                    let _ = fs::rename(&pending_dir, &real_dir);
                                }
                                st.pipe_session_id = Some(session_id.clone());
                                // Re-scan past sessions so picker stays current
                                st.past_sessions = load_past_session_ids(cli_name);

                                let msg = ChatMessage {
                                    role: ChatRole::Tool,
                                    content: format!(
                                        "{} · session {}",
                                        model,
                                        &session_id[..session_id.len().min(8)]
                                    ),
                                    tool_name: None,
                                };
                                let sid = session_id.clone();
                                persist_message(cli_name, &sid, &msg);
                                st.chat_messages.push(msg);
                            }
                            AgentEvent::PipeText { text, is_delta } => {
                                let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
                                if is_delta {
                                    if let Some(last) = st.chat_messages.last_mut() {
                                        if last.role == ChatRole::Assistant {
                                            last.content.push_str(&text);
                                            continue;
                                        }
                                    }
                                    let msg = ChatMessage {
                                        role: ChatRole::Assistant,
                                        content: text,
                                        tool_name: None,
                                    };
                                    persist_message(cli_name, &sid, &msg);
                                    st.chat_messages.push(msg);
                                } else {
                                    let msg = ChatMessage {
                                        role: ChatRole::Assistant,
                                        content: text,
                                        tool_name: None,
                                    };
                                    persist_message(cli_name, &sid, &msg);
                                    st.chat_messages.push(msg);
                                }
                            }
                            AgentEvent::PipeToolStart { name, .. } => {
                                let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
                                let msg = ChatMessage {
                                    role: ChatRole::Tool,
                                    content: format!("Running {}...", name),
                                    tool_name: Some(name),
                                };
                                persist_message(cli_name, &sid, &msg);
                                st.chat_messages.push(msg);
                            }
                            AgentEvent::PipeToolResult { id: _, output: _, is_error: _, .. } => {
                                let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
                                if let Some(last) = st.chat_messages.last_mut() {
                                    if last.role == ChatRole::Tool {
                                        if let Some(ref name) = last.tool_name.clone() {
                                            last.content = format!("{}: done", name);
                                            persist_message(cli_name, &sid, last);
                                        }
                                    }
                                }
                            }
                            AgentEvent::PipeThinking { text } => {
                                let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
                                let msg = ChatMessage {
                                    role: ChatRole::Thinking,
                                    content: text,
                                    tool_name: None,
                                };
                                persist_message(cli_name, &sid, &msg);
                                st.chat_messages.push(msg);
                            }
                            AgentEvent::Error { message } => {
                                let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
                                let msg = ChatMessage {
                                    role: ChatRole::Error,
                                    content: message,
                                    tool_name: None,
                                };
                                persist_message(cli_name, &sid, &msg);
                                st.chat_messages.push(msg);
                            }
                            AgentEvent::PipeTurnComplete { input_tokens, output_tokens } => {
                                let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
                                let msg = ChatMessage {
                                    role: ChatRole::Tool,
                                    content: format!(
                                        "in {} / out {} tokens",
                                        input_tokens, output_tokens
                                    ),
                                    tool_name: None,
                                };
                                persist_message(cli_name, &sid, &msg);
                                st.chat_messages.push(msg);
                            }
                            AgentEvent::PipeSessionEnd { result, is_error, .. } => {
                                let sid = st.pipe_session_id.as_deref().unwrap_or("pending").to_string();
                                let status = if is_error { "error" } else { "done" };
                                let msg = ChatMessage {
                                    role: ChatRole::Tool,
                                    content: format!("Session {} · {}", status, result),
                                    tool_name: None,
                                };
                                persist_message(cli_name, &sid, &msg);
                                st.chat_messages.push(msg);
                            }
                            AgentEvent::Exited { .. } => {
                                st.session_active = false;
                            }
                            _ => {}
                        }
                    }
                    Err(broadcast::error::TryRecvError::Empty) => break,
                    Err(broadcast::error::TryRecvError::Closed) => {
                        st.session_active = false;
                        break;
                    }
                    Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                }
            }
        }

        had_events
    }

    // =========================================================================
    // Snapshot + query
    // =========================================================================

    /// Build a render snapshot for the given CLI.
    pub fn snapshot(&self, cli: AgentCli) -> AgentRenderSnapshot {
        let st = self.state(cli);

        if !st.session_active {
            if !st.chat_messages.is_empty() {
                return AgentRenderSnapshot {
                    mode: AgentSnapshotMode::Chat(st.chat_messages.clone()),
                    session_active: false,
                };
            }
            return AgentRenderSnapshot {
                mode: AgentSnapshotMode::Idle,
                session_active: false,
            };
        }

        if st.pty_session.is_some() {
            let screen = st.pty_parser.screen();
            let mut grid = TermGrid::empty(self.cols, self.rows);
            for row in 0..self.rows {
                for col in 0..self.cols {
                    if let Some(cell) = screen.cell(row, col) {
                        let fg = vt100_color_to_rgb(cell.fgcolor(), [204, 204, 204]);
                        let bg = vt100_color_to_rgb(cell.bgcolor(), [0, 0, 0]);
                        let contents = cell.contents();
                        grid.cells[row as usize][col as usize] = TermCell {
                            ch: if contents.is_empty() {
                                " ".to_string()
                            } else {
                                contents
                            },
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
                mode: AgentSnapshotMode::Chat(st.chat_messages.clone()),
                session_active: true,
            }
        }
    }

    /// Returns `true` if either PTY or pipe session is alive for this CLI.
    pub fn is_active(&self, cli: AgentCli) -> bool {
        self.state(cli).session_active
    }

    /// Returns `true` if any CLI has an active session.
    pub fn any_active(&self) -> bool {
        self.states.iter().any(|s| s.session_active)
    }

    /// Returns the count of past sessions for the given CLI.
    pub fn past_session_count(&self, cli: AgentCli) -> usize {
        self.state(cli).past_sessions.len()
    }

    /// Cycle to the next past session for `cli` and load its messages.
    /// Returns `true` if a session was loaded.
    pub fn load_next_past_session(&mut self, cli: AgentCli) -> bool {
        let cli_name = match cli {
            AgentCli::Claude => "claude",
            AgentCli::Codex  => "codex",
            AgentCli::Gemini => "gemini",
        };
        let st = self.state_mut(cli);
        if st.past_sessions.is_empty() {
            return false;
        }
        let idx = st.past_session_view_idx % st.past_sessions.len();
        let session_id = st.past_sessions[idx].clone();
        let messages = load_session_messages(cli_name, &session_id);
        if !messages.is_empty() {
            st.chat_messages = messages;
        }
        st.past_session_view_idx = (idx + 1) % st.past_sessions.len();
        true
    }
}

impl Default for AgentSessionManager {
    fn default() -> Self {
        Self::new()
    }
}
