//! The flush-before-send barrier's data structures.
//!
//! No store logic lives here — `store/` (the actual `CryptoStore`/
//! `StateStore` traits and their sealed-record codec) is a later piece.
//! This module only holds the primitives decision #3 of
//! `docs/mlc/plans/messenger-proto-client-crates-2026-09-24.md` requires:
//! a way for the core to (a) mark in-memory state dirty as it mutates, (b)
//! hand a batch of that dirty state to a shell to write durably, and (c)
//! know, before releasing any outgoing network request, whether every
//! mutation that request could depend on has already been written and
//! acknowledged.
//!
//! # Ordering guarantee — read this before calling [`FlushGate::can_release_requests`]
//!
//! [`FlushGate`] enforces a **global**, not a per-record, barrier: it does
//! not track which outgoing request depends on which dirty record, because
//! it never sees requests at all (that coupling belongs to a later piece,
//! `outgoing_queue`/`sync_engine`, M12/M13). Instead it makes one flat
//! guarantee, and callers must respect the protocol below for that
//! guarantee to mean anything:
//!
//! - [`FlushGate::take_batch`] atomically drains **every** record marked
//!   dirty/deleted since the previous [`FlushGate::take_batch`] call into
//!   one [`FlushBatch`], and marks that batch's `id` unacknowledged.
//! - [`FlushGate::can_release_requests`] is `true` **only** when there is
//!   (a) no unacknowledged batch outstanding, **and** (b) nothing currently
//!   dirty that has not yet even been drained into a batch. Point (b)
//!   matters: a mutation that has been marked dirty but not yet batched has
//!   not even started its trip to durable storage, so nothing may depend on
//!   it being safe yet, regardless of how old any *other*, already-acked
//!   batch is.
//! - **Required calling protocol**: a caller must call [`take_batch`] (and
//!   hand the result to a shell to persist) for everything dirtied so far
//!   *before* it produces any outgoing request that might depend on that
//!   state, in the same synchronous step (this crate has no concurrency —
//!   see the crate doc — so "before" is a plain, total call-sequence
//!   order). A request produced immediately after such a `take_batch` call
//!   is therefore never blocked by dirty records marked *afterwards*: those
//!   records postdate the request and cannot be a dependency of it. This is
//!   the precise sense of "requests produced after a flush are not blocked
//!   by later dirty records" that this module's tests exercise — not that
//!   later dirty data is ignored, but that it opens its own, independent
//!   flush cycle rather than resurrecting or extending the previous one.
//! - [`FlushGate::ack`] on a batch id that was never produced by
//!   [`take_batch`] (already acked, or never issued) is a no-op — it never
//!   panics and never affects any other batch's state.
//!
//! [`take_batch`]: FlushGate::take_batch

use std::collections::{BTreeMap, BTreeSet};

/// A storage key for one sealed record, namespaced under
/// `messenger/{device_id}/...` (see the plan's `store/sealed.rs` layout —
/// not yet implemented in this crate; this type only carries the key
/// string, agnostic of that later layout).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordKey(String);

impl RecordKey {
    /// Builds a record key from its full storage path.
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// The record's storage path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RecordKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One sealed (already encrypted) record ready for a shell to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedRecord {
    /// The record's storage key.
    pub key: RecordKey,
    /// The sealed (already-encrypted) bytes to write at `key`.
    pub bytes: Vec<u8>,
}

/// One flush unit: every record write and key deletion a shell must
/// perform, together, before calling [`FlushGate::ack`] with `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlushBatch {
    /// This batch's sequence id, assigned in the order
    /// [`FlushGate::take_batch`] produced it. [`FlushGate::ack`] is keyed
    /// by this id.
    pub id: u64,
    /// Sealed records to write.
    pub records: Vec<SealedRecord>,
    /// Keys to delete (tombstones) — riding in the same batch as any
    /// writes dirtied or deleted in the same [`FlushGate::take_batch`]
    /// call, so a shell performs both kinds of change in one durable step.
    pub deletes: Vec<RecordKey>,
}

/// A key is either pending a write or pending a deletion; the latest call
/// for a given key before the next [`FlushGate::take_batch`] wins.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingOp {
    Write(Vec<u8>),
    Delete,
}

/// The flush-before-send barrier's state machine. See the module doc for
/// the exact ordering guarantee [`can_release_requests`] makes.
///
/// [`can_release_requests`]: FlushGate::can_release_requests
#[derive(Debug, Default)]
pub struct FlushGate {
    next_batch_id: u64,
    pending: BTreeMap<RecordKey, PendingOp>,
    unacked_batches: BTreeSet<u64>,
}

impl FlushGate {
    /// Builds an empty gate: nothing dirty, nothing outstanding.
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks `key` dirty with new sealed bytes to write on the next
    /// [`take_batch`](Self::take_batch) call. Overwrites any earlier
    /// pending write or deletion for the same `key` that has not yet been
    /// batched — only the latest state before a flush is written.
    pub fn mark_dirty(&mut self, key: RecordKey, bytes: Vec<u8>) {
        self.pending.insert(key, PendingOp::Write(bytes));
    }

