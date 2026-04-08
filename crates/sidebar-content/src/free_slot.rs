//! Per-slot docking grid types for the free-slot hyperspace (Slot1..Slot4).
//!
//! Each of the 4 slot sidebars hosts its own `DockingManager<FreeItem>`. The
//! set `{Main, Slot1..Slot4}` forms a single cross-container drag hyperspace
//! in Phase 3-new, with the restriction that `FreeItem::Chart(_)` is locked to
//! Main. See [`docs/plans/sidebar-containers-docking.md`] for the full model.

// =============================================================================
// FreeItem
// =============================================================================

/// Payload stored in each leaf of a `DockingManager<FreeItem>`.
///
/// Separated from `AgentPaneLeaf` by type so that Agents panes can never leak
/// into the free-slot hyperspace and trading panels can never leak into the
/// Agents sidebar.
#[derive(Clone, Debug)]
pub enum FreeItem {
    /// Phase 2b-new stub — placeholder leaf used to verify plumbing before
    /// real trading panels land in Phase 4-new.
    Placeholder,
}

impl uzor::panels::DockPanel for FreeItem {
    fn title(&self) -> &str {
        match self {
            FreeItem::Placeholder => "Placeholder",
        }
    }

    fn type_id(&self) -> &'static str {
        match self {
            FreeItem::Placeholder => "free_placeholder",
        }
    }

    fn min_size(&self) -> (f32, f32) {
        (200.0, 120.0)
    }

    fn closable(&self) -> bool {
        true
    }
}

// =============================================================================
// SlotDockingManager — Clone/Debug wrapper
// =============================================================================

/// Newtype around `DockingManager<FreeItem>` providing manual `Clone` + `Debug`
/// so it can live inside the `#[derive]`-d `SidebarState`.
///
/// `Clone` returns an empty manager — same policy as `AgentDockingManager`,
/// used only for `SidebarState` snapshot/undo scenarios where a clean slate
/// is desired.
pub struct SlotDockingManager(pub uzor::panels::DockingManager<FreeItem>);

impl SlotDockingManager {
    pub fn new() -> Self {
        Self(uzor::panels::DockingManager::new())
    }

    pub fn inner(&self) -> &uzor::panels::DockingManager<FreeItem> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut uzor::panels::DockingManager<FreeItem> {
        &mut self.0
    }
}

impl Default for SlotDockingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SlotDockingManager {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SlotDockingManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlotDockingManager").finish_non_exhaustive()
    }
}
