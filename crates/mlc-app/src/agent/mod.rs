//! Agent orchestration — thin re-export layer.
//!
//! On native targets: re-exports the real `gate4agent` public surface so
//! existing callers in `mlc-app` continue to compile without changes.
//!
//! On wasm32: re-exports an equivalent no-op stub module (`wasm_stub`).
//! gate4agent 0.4.8 does not compile to wasm32; the stub provides an
//! identical surface that always returns errors / empty values.

#[cfg(not(target_arch = "wasm32"))]
pub use gate4agent::{MultiCliManager, ManagerConfig, InstanceId, InstanceMode};
#[cfg(not(target_arch = "wasm32"))]
pub use gate4agent::{
    AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid, AgentCli,
    CliTool, CliCapabilities, CliFeatures, ModelInfo, PermissionModeInfo, SessionConfig,
    SessionMeta,
};

#[cfg(target_arch = "wasm32")]
mod wasm_stub;
#[cfg(target_arch = "wasm32")]
pub use wasm_stub::{
    MultiCliManager, ManagerConfig, InstanceId, InstanceMode,
    AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid, AgentCli,
    CliTool, CliCapabilities, CliFeatures, ModelInfo, PermissionModeInfo, SessionConfig,
    SessionMeta, LiveStatus, BuddyArt,
};

/// Backwards-compat alias — existing code that refers to `AgentSessionManager` keeps working.
pub type AgentSessionManager = MultiCliManager;
