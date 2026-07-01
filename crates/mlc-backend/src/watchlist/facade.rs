//! `Watchlists` / `WatchlistsMut` — the only doctrinally-sanctioned access
//! path to `WatchlistManager` from outside `mlc-backend`.
//!
//! Unlike Alert/Drawing, `WatchlistManager` never touches `DataBubble`
//! (confirmed: `docs/mlc/plans/step-3.3-indicator-watchlist-design.md`
//! §1.2 — zero bubble references anywhere in the manager). The facade holds
//! exactly one guard, no `BubbleStore` co-acquisition, ever.
//!
//! `WatchlistManager` no longer owns `active_list_id` (doctrine 1B — which
//! tab is open is presentation, lives on `mlc-app::watchlist_ui`). Every
//! method that used to implicitly target "the active list" now takes
//! `list_id: u64` explicitly; the caller (app) supplies it from its own
//! UI-side active-list-id.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A / §1B.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::WatchlistList;

use super::manager::WatchlistManager;

/// Read-only facade over `WatchlistManager`.
pub struct Watchlists<'a> {
    mgr: RwLockReadGuard<'a, WatchlistManager>,
}

impl<'a> Watchlists<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, WatchlistManager>) -> Self {
        Self { mgr }
    }

    /// All watchlists.
    pub fn lists(&self) -> &[WatchlistList] {
        &self.mgr.lists
    }

    /// Find a watchlist by id.
    pub fn find(&self, list_id: u64) -> Option<&WatchlistList> {
        self.mgr.find(list_id)
    }

    /// Check if a (symbol, exchange, account_type) triple is in the given list.
    pub fn contains(&self, list_id: u64, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.mgr.contains(list_id, symbol, exchange, account_type)
    }
}

/// Mutable facade over `WatchlistManager`.
pub struct WatchlistsMut<'a> {
    mgr: RwLockWriteGuard<'a, WatchlistManager>,
}

impl<'a> WatchlistsMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, WatchlistManager>) -> Self {
        Self { mgr }
    }

    /// All watchlists.
    pub fn lists(&self) -> &[WatchlistList] {
        &self.mgr.lists
    }

    /// Find a watchlist by id.
    pub fn find(&self, list_id: u64) -> Option<&WatchlistList> {
        self.mgr.find(list_id)
    }

    /// Find a watchlist by id, mutably. `WatchlistList` IS the domain's own
    /// value type (same class as `AlertItem`/`IndicatorInstance` returned by
    /// other facades' `_mut` accessors) — the facade's job is hiding
    /// `DataBubble`/`BubbleStore`, not hiding every field of a value type
    /// that has no bubble concept at all.
    pub fn find_mut(&mut self, list_id: u64) -> Option<&mut WatchlistList> {
        self.mgr.find_mut(list_id)
    }

    /// Create a new empty watchlist and return its id.
    pub fn create_list(&mut self, name: String) -> u64 {
        self.mgr.create_list(name)
    }

    /// Delete a watchlist by id. Cannot delete the last one.
    pub fn delete_list(&mut self, id: u64) -> bool {
        self.mgr.delete_list(id)
    }

    /// Generate the next unique id (for groups etc).
    pub fn next_unique_id(&mut self) -> u64 {
        self.mgr.next_unique_id()
    }

    /// Add a symbol to the given list.
    pub fn add_symbol(&mut self, list_id: u64, symbol: String, exchange: String, account_type: String) {
        self.mgr.add_symbol(list_id, symbol, exchange, account_type);
    }

    /// Remove a specific (symbol, exchange, account_type) triple from the given list.
    pub fn remove_symbol(&mut self, list_id: u64, symbol: &str, exchange: &str, account_type: &str) {
        self.mgr.remove_symbol(list_id, symbol, exchange, account_type);
    }

    /// Toggle symbol membership in the given list: add if missing, remove if
    /// present. Returns the new state (`true` = now in list).
    pub fn toggle_symbol(&mut self, list_id: u64, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.mgr.toggle_symbol(list_id, symbol, exchange, account_type)
    }

    /// Reorder a symbol in the given list's ungrouped section.
    pub fn reorder_symbol(&mut self, list_id: u64, from_idx: usize, to_idx: usize) {
        self.mgr.reorder_symbol(list_id, from_idx, to_idx);
    }

    /// Move a symbol into a specific group of the given list. No-op if the
    /// list or the group does not exist.
    pub fn move_to_group(&mut self, list_id: u64, symbol: &str, exchange: &str, account_type: &str, group_id: u64) {
        if let Some(list) = self.mgr.find_mut(list_id) {
            list.move_to_group(symbol, exchange, account_type, group_id);
        }
    }

    /// Add a new group to the given list. Returns the new group id, or 0 if
    /// the list does not exist.
    pub fn add_group(&mut self, list_id: u64, name: String) -> u64 {
        let id = self.mgr.next_unique_id();
        if let Some(list) = self.mgr.find_mut(list_id) {
            list.add_group(id, name);
            id
        } else {
            0
        }
    }

    /// Remove a group from the given list by id. Its symbols move to ungrouped.
    pub fn remove_group(&mut self, list_id: u64, group_id: u64) {
        if let Some(list) = self.mgr.find_mut(list_id) {
            list.remove_group(group_id);
        }
    }

    /// Set a color flag for a symbol in the given list.
    pub fn set_color_flag(&mut self, list_id: u64, symbol: &str, exchange: &str, account_type: &str, color: &str) {
        if let Some(list) = self.mgr.find_mut(list_id) {
            list.set_color_flag(symbol, exchange, account_type, color);
        }
    }

    /// Replace the whole manager wholesale (import-from-file, cross-window
    /// sync push). Existing park/unpark and cross-window-sync call sites keep
    /// using the raw `RwLock` bypass (design §6.6) — this method exists for
    /// any future non-bypass caller (e.g. an import feature) that needs a
    /// named "load a whole watchlist set" operation.
    pub fn replace_all(&mut self, manager: WatchlistManager) {
        *self.mgr = manager;
    }

    /// Check if a (symbol, exchange, account_type) triple is in the given list.
    pub fn contains(&self, list_id: u64, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.mgr.contains(list_id, symbol, exchange, account_type)
    }
}
