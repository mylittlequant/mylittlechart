//! No-op stub for gate4agent types on wasm32.
//!
//! gate4agent has hard non-wasm deps (portable-pty, vt100, vte, rusqlite,
//! tokio rt-multi-thread). On wasm32 the real crate is not compiled in;
//! this module provides the identical public surface so all mlc-app call
//! sites compile without change.
//!
//! Methods return `Err("unsupported on wasm")` for fallible calls and
//! sensible defaults for everything else.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

// ── InstanceId ─────────────────────────────────────────────────────────────

static INSTANCE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Opaque identifier for a per-instance agent session (wasm stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceId(pub u64);

impl InstanceId {
    pub fn new() -> Self {
        Self(INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for InstanceId {
    fn default() -> Self {
        Self::new()
    }
}

// ── InstanceMode ───────────────────────────────────────────────────────────

/// Operating mode for an agent instance (wasm stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceMode {
    Pty,
    Chat,
}

// ── AgentCli ───────────────────────────────────────────────────────────────

/// Which AI CLI agent to use (wasm stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentCli {
    Claude,
    Codex,
    Gemini,
    OpenCode,
}

impl AgentCli {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentCli::Claude => "claude",
            AgentCli::Codex => "codex",
            AgentCli::Gemini => "gemini",
            AgentCli::OpenCode => "opencode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AgentCli::Claude => "Claude",
            AgentCli::Codex => "Codex",
            AgentCli::Gemini => "Gemini",
            AgentCli::OpenCode => "OpenCode",
        }
    }
}

// ── CliCapabilities + supporting types ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub is_default: bool,
    pub is_free_tier: bool,
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionModeInfo {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CliFeatures {
    pub thinking: bool,
    pub effort_control: bool,
    pub mcp: bool,
    pub resume: bool,
    pub continue_last: bool,
    pub allowed_tools_filter: bool,
    pub system_prompt_injection: bool,
    pub max_turns: bool,
    pub sandbox_mode: bool,
    pub ide_context: bool,
    pub plan_mode: bool,
    pub speed_toggle: bool,
    pub multi_provider: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliCapabilities {
    pub tool_id: String,
    pub display_name: String,
    pub binary: String,
    pub available_models: Vec<ModelInfo>,
    pub permission_modes: Vec<PermissionModeInfo>,
    pub features: CliFeatures,
}

impl CliCapabilities {
    pub fn default_model(&self) -> Option<&ModelInfo> {
        self.available_models
            .iter()
            .find(|m| m.is_default)
            .or_else(|| self.available_models.first())
    }

    pub fn default_permission_mode(&self) -> Option<&PermissionModeInfo> {
        self.permission_modes
            .iter()
            .find(|p| p.is_default)
            .or_else(|| self.permission_modes.first())
    }
}

// ── CliTool ────────────────────────────────────────────────────────────────

/// CLI tool selection (wasm stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CliTool {
    #[default]
    ClaudeCode,
    Codex,
    Gemini,
    OpenCode,
}

impl CliTool {
    pub fn capabilities(&self) -> CliCapabilities {
        let (tool_id, display_name, binary) = match self {
            CliTool::ClaudeCode => ("claude_code", "Claude Code", "claude"),
            CliTool::Codex      => ("codex",       "Codex",       "codex"),
            CliTool::Gemini     => ("gemini",      "Gemini",      "gemini"),
            CliTool::OpenCode   => ("opencode",    "OpenCode",    "opencode"),
        };
        CliCapabilities {
            tool_id:          tool_id.to_string(),
            display_name:     display_name.to_string(),
            binary:           binary.to_string(),
            available_models: vec![ModelInfo {
                id:             format!("{}-default", tool_id),
                display_name:   display_name.to_string(),
                is_default:     true,
                is_free_tier:   false,
                context_window: None,
            }],
            permission_modes: vec![],
            features:         CliFeatures::default(),
        }
    }
}

// ── SessionConfig ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    pub tool: CliTool,
    pub working_dir: PathBuf,
    pub env_vars: Vec<(String, String)>,
    pub name: Option<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            tool:        CliTool::ClaudeCode,
            working_dir: PathBuf::from("."),
            env_vars:    Vec::new(),
            name:        None,
        }
    }
}

// ── ManagerConfig ──────────────────────────────────────────────────────────

pub struct ManagerConfig {
    pub sessions_dir: PathBuf,
    pub default_cols: u16,
    pub default_rows: u16,
}

impl Default for ManagerConfig {
    fn default() -> Self {
        Self {
            sessions_dir: PathBuf::from("."),
            default_cols: 80,
            default_rows: 24,
        }
    }
}

// ── TermCell / TermGrid / BuddyArt ─────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct TermCell {
    pub ch: String,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub bold: bool,
    pub bg_is_default: bool,
    pub width: u8,
}

