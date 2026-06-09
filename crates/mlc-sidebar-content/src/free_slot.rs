//! Per-slot docking grid types for the free-slot hyperspace (Slot1..Slot4).
//!
//! Each of the 4 slot sidebars hosts its own `DockingManager<Panel>`. The
//! set `{Main, Slot1..Slot4}` forms a single cross-container drag hyperspace.
//!
//! `Panel` is the unified docking-payload enum defined in `mlc_core::state`.
//! K1.3: `FreeItem` removed; `Panel` is the single source of truth.
//!
//! See `docs/plans/chart-subpane-pid-refactor-2026-06-03.md`.

// =============================================================================
// PanelId — re-exported from mlc-core (K1 single source of truth)
// =============================================================================

/// Re-export of `mlc_core::state::PanelId`.  Existing
/// `mlc_sidebar_content::free_slot::PanelId` imports keep compiling; new
/// code should import from `mlc_core::state` directly.
pub use mlc_core::state::PanelId;

// =============================================================================
// SlotDockingManager — Clone/Debug wrapper
// =============================================================================

/// Newtype around `DockingManager<Panel>` providing manual `Clone` + `Debug`
/// so it can live inside the `#[derive]`-d `SidebarState`.
///
/// `Clone` returns an empty manager — same policy as `AgentDockingManager`,
/// used only for `SidebarState` snapshot/undo scenarios where a clean slate
/// is desired.
pub struct SlotDockingManager(pub uzor::layout::DockState<mlc_core::state::Panel>);

impl SlotDockingManager {
    pub fn new() -> Self {
        Self(uzor::layout::DockState::new())
    }

    pub fn inner(&self) -> &uzor::layout::DockState<mlc_core::state::Panel> {
        &self.0
    }

    pub fn inner_mut(&mut self) -> &mut uzor::layout::DockState<mlc_core::state::Panel> {
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
