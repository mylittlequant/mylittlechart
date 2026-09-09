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

/// How long a toast stays on screen before it expires — the one duration
/// every toast painter in this codebase already used (native's pre-restyle
/// `render_toasts`, the web shell's own `render_system_toasts`,
/// `mlc_alert_delivery::ToastNotification`'s own default).
pub const TOAST_DURATION_MS: f64 = 5_000.0;

/// One active, on-screen toast — normalized out of a drained
/// [`mlc_core::SystemEvent`] by [`ChartApp::refresh_active_toasts`] so
/// neither shell (nor the shared layout painter) has to touch
/// `SystemEventLog`'s own bookkeeping (`toast`/`toast_pending`) to draw
/// one. `id` is the originating `SystemEvent::id` — stable across frames
/// and never reused — so the ✕ and the drag handle key off it instead of a
/// Vec index (an index shifts the moment an earlier toast expires or is
/// dismissed).
#[derive(Clone, Debug)]
pub struct ToastItem {
    pub id: u64,
    pub title: String,
    pub body: String,
    /// Wall-clock ms (whatever the shell's own frame clock reads — native's
    /// `web_time` epoch millis, the web shell's own `now_ms`) this toast
    /// was shown. Deliberately NOT `SystemEvent::at_ms`: a toast fades on
    /// its OWN reveal clock, not on when the underlying fact happened.
    pub shown_at_ms: f64,
}

impl ToastItem {
    /// `1.0` = just shown, `0.0` = expired — the fade curve every toast
    /// painter in this codebase already applied over the final 20% of a
    /// toast's life.
    pub fn remaining_fraction(&self, now_ms: f64) -> f64 {
        let elapsed = (now_ms - self.shown_at_ms).max(0.0);
        (1.0 - elapsed / TOAST_DURATION_MS).clamp(0.0, 1.0)
    }

    pub fn is_expired(&self, now_ms: f64) -> bool {
        now_ms - self.shown_at_ms >= TOAST_DURATION_MS
    }
}

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

    /// Drain the bus's pending toasts into the visible queue, honouring
    /// quiet mode (`ProfileUiSettings::toasts_enabled`), and drop whatever
    /// already expired. Call once per frame per window that is actually on
    /// screen — same scope [`Self::drain_system_toasts`] itself already
    /// had. Quiet mode still drains (a suppressed toast must not pile up in
    /// `SystemEventLog`'s one-shot queue) — it just never reaches
    /// [`Self::active_toasts`], so the event still lands in the panel and
    /// the unread badge (both read `system_events()` directly, untouched by
    /// this).
    pub fn refresh_active_toasts(&mut self, now_ms: f64) {
        let enabled = self.core.user().profile().ui.toasts_enabled;
        for event in self.system_events.take_pending_toasts() {
            if enabled {
                self.active_toasts.push(ToastItem {
                    id: event.id,
                    title: event.title,
                    body: event.body,
                    shown_at_ms: now_ms,
                });
            }
        }
        self.active_toasts.retain(|t| !t.is_expired(now_ms));
    }

    /// Currently visible toasts, newest last (stack order) — the shared
    /// layout painter's own input.
    pub fn active_toasts(&self) -> &[ToastItem] {
        &self.active_toasts
    }

    /// Dismiss one toast by id (the ✕) — every other active toast is
    /// untouched.
    pub fn dismiss_toast(&mut self, id: u64) {
        self.active_toasts.retain(|t| t.id != id);
    }

    /// Dismiss the newest (most-recently-shown) toast — keyboard Escape.
    /// No-op when nothing is showing.
    pub fn dismiss_newest_toast(&mut self) {
        self.active_toasts.pop();
    }
}
