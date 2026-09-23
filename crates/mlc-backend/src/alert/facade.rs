//! `Alerts` / `AlertsMut` — the only doctrinally-sanctioned access path to
//! `AlertManager` from outside `mlc-backend`.
//!
//! Each facade struct holds both the `AlertManager` guard and the
//! `BubbleStore` guard for the duration of the call chain. App code never
//! sees `DataBubble` or the raw `AlertManager` — it calls
//! `core.alerts().X(...)` / `core.alerts_mut().X(...)`.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::alert::{AlertItem, AlertPatch, AlertSpec, AlertStatus, DrawingExtendMode};
use mlc_core::ids::BubbleId;
use mlc_core::instrument::InstrumentAddress;

use crate::bubble::BubbleStore;

use super::manager::AlertManager;

/// Read-only facade over `AlertManager` + `BubbleStore`.
pub struct Alerts<'a> {
    mgr: RwLockReadGuard<'a, AlertManager>,
    bubbles: RwLockReadGuard<'a, BubbleStore>,
}

impl<'a> Alerts<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, AlertManager>, bubbles: RwLockReadGuard<'a, BubbleStore>) -> Self {
        Self { mgr, bubbles }
    }

    /// All alert items targeting one bubble's instrument.
    pub fn items(&self, bid: BubbleId) -> Vec<AlertItem> {
        self.mgr.items(&self.bubbles, bid)
    }

    /// All alert items across every account.
    pub fn items_all(&self) -> Vec<AlertItem> {
        self.mgr.items_all()
    }

    /// Find an alert by ID, scoped to a known bubble's instrument.
    pub fn get(&self, bid: BubbleId, id: u64) -> Option<&AlertItem> {
        self.mgr.get(&self.bubbles, bid, id)
    }

    /// Search the whole store for an alert id; returns the live bubble (if
    /// any) whose instrument it targets, plus a clone of the alert.
    pub fn find_owner(&self, id: u64) -> Option<(BubbleId, AlertItem)> {
        self.mgr.find_owner(&self.bubbles, id)
    }

    /// Clone of every alert targeting one bubble's instrument (for preset save).
    pub fn snapshot_for_bid(&self, bid: BubbleId) -> Vec<AlertItem> {
        self.mgr.snapshot_for_bid(&self.bubbles, bid)
    }

    /// Total alert count across every account.
    pub fn len_total(&self) -> usize {
        self.mgr.len_total()
    }

    /// Every alert belonging to `account_id`, across every instrument
    /// (account-wide surface, Arc C3).
    pub fn for_account(&self, account_id: &str) -> Vec<AlertItem> {
        self.mgr.for_account(account_id)
    }

    /// Every alert belonging to `account_id` on `address`.
    pub fn for_address(&self, account_id: &str, address: &InstrumentAddress) -> Vec<AlertItem> {
        self.mgr.for_address(account_id, address)
    }
}

/// Mutable facade over `AlertManager` + `BubbleStore`.
pub struct AlertsMut<'a> {
    mgr: RwLockWriteGuard<'a, AlertManager>,
    bubbles: RwLockWriteGuard<'a, BubbleStore>,
}

