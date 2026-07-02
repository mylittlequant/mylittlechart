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

    /// All alert items on one bubble.
    pub fn items(&self, bid: BubbleId) -> &[AlertItem] {
        self.mgr.items(&self.bubbles, bid)
    }

    /// All alert items across every bubble.
    pub fn items_all(&self) -> Vec<AlertItem> {
        self.mgr.items_all(&self.bubbles)
    }

    /// Find an alert by ID on a known bubble.
    pub fn get(&self, bid: BubbleId, id: u64) -> Option<&AlertItem> {
        self.mgr.get(&self.bubbles, bid, id)
    }

    /// Search every bubble for an alert id; returns the owning bubble plus
    /// a clone of the alert.
    pub fn find_owner(&self, id: u64) -> Option<(BubbleId, AlertItem)> {
        self.mgr.find_owner(&self.bubbles, id)
    }

    /// Clone of all alerts on one bubble (for preset save).
    pub fn snapshot_for_bid(&self, bid: BubbleId) -> Vec<AlertItem> {
        self.mgr.snapshot_for_bid(&self.bubbles, bid)
    }

    /// Total alert count across every bubble.
    pub fn len_total(&self) -> usize {
        self.mgr.len_total(&self.bubbles)
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
        self.mgr.update(&mut self.bubbles, bid, id, patch)
    }

    /// Remove an alert by ID. Returns `true` if found and removed.
    pub fn remove(&mut self, bid: BubbleId, id: u64) -> bool {
        self.mgr.remove(&mut self.bubbles, bid, id)
    }

    /// Flip `Active <-> Paused`. Returns the new status, or `None` if not found.
    pub fn toggle_pause(&mut self, bid: BubbleId, id: u64) -> Option<AlertStatus> {
        self.mgr.toggle_pause(&mut self.bubbles, bid, id)
    }

    /// Check all active alerts on `bid` using dynamic price resolution.
    /// Returns the list of triggered alert IDs.
    pub fn on_tick(
        &mut self,
        bid: BubbleId,
        current_price: f64,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Vec<u64> {
        self.mgr.on_tick(&mut self.bubbles, bid, current_price, current_bar, drawing_points, indicator_values)
    }

    /// Check all active `Signal`-source alerts on `bid` against a batch of
    /// newly-emitted signals. Returns the list of triggered alert IDs.
    pub fn on_signals(&mut self, bid: BubbleId, new_signals: &[(u64, usize, i8, u8, String)]) -> Vec<u64> {
        self.mgr.on_signals(&mut self.bubbles, bid, new_signals)
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
