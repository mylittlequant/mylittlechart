//! Agent rendering types — re-exported from `gate4agent` where they now live.

pub use gate4agent::{
    AgentCli, AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid,
};
pub use gate4agent::pty::snapshot::LiveStatus;
pub use gate4agent::history::SessionMeta;
