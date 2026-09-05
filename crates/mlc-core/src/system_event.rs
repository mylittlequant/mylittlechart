//! System event bus — one durable per-profile log of everything the system
//! told the user (tier changes, sign-in/out, shares, nick edits, sync
//! status, memory pressure, updates, alert deliveries), plus a one-shot
//! toast queue the shells' transient toast painters drain.
//!
//! Pure data — no `uzor`, no network, no rendering (crate contract: see
//! `mylittlechart/CLAUDE.md`'s crate-contract table). `ChartApp` is the one
//! writer (single-writer doctrine); shells only report facts through its
//! `push_system_event` method, never through this type directly.
//!
//! Scope/decision: `docs/mlc/plans/users-paid-features-scope-2026-09-05.md`
//! §8. Facts: `docs/mlc/research/system-toasts-and-event-log-recon-2026-09-05.md`.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Ring capacity for [`SystemEventLog`] — oldest events fall off past this
/// many, per-profile (owner spec: "кольцо на 200 записей на профиль").
pub const SYSTEM_EVENT_LOG_CAPACITY: usize = 200;

/// What kind of fact this event records — lets the panel (added by a later
/// agent) filter/iconify by category without parsing free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SystemEventKind {
    /// Sign-in / sign-out.
    Account,
    /// Paid-tier transition (redeemed, expired, renewed).
    Tier,
    /// A live/screenshot share was created or revoked.
    Share,
    /// Account nickname saved or refused.
    Nick,
    /// Cloud sync status: errors, conflicts, cloud-profile restore.
    Sync,
    /// App update available / downloaded.
    Update,
    /// An alert fired and was delivered (mirrors every `ToastNotification`
    /// the alert-delivery engine produces).
    Alert,
    /// Memory-watchdog pressure warning.
    Memory,
    /// Agent API fact — today, only "a key-minting/revoke or an HTTP
    /// request found the vault sealed" (owner ruling 2026-09-05/06, agent
    /// API auth + `SecretStore` wiring plan).
    Agent,
    /// Anything else — also the serde fallback for a future variant an
    /// older build doesn't know about, and for an unset field on load.
    #[default]
    Info,
}

/// One fact the system told the user.
///
/// `#[serde(default)]` on every field: this type is only ever constructed by
/// [`SystemEventLog::push`], but a profile written before this type existed
/// has no `system_events` key at all — the defaults here are what a
/// partially-shaped or foreign-edited entry falls back to rather than
/// failing the whole profile load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemEvent {
    /// Id unique within its [`SystemEventLog`] — assigned by `push`, never
    /// reused, monotonically increasing (NOT reset when the ring trims).
    #[serde(default)]
    pub id: u64,
    /// Unix milliseconds when the event was recorded.
    #[serde(default)]
    pub at_ms: i64,
    #[serde(default)]
    pub kind: SystemEventKind,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// Whether the events panel has been opened since this fired.
    #[serde(default)]
    pub read: bool,
    /// Whether this event was ALSO meant to show as a transient toast —
    /// independent of `read`: a toast can expire long before the event is
    /// marked read, and a non-toast event (e.g. `Memory`) never shows one.
    #[serde(default)]
    pub toast: bool,
    /// One-shot: `true` until some shell's toast painter drains it via
    /// [`SystemEventLog::take_pending_toasts`]. Never (de)serialized —
    /// reloading a profile must never replay a past session's toasts.
    #[serde(skip)]
    toast_pending: bool,
}

/// Ring-buffered log of [`SystemEvent`]s, newest last, capped at
/// [`SYSTEM_EVENT_LOG_CAPACITY`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEventLog {
    #[serde(default)]
    pub events: VecDeque<SystemEvent>,
    #[serde(default = "default_next_id")]
    next_id: u64,
}

fn default_next_id() -> u64 {
    1
}

impl SystemEventLog {
    pub fn new() -> Self {
        Self {
            events: VecDeque::new(),
            next_id: default_next_id(),
        }
    }

    /// Record one fact and trim the ring to [`SYSTEM_EVENT_LOG_CAPACITY`].
    /// Returns a reference to the stored event (id/at_ms already assigned)
    /// so the caller never needs a second lookup.
    pub fn push(
        &mut self,
        kind: SystemEventKind,
        title: impl Into<String>,
        body: impl Into<String>,
        toast: bool,
    ) -> &SystemEvent {
        let id = self.next_id;
        self.next_id += 1;
        self.events.push_back(SystemEvent {
            id,
            at_ms: now_ms(),
            kind,
            title: title.into(),
            body: body.into(),
            read: false,
            toast,
            toast_pending: toast,
        });
        while self.events.len() > SYSTEM_EVENT_LOG_CAPACITY {
            self.events.pop_front();
        }
        // Safe: a `push_back` just happened immediately above and
        // `SYSTEM_EVENT_LOG_CAPACITY` is > 0, so the trim loop can never
        // empty the ring — `back()` always finds the event just pushed.
        self.events.back().expect("push_back above guarantees a non-empty ring")
    }

