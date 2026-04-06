/// Agent rendering types — re-exported from `sidebar-content` where they live.
///
/// `sidebar-content` owns these types because the rendering crate needs them
/// to draw the Agents panel, and `chart-app` depends on `sidebar-content`.
pub use sidebar_content::agent_types::{
    AgentRenderSnapshot, AgentSnapshotMode, ChatMessage, ChatRole, TermCell, TermGrid,
};
