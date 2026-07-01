//! Agent rendering types — re-exported from `gate4agent` where they now live.

#[cfg(not(target_arch = "wasm32"))]
pub use gate4agent::{
    AgentCli, AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid,
};
#[cfg(not(target_arch = "wasm32"))]
pub use gate4agent::pty::snapshot::LiveStatus;
#[cfg(not(target_arch = "wasm32"))]
pub use gate4agent::history::SessionMeta;

#[cfg(target_arch = "wasm32")]
pub use crate::gate4agent_stub::{
    AgentCli, AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid,
};
#[cfg(target_arch = "wasm32")]
pub use crate::gate4agent_stub::pty::snapshot::LiveStatus;
#[cfg(target_arch = "wasm32")]
pub use crate::gate4agent_stub::history::SessionMeta;