    /// Number of events not yet marked read — the toolbar badge count.
    pub fn unread_count(&self) -> usize {
        self.events.iter().filter(|e| !e.read).count()
    }

    /// Mark every event read (panel's "mark all read" action).
    pub fn mark_all_read(&mut self) {
        for e in self.events.iter_mut() {
            e.read = true;
        }
    }

    /// Mark one event read by id. No-op if `id` fell off the ring.
    pub fn mark_read(&mut self, id: u64) {
        if let Some(e) = self.events.iter_mut().find(|e| e.id == id) {
            e.read = true;
        }
    }

    /// Newest-first iterator — the panel's natural read order.
    pub fn iter_newest_first(&self) -> impl Iterator<Item = &SystemEvent> {
        self.events.iter().rev()
    }

    /// Count of events still waiting for a toast paint, without draining
    /// them — the read-only counterpart to [`Self::take_pending_toasts`],
    /// for inspection surfaces (e.g. the admin-tier `GET .../ui/state` test
    /// hook) that must not consume the queue just by looking at it.
    pub fn pending_toast_count(&self) -> usize {
        self.events.iter().filter(|e| e.toast_pending).count()
    }

    /// One-shot drain of every event still waiting for a toast paint,
    /// oldest first. A second call — this frame or any later one — never
    /// returns an event already drained once, even if it is still unread.
    pub fn take_pending_toasts(&mut self) -> Vec<SystemEvent> {
        let mut drained = Vec::new();
        for e in self.events.iter_mut() {
            if e.toast_pending {
                e.toast_pending = false;
                drained.push(e.clone());
            }
        }
        drained
    }
}

impl Default for SystemEventLog {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_assigns_increasing_ids_and_returns_the_stored_event() {
        let mut log = SystemEventLog::new();
        let first_id = log.push(SystemEventKind::Info, "a", "b", false).id;
        let second_id = log.push(SystemEventKind::Tier, "c", "d", true).id;
        assert_eq!(first_id, 1);
        assert_eq!(second_id, 2);
        assert_eq!(log.events.len(), 2);
    }

    #[test]
    fn ring_trims_to_capacity_keeping_the_newest() {
        let mut log = SystemEventLog::new();
        for i in 0..(SYSTEM_EVENT_LOG_CAPACITY + 10) {
            log.push(SystemEventKind::Info, format!("event {i}"), "", false);
        }
        assert_eq!(log.events.len(), SYSTEM_EVENT_LOG_CAPACITY);
        // The oldest 10 fell off — the newest-last entry is the very last push.
        assert_eq!(
            log.events.back().map(|e| e.title.clone()),
            Some(format!("event {}", SYSTEM_EVENT_LOG_CAPACITY + 9))
        );
        // The front entry is exactly the 11th pushed ("event 10").
        assert_eq!(log.events.front().map(|e| e.title.clone()), Some("event 10".to_string()));
    }

    #[test]
    fn unread_count_and_mark_all_read() {
        let mut log = SystemEventLog::new();
        log.push(SystemEventKind::Account, "a", "", false);
        log.push(SystemEventKind::Tier, "b", "", false);
        assert_eq!(log.unread_count(), 2);
        log.mark_all_read();
        assert_eq!(log.unread_count(), 0);
    }

    #[test]
    fn mark_read_marks_only_the_named_event() {
        let mut log = SystemEventLog::new();
        let id_a = log.push(SystemEventKind::Account, "a", "", false).id;
        log.push(SystemEventKind::Tier, "b", "", false);
        log.mark_read(id_a);
        assert_eq!(log.unread_count(), 1);
    }

    #[test]
    fn take_pending_toasts_is_one_shot() {
        let mut log = SystemEventLog::new();
        log.push(SystemEventKind::Tier, "toast me", "", true);
        log.push(SystemEventKind::Memory, "no toast", "", false);
        let drained = log.take_pending_toasts();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].title, "toast me");
        // Second drain the same frame (or any later one) returns nothing —
        // the event is still in `events`/still unread, just not pending.
        assert!(log.take_pending_toasts().is_empty());
        assert_eq!(log.events.len(), 2);
    }

    #[test]
    fn iter_newest_first_reverses_insertion_order() {
        let mut log = SystemEventLog::new();
        log.push(SystemEventKind::Info, "first", "", false);
        log.push(SystemEventKind::Info, "second", "", false);
        let titles: Vec<&str> = log.iter_newest_first().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["second", "first"]);
    }

    #[test]
    fn old_profile_json_without_system_events_deserializes_via_field_default() {
        #[derive(Serialize, Deserialize)]
        struct LegacyHolder {
            #[serde(default)]
            system_events: SystemEventLog,
        }
        let legacy: LegacyHolder = serde_json::from_str("{}").expect("must deserialize legacy json");
        assert_eq!(legacy.system_events.events.len(), 0);
        assert_eq!(legacy.system_events.unread_count(), 0);
    }
}
