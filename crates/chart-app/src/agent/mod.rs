mod manager;
mod snapshot;

pub use manager::AgentSessionManager;
pub use snapshot::{AgentRenderSnapshot, AgentSnapshotMode, TermCell, TermGrid, ChatMessage, ChatRole};