impl Default for TermCell {
    fn default() -> Self {
        Self {
            ch:           " ".to_string(),
            fg:           [204, 204, 204],
            bg:           [0, 0, 0],
            bold:         false,
            bg_is_default: true,
            width:        1,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BuddyArt {
    pub rows:       Vec<Vec<TermCell>>,
    pub width:      u16,
    pub anchor_col: u16,
    pub anchor_row: u16,
}

#[derive(Clone, Debug)]
pub struct TermGrid {
    pub cells:          Vec<Vec<TermCell>>,
    pub cols:           u16,
    pub rows:           u16,
    pub cursor_row:     u16,
    pub cursor_col:     u16,
    pub cursor_visible: bool,
    pub buddy:          Option<BuddyArt>,
}

impl TermGrid {
    pub fn empty(cols: u16, rows: u16) -> Self {
        Self {
            cells:          vec![vec![TermCell::default(); cols as usize]; rows as usize],
            cols,
            rows,
            cursor_row:     0,
            cursor_col:     0,
            cursor_visible: true,
            buddy:          None,
        }
    }
}

// ── ChatRole / ChatMessage ─────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
    Thinking,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role:      ChatRole,
    pub content:   String,
    pub tool_name: Option<String>,
}

// ── AgentSnapshotMode / AgentRenderSnapshot ────────────────────────────────

#[derive(Clone, Debug)]
pub enum AgentSnapshotMode {
    Pty(TermGrid),
    Chat(Vec<ChatMessage>),
    Idle,
}

#[derive(Clone, Debug)]
pub struct AgentRenderSnapshot {
    pub mode:            AgentSnapshotMode,
    pub session_active:  bool,
    pub live_status:     LiveStatus,
    pub pipe_session_id: Option<String>,
    pub context_percent: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum LiveStatus {
    #[default]
    Idle,
    Thinking,
    RunningTool { name: String, done: u32 },
}

// ── SessionMeta ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SessionMeta {
    pub id:        String,
    pub timestamp: i64,
    pub preview:   String,
}

// ── MultiCliManager ────────────────────────────────────────────────────────

/// No-op agent session manager for wasm32.
///
/// Every method that would touch PTY / process / DB returns an unsupported
/// error or a sensible zero-value. The chart application will simply never
/// have a live agent panel on wasm.
#[derive(Default)]
pub struct MultiCliManager;

impl MultiCliManager {
    pub fn new(_config: ManagerConfig) -> Self {
        Self
    }

    // ── Per-instance API (matches gate4agent 0.2.37 surface) ──────────────

    pub fn create_instance(
        &mut self,
        _cli: AgentCli,
        _mode: InstanceMode,
        _workdir: PathBuf,
    ) -> Result<InstanceId, String> {
        Err("agent PTY not supported on wasm".to_string())
    }

    pub async fn start_pty_instance(
        &mut self,
        _id: InstanceId,
        _config: SessionConfig,
    ) -> Result<(), String> {
        Err("agent PTY not supported on wasm".to_string())
    }

    pub async fn write_pty_instance(
        &mut self,
        _id: InstanceId,
        _text: &str,
    ) -> Result<(), String> {
        Err("agent PTY not supported on wasm".to_string())
    }

    pub async fn send_chat_instance(
        &mut self,
        _id: InstanceId,
        _prompt: &str,
    ) -> Result<(), String> {
        Err("agent chat not supported on wasm".to_string())
    }

    pub async fn stop_instance(&mut self, _id: InstanceId) {}

    pub async fn resize_instance(&mut self, _id: InstanceId, _cols: u16, _rows: u16) {}

    pub fn snapshot_instance(&self, _id: InstanceId) -> Option<AgentRenderSnapshot> {
        None
    }

    pub fn drain_events(&mut self) -> bool {
        false
    }

    pub fn load_latest_history_instance(&mut self, _id: InstanceId) -> bool {
        false
    }

    pub fn load_history_instance(&mut self, _id: InstanceId, _session_id: &str) -> bool {
        false
    }

    pub fn clear_chat_instance(&mut self, _id: InstanceId) {}

    pub fn list_past_sessions_instance(&self, _id: InstanceId) -> Vec<SessionMeta> {
        vec![]
    }

    pub fn cli_workdir(&self, _cli: AgentCli) -> PathBuf {
        PathBuf::from(".")
    }

    pub fn is_instance_active(&self, _id: InstanceId) -> bool {
        false
    }

    pub fn list_instances(&self) -> Vec<InstanceId> {
        vec![]
    }

    pub fn instance_cli(&self, _id: InstanceId) -> Option<AgentCli> {
        None
    }

    pub fn instance_mode(&self, _id: InstanceId) -> Option<InstanceMode> {
        None
    }
}
