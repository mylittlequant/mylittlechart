//! System event bus — `ChartApp` owns the one durable log of everything the
//! system told the user (tier/account/share/nick/sync/memory/update/alert
//! facts). Shells report facts through the methods here; they never touch
//! `mlc_core::SystemEventLog` directly (single-writer doctrine, same shape
//! as every other `ChartApp`-owned domain state).
//!
//! Scope/decision: `docs/mlc/plans/users-paid-features-scope-2026-09-05.md`
//! §8. Facts: `docs/mlc/research/system-toasts-and-event-log-recon-2026-09-05.md`.
//! The panel that reads this API (bottom-of-right-toolbar bell icon +
//! `RightSidebarPanel::Events`) is a follow-up arc — this module only ships
//! the bus and the read/write surface it needs.

use crate::ChartApp;

impl ChartApp {
    /// Record one fact the system just told the user. `toast` requests a
    /// transient toast the next time a shell drains
    /// [`Self::drain_system_toasts`] — the event itself is kept in the log
    /// regardless of `toast`.
    pub fn push_system_event(
        &mut self,
        kind: mlc_core::SystemEventKind,
        title: impl Into<String>,
        body: impl Into<String>,
        toast: bool,
    ) -> &mlc_core::SystemEvent {
        self.system_events.push(kind, title, body, toast)
    }

    /// Read-only view of the full log, newest-last on disk / oldest-first —
    /// use [`mlc_core::SystemEventLog::iter_newest_first`] for panel display
    /// order. The panel agent's entry point.
    pub fn system_events(&self) -> &mlc_core::SystemEventLog {
        &self.system_events
    }

    /// Mutable access for the panel's per-row "mark read" action.
    pub fn system_events_mut(&mut self) -> &mut mlc_core::SystemEventLog {
        &mut self.system_events
    }

    /// Number of events not yet marked read — the toolbar badge count.
    pub fn unread_count(&self) -> usize {
        self.system_events.unread_count()
    }

    /// Number of events still waiting for a toast paint — read-only, does
    /// not drain (see [`mlc_core::SystemEventLog::pending_toast_count`]).
    pub fn pending_toast_count(&self) -> usize {
        self.system_events.pending_toast_count()
    }

    /// Mark every event read (panel's "mark all read" action) and persist —
    /// same dirty-flag pattern every other sidebar-state mutation uses.
    pub fn mark_all_system_events_read(&mut self) {
        self.system_events.mark_all_read();
        self.persist_profile();
    }

    /// One-shot drain of events still waiting for a toast paint. Called by
    /// both shells' per-frame toast loop; each event surfaces as a toast at
    /// most once, regardless of how many frames it sits in the log after.
    pub fn drain_system_toasts(&mut self) -> Vec<mlc_core::SystemEvent> {
        self.system_events.take_pending_toasts()
    }
}
