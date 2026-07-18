//! Sidebar content rendering for chart applications.
//!
//! Provides a faithful clone of the terminal core's right sidebar system,
//! adapted for use from `chart-app` without depending on `zengeld-terminal-core`.

// On wasm32, gate4agent is absent from the dep graph; the stub module
// provides the same surface. Each submodule that references gate4agent
// imports it locally via `#[cfg(target_arch = "wasm32")] use crate::gate4agent_stub as gate4agent`.
#[cfg(target_arch = "wasm32")]
pub mod gate4agent_stub;

pub mod agent_types;
pub mod agents_dock;
pub mod free_slot;
pub mod types;
pub mod state;
pub mod render;

pub use render::{render_right_sidebar, RightSidebarResult};
pub use state::{SidebarState, RightSidebarPanel, RIGHT_SIDEBAR_WIDTH, MetricsSnapshot, MonitoringSection};
pub use types::{ObjectTreeItem, AlertItem, IndicatorsTabData, WatchlistItem, WatchlistColumnConfig, ConnectorGroup, ConnectorStatusItem};
pub use agent_types::{AgentCli, AgentRenderSnapshot, AgentSnapshotMode, TermGrid, TermCell, ChatMessage, ChatRole};
pub use agents_dock::{AgentPaneLeaf, AgentLeafDescriptor, AgentDockingManager};
pub use free_slot::SlotDockingManager;
