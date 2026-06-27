//! Top-level `Managers` container.
//!
//! Bundles every backend manager that mutates the bubble data plane.
//! Hosted by `BackendCore.managers: RwLock<Managers>` so all writers
//! acquire one lock and lock-ordering bugs cannot creep in.
//!
//! Current population (after Signal/Trade scaffold strip):
//! - `alert:  AlertManager`  (from the `mlc-alerts` crate).
//! - `bubble: BubbleStore`   (DataBubble registry + tag/lifecycle —
//!   slimming further as the ui-blob cutover progresses).

use mlc_alerts::AlertManager;

use crate::tag_manager::BubbleStore;

/// Top-level container for every backend manager service.
pub struct Managers {
    /// Global alert service.  Owns alert items, the ID counter, and
    /// per-symbol crossing state.
    pub alert: AlertManager,

    /// Bubble registry + sync-group routing.
    pub bubble: BubbleStore,
}

impl Managers {
    pub fn new() -> Self {
        Self {
            alert: AlertManager::new(),
            bubble: BubbleStore::new(),
        }
    }
}

impl Default for Managers {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Managers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Managers").finish_non_exhaustive()
    }
}
