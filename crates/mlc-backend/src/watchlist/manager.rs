//! `WatchlistManager` — CRUD over named watchlists.
//!
//! Owns `lists: Vec<WatchlistList>` (domain value type, `mlc-core`). Does
//! NOT own `active_list_id` (which tab is open — presentation, lives on
//! `mlc-app::watchlist_ui::WatchlistUiState`) or `order_snapshot` (derived
//! sort-cache — same UI home). See
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B.
//!
//! Every CRUD method that used to implicitly target "the active list" now
//! takes `list_id: u64` explicitly — the caller (facade) supplies it from
//! the UI-side `active_list_id`.

use mlc_core::{InstrumentAddress, WatchlistList};
use serde::{Deserialize, Serialize};

/// Manages multiple watchlists.
///
/// Persisted wholesale to `watchlists.json` (`mlc_core::load_json` /
/// `save_json`) — see `mlc-app/src/persistence/profile.rs`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistManager {
    pub lists: Vec<WatchlistList>,
    next_id: u64,
}

impl WatchlistManager {
    /// Create with a single default list containing the given symbols.
    pub fn new(default_symbols: Vec<InstrumentAddress>) -> Self {
        let list = WatchlistList {
            id: 1,
            name: "Watchlist 1".to_string(),
            groups: Vec::new(),
            ungrouped: default_symbols,
            color_flags: std::collections::HashMap::new(),
        };
        Self { lists: vec![list], next_id: 2 }
    }

    /// Find a watchlist by id.
    pub fn find(&self, list_id: u64) -> Option<&WatchlistList> {
        self.lists.iter().find(|l| l.id == list_id)
    }

    /// Find a watchlist by id, mutably.
    pub fn find_mut(&mut self, list_id: u64) -> Option<&mut WatchlistList> {
        self.lists.iter_mut().find(|l| l.id == list_id)
    }

    /// Check if an instrument is in the given list.
    pub fn contains(&self, list_id: u64, address: &InstrumentAddress) -> bool {
        self.find(list_id).map(|l| l.contains(address)).unwrap_or(false)
    }

    /// Add an instrument to the given list.
    pub fn add_symbol(&mut self, list_id: u64, address: InstrumentAddress) {
        if let Some(list) = self.find_mut(list_id) {
            list.add_symbol(address);
        }
    }

    /// Remove a specific instrument from the given list.
    pub fn remove_symbol(&mut self, list_id: u64, address: &InstrumentAddress) {
        if let Some(list) = self.find_mut(list_id) {
            list.remove_symbol(address);
        }
    }

    /// Toggle instrument membership in the given list: add if missing, remove
    /// if present. Returns the new state (`true` = now in list).
    pub fn toggle_symbol(&mut self, list_id: u64, address: &InstrumentAddress) -> bool {
        if self.contains(list_id, address) {
            self.remove_symbol(list_id, address);
            false
        } else {
            self.add_symbol(list_id, address.clone());
            true
        }
    }

    /// Create a new empty watchlist and return its id.
    pub fn create_list(&mut self, name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.lists.push(WatchlistList { id, name, groups: Vec::new(), ungrouped: Vec::new(), color_flags: std::collections::HashMap::new() });
        id
    }

    /// Delete a watchlist by id. Cannot delete the last one. Returns `true` on success.
    pub fn delete_list(&mut self, id: u64) -> bool {
        if self.lists.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.lists.iter().position(|l| l.id == id) {
            self.lists.remove(idx);
            true
        } else {
            false
        }
    }

    /// Generate the next unique id (for groups etc).
    pub fn next_unique_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Reorder a symbol in the given list's ungrouped section.
    ///
    /// Moves the symbol at position `from_idx` to position `to_idx`. Both
    /// indices are clamped to the valid range; no-op if `from_idx == to_idx`
    /// or if the list has no ungrouped symbols at those positions.
    pub fn reorder_symbol(&mut self, list_id: u64, from_idx: usize, to_idx: usize) {
        if from_idx == to_idx {
            return;
        }
        if let Some(list) = self.find_mut(list_id) {
            let len = list.ungrouped.len();
            if from_idx >= len || to_idx >= len {
                return;
            }
            let symbol = list.ungrouped.remove(from_idx);
            let insert_at = if to_idx > from_idx { to_idx.min(list.ungrouped.len()) } else { to_idx };
            list.ungrouped.insert(insert_at, symbol);
        }
    }
}

impl Default for WatchlistManager {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::AccountType;

    #[test]
    fn test_new_manager_has_one_list() {
        let mgr = WatchlistManager::new(vec![InstrumentAddress::new("Binance", "BTCUSDT", AccountType::Spot)]);
        assert_eq!(mgr.lists.len(), 1);
        assert_eq!(mgr.lists[0].id, 1);
    }

    #[test]
    fn test_toggle_symbol_add_remove() {
        let mut mgr = WatchlistManager::default();
        let id = mgr.create_list("L1".to_string());
        let address = InstrumentAddress::new("Binance", "ETHUSDT", AccountType::Spot);
        assert!(!mgr.contains(id, &address));
        let added = mgr.toggle_symbol(id, &address);
        assert!(added);
        assert!(mgr.contains(id, &address));
        let removed = mgr.toggle_symbol(id, &address);
        assert!(!removed);
        assert!(!mgr.contains(id, &address));
    }

    #[test]
    fn test_create_and_delete_list() {
        let mut mgr = WatchlistManager::default();
        assert_eq!(mgr.lists.len(), 1);
        let id = mgr.create_list("My List".to_string());
        assert_eq!(mgr.lists.len(), 2);
        let deleted = mgr.delete_list(id);
        assert!(deleted);
        assert_eq!(mgr.lists.len(), 1);
    }

    #[test]
    fn test_cannot_delete_last_list() {
        let mut mgr = WatchlistManager::default();
        let only_id = mgr.lists[0].id;
        let deleted = mgr.delete_list(only_id);
        assert!(!deleted);
        assert_eq!(mgr.lists.len(), 1);
    }

    #[test]
    fn test_color_flags_persist_on_list() {
        let mut mgr = WatchlistManager::default();
        let id = mgr.lists[0].id;
        let address = InstrumentAddress::new("Binance", "BTCUSDT", AccountType::Spot);
        mgr.add_symbol(id, address.clone());
        mgr.find_mut(id).unwrap().set_color_flag(&address, "#ff0000");
        assert_eq!(mgr.find(id).unwrap().get_color_flag(&address), Some("#ff0000"));
    }
}
