//! Agent orchestration — thin re-export layer.
//!
//! All business logic lives in `gate4agent`. This module re-exports the
//! public surface so existing callers in `chart-app` continue to compile
//! without changes.

pub use gate4agent::{MultiCliManager, ManagerConfig};
pub use gate4agent::snapshot::{
    AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid, AgentCli,
};

/// Backwards-compat alias — existing code that refers to `AgentSessionManager` keeps working.
pub type AgentSessionManager = MultiCliManager;
