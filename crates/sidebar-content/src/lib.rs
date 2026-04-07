//! Sidebar content rendering for chart applications.
//!
//! Provides a faithful clone of the terminal core's right sidebar system,
//! adapted for use from `chart-app` without depending on `zengeld-terminal-core`.

pub mod agent_types;
pub mod types;
pub mod state;
pub mod render;
pub mod watchlist;

pub use render::{render_right_sidebar, RightSidebarResult};
pub use state::{SidebarState, RightSidebarPanel, RIGHT_SIDEBAR_WIDTH, MetricsSnapshot};
pub use types::{ObjectTreeItem, AlertItem, IndicatorsTabData, WatchlistItem, ConnectorGroup, ConnectorStatusItem};
pub use agent_types::{AgentCli, AgentRenderSnapshot, AgentSnapshotMode, TermGrid, TermCell, ChatMessage, ChatRole};
