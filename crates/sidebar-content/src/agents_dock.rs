//! Docking grid types for the Agents panel.
//!
//! `AgentPaneLeaf` is the payload stored in each leaf of the sidebar-local
//! `DockingManager`. `AgentLeafDescriptor` carries the richer metadata needed
//! to re-create an `AgentInstance` after a profile restore.

use std::path::PathBuf;
use gate4agent::{AgentCli, InstanceId, InstanceMode};

// =============================================================================
// AgentDockingManager — Clone/Debug wrapper
// =============================================================================

/// Newtype wrapper around `DockingManager<AgentPaneLeaf>` that provides
/// manual `Clone` and `Debug` impls so it can be a field of the `#[derive]`-d
/// `SidebarState`.
///
/// `Clone` creates a new empty manager rather than deep-copying the internal
/// tree. This is intentional: `SidebarState::clone()` is only used for
/// snapshot/undo scenarios where we want a clean docking slate anyway.
pub struct AgentDockingManager(pub uzor::panels::DockingManager<AgentPaneLeaf>);

impl AgentDockingManager {
    /// Create a new empty docking manager.
    pub fn new() -> Self {
        Self(uzor::panels::DockingManager::new())
    }

    /// Borrow the inner manager immutably.
    pub fn inner(&self) -> &uzor::panels::DockingManager<AgentPaneLeaf> {
        &self.0
    }

    /// Borrow the inner manager mutably.
    pub fn inner_mut(&mut self) -> &mut uzor::panels::DockingManager<AgentPaneLeaf> {
        &mut self.0
    }
}

impl Default for AgentDockingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AgentDockingManager {
    /// Returns a new **empty** docking manager. Structural cloning of the
    /// panel tree is not needed for the undo/snapshot use-cases that drive
    /// `SidebarState::clone()`.
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AgentDockingManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentDockingManager").finish_non_exhaustive()
    }
}

// =============================================================================
// AgentLeafDescriptor
// =============================================================================

/// Full descriptor for an agent pane — persisted in the profile and used to
/// re-create agent instances when the user restarts the application.
#[derive(Clone, Debug)]
pub struct AgentLeafDescriptor {
    /// Opaque instance identifier assigned by `MultiCliManager`.
    pub instance_id: InstanceId,
    /// Which AI CLI runs in this pane.
    pub cli: AgentCli,
    /// Transport mode: PTY mirror or chat/pipe.
    pub mode: InstanceMode,
    /// Working directory for the agent process.
    pub workdir: PathBuf,
    /// Conversation session ID — only meaningful for `InstanceMode::Chat` panes.
    pub chat_session_id: Option<String>,
}

// =============================================================================
// AgentPaneLeaf
// =============================================================================

/// Leaf payload stored inside the sidebar-local `DockingManager`.
///
/// Implements [`uzor::panels::DockPanel`] so the docking engine can query tab
/// titles, minimum sizes, and close-ability without depending on the heavier
/// `AgentLeafDescriptor`.
#[derive(Clone, Debug)]
pub struct AgentPaneLeaf {
    /// Opaque instance identifier — used to look up the full descriptor in
    /// `SidebarState::agent_leaves`.
    pub instance_id: InstanceId,
    /// Which AI CLI runs in this pane (stored here for cheap title rendering).
    pub cli: AgentCli,
    /// Transport mode (stored here for cheap title rendering).
    pub mode: InstanceMode,
}

impl uzor::panels::DockPanel for AgentPaneLeaf {
    fn title(&self) -> &str {
        match self.cli {
            AgentCli::Claude => match self.mode {
                InstanceMode::Pty  => "Claude PTY",
                InstanceMode::Chat => "Claude Chat",
            },
            AgentCli::Codex => match self.mode {
                InstanceMode::Pty  => "Codex PTY",
                InstanceMode::Chat => "Codex Chat",
            },
            AgentCli::Gemini => match self.mode {
                InstanceMode::Pty  => "Gemini PTY",
                InstanceMode::Chat => "Gemini Chat",
            },
            AgentCli::OpenCode => match self.mode {
                InstanceMode::Pty  => "OpenCode PTY",
                InstanceMode::Chat => "OpenCode Chat",
            },
        }
    }

    fn type_id(&self) -> &'static str {
        "agent_pane"
    }

    fn min_size(&self) -> (f32, f32) {
        (80.0, 140.0)
    }

    fn closable(&self) -> bool {
        true
    }
}
