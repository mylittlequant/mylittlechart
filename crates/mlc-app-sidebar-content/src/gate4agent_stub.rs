//! No-op stub for gate4agent types on wasm32.
//!
//! gate4agent has hard non-wasm deps (portable-pty, vt100, vte, rusqlite,
//! tokio rt-multi-thread). On wasm32 the real crate is absent; this module
//! provides the surface that mlc-app-sidebar-content actually uses so all call
//! sites compile without change.

use serde::{Deserialize, Serialize};
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
            AgentCli::Claude    => "claude",
            AgentCli::Codex     => "codex",
            AgentCli::Gemini    => "gemini",
            AgentCli::OpenCode  => "opencode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AgentCli::Claude    => "Claude",
            AgentCli::Codex     => "Codex",
            AgentCli::Gemini    => "Gemini",
            AgentCli::OpenCode  => "OpenCode",
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
    pub fn capabilities(self) -> CliCapabilities {
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

// ── TermCell / BuddyArt / TermGrid ────────────────────────────────────────

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
            ch:            " ".to_string(),
            fg:            [204, 204, 204],
            bg:            [0, 0, 0],
            bold:          false,
            bg_is_default: true,
            width:         1,
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
    pub live_status:     pty::snapshot::LiveStatus,
    pub pipe_session_id: Option<String>,
    pub context_percent: Option<f64>,
}

// ── Submodule stubs for path-aliased re-exports ────────────────────────────
//
// agent_types.rs uses:
//   pub use gate4agent::pty::snapshot::LiveStatus;
//   pub use gate4agent::history::SessionMeta;
//
// When `gate4agent` is aliased to this stub module on wasm32, those paths
// must resolve here.

pub mod pty {
    pub mod snapshot {
        /// Live status of an agent session (wasm stub).
        #[derive(Clone, Debug, Default, PartialEq)]
        pub enum LiveStatus {
            #[default]
            Idle,
            Thinking,
            RunningTool { name: String, done: u32 },
        }
    }
}

pub mod history {
    /// Metadata for a past agent session (wasm stub).
    #[derive(Debug, Clone)]
    pub struct SessionMeta {
        pub id:        String,
        pub timestamp: i64,
        pub preview:   String,
    }
}

// Re-export SessionMeta at crate root for `gate4agent::SessionMeta` paths.
pub use history::SessionMeta;
