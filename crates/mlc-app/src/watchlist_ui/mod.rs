//! Watchlist UI/presentation state — split off `WatchlistManager` per
//! doctrine 1B (`docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B).
//!
//! `active_list_id` (which tab is open), `order_snapshot` (derived sort
//! cache), `WatchlistColumnConfig` (column visibility/widths), and
//! `GroupUiState` (group color / collapsed) are presentation — a headless
//! daemon consumer doesn't need any of them to work correctly. They live
//! here, sidecar-keyed by the domain ids (`WatchlistList.id` /
//! `WatchlistGroup.id`) owned by `mlc_backend::watchlist::WatchlistManager`.
//!
//! Hard cutover: old `watchlists.json` (which embedded these fields inside
//! `WatchlistManager`/`WatchlistList`/`WatchlistGroup`) loses this state on
//! first load under the new split — accepted, presentation-only, no
//! migration shim (owner decision, doctrine-cutover-2026-06-27 follow-up).

use std::collections::HashMap;

use mlc_core::InstrumentAddress;
use serde::{Deserialize, Serialize};

/// Configuration for which columns to show in the watchlist sidebar.
///
/// Defined in `mlc-app-sidebar-content::types` (not here) because the
/// sidebar render function needs it by concrete type and
/// `mlc-app-sidebar-content` sits below `mlc-app` in the dependency graph
/// (`mlc-app` -> `mlc-app-sidebar-content`, one-way). Re-exported here so
/// `mlc-app` callers reach it through `watchlist_ui`.
pub use mlc_app_sidebar_content::WatchlistColumnConfig;

// =============================================================================
// GroupUiState
// =============================================================================

/// Presentation state for one `WatchlistGroup` — color (decoration, no
/// routing by it) and collapsed/expanded state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupUiState {
    pub color: String,
    pub collapsed: bool,
}

impl Default for GroupUiState {
    fn default() -> Self {
        Self { color: "#6b7280".to_string(), collapsed: false }
    }
}

// =============================================================================
// ListUiState
// =============================================================================

/// Presentation state for one `WatchlistList`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListUiState {
    pub column_config: WatchlistColumnConfig,
    /// Snapshot of the original symbol order (before any sort).
    ///
    /// When `None`, the domain `ungrouped` order IS the original order.
    /// Set when the user first activates a sort mode; cleared when the user
    /// resets to mode 0 or manually reorders symbols via drag.
    #[serde(default)]
    pub order_snapshot: Option<Vec<InstrumentAddress>>,
    /// Per-group presentation state, keyed by `WatchlistGroup.id`.
    #[serde(default)]
    pub groups: HashMap<u64, GroupUiState>,
}

// =============================================================================
// WatchlistUiState
// =============================================================================

/// Top-level watchlist UI/presentation state — which list tab is active,
/// plus per-list presentation sidecars keyed by `WatchlistList.id`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistUiState {
    pub active_list_id: u64,
    #[serde(default)]
    pub lists: HashMap<u64, ListUiState>,
}

impl Default for WatchlistUiState {
    /// `active_list_id` defaults to `1` — matches
    /// `WatchlistManager::new`'s first list id (see
    /// `crates/mlc-backend/src/watchlist/manager.rs`), so a fresh
    /// `WatchlistUiState` paired with a fresh `WatchlistManager` resolves
    /// `active_list()` correctly without extra wiring.
    fn default() -> Self {
        Self { active_list_id: 1, lists: HashMap::new() }
    }
}

impl WatchlistUiState {
    /// Presentation state for a given list, materializing a default entry on
    /// first access (mirrors `WatchlistManager::find`'s "just works" feel
    /// without forcing every render call site to `unwrap_or_default`).
    pub fn list_mut(&mut self, list_id: u64) -> &mut ListUiState {
        self.lists.entry(list_id).or_default()
    }

    /// Read-only presentation state for a given list, `None` if never touched.
    pub fn list(&self, list_id: u64) -> Option<&ListUiState> {
        self.lists.get(&list_id)
    }

    /// Column config for a given list, or defaults if the list has no UI
    /// state yet.
    pub fn column_config(&self, list_id: u64) -> WatchlistColumnConfig {
        self.list(list_id).map(|l| l.column_config.clone()).unwrap_or_default()
    }

    /// Drop the sidecar for a deleted list (call alongside
    /// `WatchlistManager::delete_list`).
    pub fn remove_list(&mut self, list_id: u64) {
        self.lists.remove(&list_id);
    }
}
