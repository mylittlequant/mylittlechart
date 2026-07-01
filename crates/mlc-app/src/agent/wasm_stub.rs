//! No-op stub for gate4agent types on wasm32.
//!
//! Shared types (InstanceMode, AgentCli, InstanceId, AgentRenderSnapshot,
//! SessionMeta, ChatRole, ChatMessage, TermCell, TermGrid, CliTool,
//! CliCapabilities, CliFeatures, ModelInfo, PermissionModeInfo, BuddyArt,
//! AgentSnapshotMode, LiveStatus) are re-exported from
//! `mlc_app_sidebar_content::gate4agent_stub` so that both crates share one
//! wasm copy of each type — eliminating the E0308 mismatches that occur when
//! mlc-app compares sidebar-content values against its own duplicate types.
//!
//! mlc-app-specific types (MultiCliManager, ManagerConfig, SessionConfig,
//! AgentSessionManager) are defined here.

use std::path::PathBuf;

// ── Re-export all shared types from sidebar-content's single wasm stub ──────

pub use mlc_app_sidebar_content::gate4agent_stub::{
    AgentCli,
    AgentRenderSnapshot,
    AgentSnapshotMode,
    BuddyArt,
    ChatMessage,
    ChatRole,
    CliCapabilities,
    CliFeatures,
    CliTool,
    InstanceId,
    InstanceMode,
    ModelInfo,
    PermissionModeInfo,
    TermCell,
    TermGrid,
};

pub use mlc_app_sidebar_content::gate4agent_stub::pty::snapshot::LiveStatus;
pub use mlc_app_sidebar_content::gate4agent_stub::history::SessionMeta;

// ── SessionConfig ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