    /// Marks `key` for deletion on the next
    /// [`take_batch`](Self::take_batch) call. Overwrites any earlier
    /// pending write or deletion for the same `key` that has not yet been
    /// batched.
    pub fn mark_deleted(&mut self, key: RecordKey) {
        self.pending.insert(key, PendingOp::Delete);
    }

    /// Drains everything currently pending into one new [`FlushBatch`],
    /// or returns `None` if nothing is pending. The returned batch's `id`
    /// is unacknowledged until [`ack`](Self::ack) is called with it.
    pub fn take_batch(&mut self) -> Option<FlushBatch> {
        if self.pending.is_empty() {
            return None;
        }
        let id = self.next_batch_id;
        self.next_batch_id += 1;
        let mut records = Vec::new();
        let mut deletes = Vec::new();
        for (key, op) in std::mem::take(&mut self.pending) {
            match op {
                PendingOp::Write(bytes) => records.push(SealedRecord { key, bytes }),
                PendingOp::Delete => deletes.push(key),
            }
        }
        self.unacked_batches.insert(id);
        Some(FlushBatch { id, records, deletes })
    }

    /// Acknowledges that batch `id` has been durably written. A no-op if
    /// `id` was never produced by [`take_batch`](Self::take_batch), or was
    /// already acknowledged.
    pub fn ack(&mut self, id: u64) {
        self.unacked_batches.remove(&id);
    }

    /// `true` iff every batch ever taken has been acknowledged and nothing
    /// is currently pending (dirtied but not yet batched) — see the module
    /// doc for why both conditions are required.
    pub fn can_release_requests(&self) -> bool {
        self.unacked_batches.is_empty() && self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_blocked_until_prior_flush_acked() {
        let mut gate = FlushGate::new();
        gate.mark_dirty(RecordKey::new("messenger/dev/account"), b"v1".to_vec());
        assert!(!gate.can_release_requests(), "dirty and not yet batched");

        let batch = gate.take_batch().expect("something was pending");
        assert!(!gate.can_release_requests(), "batched but not yet acked");

        gate.ack(batch.id);
        assert!(gate.can_release_requests(), "flushed and acked: safe to release");
    }

    #[test]
    fn requests_produced_after_a_flush_are_not_blocked_by_later_dirty_records() {
        let mut gate = FlushGate::new();
        gate.mark_dirty(RecordKey::new("messenger/dev/a"), b"1".to_vec());
        let batch1 = gate.take_batch().expect("first batch");
        gate.ack(batch1.id);
        assert!(
            gate.can_release_requests(),
            "fully flushed and acked: safe to release requests produced right now"
        );

        // A later, independent mutation (e.g. the NEXT Olm ratchet advance)
        // opens its own flush cycle. It correctly blocks release again
        // (its own bytes are not on disk yet) -- but it does so on its own
        // terms, not by resurrecting or extending batch1's bookkeeping.
        gate.mark_dirty(RecordKey::new("messenger/dev/b"), b"2".to_vec());
        assert!(!gate.can_release_requests(), "new dirty data is not yet on disk");

        let batch2 = gate.take_batch().expect("second batch");
        assert_ne!(batch1.id, batch2.id, "each flush cycle gets a fresh id");
        gate.ack(batch2.id);
        assert!(
            gate.can_release_requests(),
            "second cycle flushed independently of the first"
        );
    }

    #[test]
    fn ack_of_unknown_batch_is_ignored() {
        let mut gate = FlushGate::new();
        gate.ack(999); // never issued -- must not panic
        assert!(gate.can_release_requests());

        gate.mark_dirty(RecordKey::new("messenger/dev/a"), b"1".to_vec());
        let batch = gate.take_batch().expect("pending");
        gate.ack(batch.id + 1); // wrong id -- ignored, real batch stays unacked
        assert!(!gate.can_release_requests());

        gate.ack(batch.id);
        assert!(gate.can_release_requests());
    }

    #[test]
    fn deletes_ride_in_the_batch() {
        let mut gate = FlushGate::new();
        gate.mark_dirty(RecordKey::new("messenger/dev/keep"), b"kept".to_vec());
        gate.mark_deleted(RecordKey::new("messenger/dev/gone"));

        let batch = gate.take_batch().expect("pending");
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].key.as_str(), "messenger/dev/keep");
        assert_eq!(batch.records[0].bytes, b"kept".to_vec());
        assert_eq!(batch.deletes.len(), 1);
        assert_eq!(batch.deletes[0].as_str(), "messenger/dev/gone");
    }

    #[test]
    fn take_batch_returns_none_when_nothing_is_pending() {
        let mut gate = FlushGate::new();
        assert!(gate.take_batch().is_none());
    }

    #[test]
    fn a_later_write_to_the_same_key_overwrites_an_earlier_pending_delete() {
        let mut gate = FlushGate::new();
        let key = RecordKey::new("messenger/dev/x");
        gate.mark_deleted(key.clone());
        gate.mark_dirty(key.clone(), b"new".to_vec());

        let batch = gate.take_batch().expect("pending");
        assert!(batch.deletes.is_empty());
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].bytes, b"new".to_vec());
    }
}