impl<'a> AlertsMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, AlertManager>, bubbles: RwLockWriteGuard<'a, BubbleStore>) -> Self {
        Self { mgr, bubbles }
    }

    /// Create a new alert on `bid`. Returns the new alert's id.
    pub fn add(&mut self, bid: BubbleId, spec: AlertSpec) -> u64 {
        self.mgr.add(&mut self.bubbles, bid, spec)
    }

    /// Apply a partial update to an existing alert. Returns `true` if found.
    pub fn update(&mut self, bid: BubbleId, id: u64, patch: AlertPatch) -> bool {
        self.mgr.update(&self.bubbles, bid, id, patch)
    }

    /// Remove an alert by ID. Returns `true` if found and removed.
    pub fn remove(&mut self, bid: BubbleId, id: u64) -> bool {
        self.mgr.remove(&mut self.bubbles, bid, id)
    }

    /// Flip `Active <-> Paused`. Returns the new status, or `None` if not found.
    pub fn toggle_pause(&mut self, bid: BubbleId, id: u64) -> Option<AlertStatus> {
        self.mgr.toggle_pause(&self.bubbles, bid, id)
    }

    /// Check all active alerts on `bid` using dynamic price resolution.
    /// `current_price` is `None` when `bid` has no real price yet — see
    /// `AlertManager::on_tick`'s own doc. Returns the list of triggered
    /// alert IDs.
    pub fn on_tick(
        &mut self,
        bid: BubbleId,
        current_price: Option<f64>,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Vec<u64> {
        self.mgr.on_tick(&self.bubbles, bid, current_price, current_bar, drawing_points, indicator_values)
    }

    /// Check all active `Signal`-source alerts on `bid` against a batch of
    /// newly-emitted signals. Returns the list of triggered alert IDs.
    pub fn on_signals(&mut self, bid: BubbleId, new_signals: &[(u64, usize, i8, u8, String)]) -> Vec<u64> {
        self.mgr.on_signals(&self.bubbles, bid, new_signals)
    }

    /// Remove every alert attached to `primitive_id`, across all bubbles.
    /// Returns the removed items so the caller can push undo commands.
    pub fn clear_for_drawing(&mut self, primitive_id: u64) -> Vec<AlertItem> {
        self.mgr.clear_for_drawing(&mut self.bubbles, primitive_id)
    }

    /// Remove every alert attached to `indicator_id`, across all bubbles.
    /// Returns the removed count.
    pub fn clear_for_indicator(&mut self, indicator_id: u64) -> usize {
        self.mgr.clear_for_indicator(&mut self.bubbles, indicator_id)
    }

    /// Insert a single alert item verbatim (undo/redo restore, preset restore).
    pub fn restore_one(&mut self, bid: BubbleId, item: AlertItem) {
        self.mgr.restore_one(&mut self.bubbles, bid, item)
    }

    /// Create a new alert for `account_id` from an [`AlertSpec`] — own
    /// counter, address-keyed (account-wide surface, Arc C3). The caller
    /// ensures a subscription exists first via
    /// `BackendCore::ensure_alert_bubble`. Returns the new alert's id.
    pub fn add_for_account(&mut self, account_id: impl Into<String>, spec: AlertSpec) -> u64 {
        self.mgr.add_for_account(&mut self.bubbles, account_id, spec)
    }

    /// Apply a partial update to an existing alert by its global id.
    /// Returns `true` if found.
    pub fn update_by_id(&mut self, id: u64, patch: AlertPatch) -> bool {
        self.mgr.update_by_id(id, patch)
    }

    /// Remove an alert by its global id. Returns `true` if found and removed.
    pub fn remove_by_id(&mut self, id: u64) -> bool {
        self.mgr.remove_by_id(&mut self.bubbles, id)
    }

    /// Insert a single alert item verbatim under its own `account_id`
    /// (migration fold + sync-pull entry, Arc C3.3/C3.4).
    pub fn restore_item(&mut self, item: AlertItem) {
        self.mgr.restore_item(&mut self.bubbles, item)
    }

    /// Remove and return only the `last_price` entries for `bubble_ids` as a
    /// fresh, small `AlertManager`. Used exclusively by preset tab park
    /// (shared-BackendCore arc §5.3) — replaces whole-manager `swap()`,
    /// which under a shared core would drop every other window's crossing-
    /// detection cache too. Alert *data* itself travels via
    /// `BubblesMut::extract_bubbles`, not through this call.
    pub fn extract_for_bubbles(&mut self, bubble_ids: &[BubbleId]) -> AlertManager {
        self.mgr.extract_for_bubbles(bubble_ids)
    }

    /// Re-insert every `last_price` entry from `other`. Used exclusively by
    /// preset tab unpark.
    pub fn merge_for_bubbles(&mut self, other: AlertManager) {
        self.mgr.merge_for_bubbles(other)
    }
}
