//! `Watchlists` / `WatchlistsMut` — the only doctrinally-sanctioned access
//! path to `WatchlistManager` from outside `mlc-backend`.
//!
//! Unlike Alert/Drawing/Indicator, `WatchlistManager` never touches
//! `DataBubble` (confirmed: `docs/mlc/plans/step-3.3-indicator-watchlist-design.md`
//! §1.2 — zero bubble references anywhere in the manager). The facade holds
//! exactly one guard, no `BubbleStore` co-acquisition, ever.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_watchlist::{WatchlistList, WatchlistManager};

/// Read-only facade over `WatchlistManager`.
pub struct Watchlists<'a> {
    mgr: RwLockReadGuard<'a, WatchlistManager>,
}

impl<'a> Watchlists<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, WatchlistManager>) -> Self {
        Self { mgr }
    }

    /// Raw `&WatchlistManager` reference, for render functions
    /// (`mlc_app_sidebar_content::render::render_right_sidebar`) whose signature
    /// takes the manager type directly rather than the facade — the facade
    /// hides `BubbleStore` co-acquisition (there is none here), not the
    /// manager's own already-intent-shaped API.
    pub fn as_manager(&self) -> &WatchlistManager {
        &self.mgr
    }

    /// All watchlists.
    pub fn lists(&self) -> &[WatchlistList] {
        &self.mgr.lists
    }

    /// The currently-active watchlist, if any.
    pub fn active(&self) -> Option<&WatchlistList> {
        self.mgr.active_list()
    }

    /// The id of the currently-active watchlist.
    pub fn active_id(&self) -> u64 {
        self.mgr.active_list_id
    }

    /// Find a watchlist by id.
    pub fn find(&self, list_id: u64) -> Option<&WatchlistList> {
        self.mgr.lists.iter().find(|l| l.id == list_id)
    }

    /// Check if a (symbol, exchange, account_type) triple is in the active
    /// watchlist.
    pub fn contains(&self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.mgr.contains_with_type(symbol, exchange, account_type)
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

    /// The currently-active watchlist, if any.
    pub fn active(&self) -> Option<&WatchlistList> {
        self.mgr.active_list()
    }

    /// The id of the currently-active watchlist.
    pub fn active_id(&self) -> u64 {
        self.mgr.active_list_id
    }

    /// Find a watchlist by id.
    pub fn find(&self, list_id: u64) -> Option<&WatchlistList> {
        self.mgr.lists.iter().find(|l| l.id == list_id)
    }

    /// The currently-active watchlist, mutably. `WatchlistList` IS the
    /// domain's own value type (same class as `AlertItem`/`IndicatorInstance`
    /// returned by other facades' `_mut` accessors) — the facade's job is
    /// hiding `DataBubble`/`BubbleStore`, not hiding every field of a value
    /// type that has no bubble concept at all.
    pub fn active_mut(&mut self) -> Option<&mut WatchlistList> {
        self.mgr.active_list_mut()
    }

    /// Find a watchlist by id, mutably.
    pub fn find_mut(&mut self, list_id: u64) -> Option<&mut WatchlistList> {
        self.mgr.lists.iter_mut().find(|l| l.id == list_id)
    }

    /// Create a new empty watchlist and return its id.
    pub fn create_list(&mut self, name: String) -> u64 {
        self.mgr.create_list(name)
    }

    /// Delete a watchlist by id. Cannot delete the last one.
    pub fn delete_list(&mut self, id: u64) -> bool {
        self.mgr.delete_list(id)
    }

    /// Set the active watchlist.
    pub fn set_active(&mut self, id: u64) {
        self.mgr.active_list_id = id;
    }

    /// Add a symbol to the active watchlist.
    pub fn add_symbol(&mut self, symbol: String, exchange: String, account_type: String) {
        self.mgr.add_symbol_with_type(symbol, exchange, account_type);
    }

    /// Remove a specific (symbol, exchange, account_type) triple from the
    /// active watchlist.
    pub fn remove_symbol(&mut self, symbol: &str, exchange: &str, account_type: &str) {
        self.mgr.remove_symbol(symbol, exchange, account_type);
    }

    /// Toggle symbol membership: add if missing, remove if present. Returns
    /// the new state (`true` = now in list).
    pub fn toggle_symbol(&mut self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.mgr.toggle_symbol(symbol, exchange, account_type)
    }

    /// Reorder a symbol in the active list's ungrouped section.
    pub fn reorder_symbol(&mut self, from_idx: usize, to_idx: usize) {
        self.mgr.reorder_symbol(from_idx, to_idx);
    }

    /// Move a symbol into a specific group of the active list. No-op if the
    /// active list or the group does not exist.
    pub fn move_to_group(&mut self, symbol: &str, exchange: &str, account_type: &str, group_id: u64) {
        if let Some(list) = self.mgr.active_list_mut() {
            list.move_to_group(symbol, exchange, account_type, group_id);
        }
    }

    /// Add a new group to the active list. Returns the new group id, or 0 if
    /// there is no active list.
    pub fn add_group(&mut self, name: String, color: String) -> u64 {
        let id = self.mgr.next_unique_id();
        if let Some(list) = self.mgr.active_list_mut() {
            list.add_group(id, name, color);
            id
        } else {
            0
        }
    }

    /// Remove a group from the active list by id. Its symbols move to
    /// ungrouped.
    pub fn remove_group(&mut self, group_id: u64) {
        if let Some(list) = self.mgr.active_list_mut() {
            list.remove_group(group_id);
        }
    }

    /// Set a color flag for a symbol in the active list.
    pub fn set_color_flag(&mut self, symbol: &str, exchange: &str, account_type: &str, color: &str) {
        if let Some(list) = self.mgr.active_list_mut() {
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

    /// Check if a (symbol, exchange, account_type) triple is in the active
    /// watchlist.
    pub fn contains(&self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.mgr.contains_with_type(symbol, exchange, account_type)
    }
}
