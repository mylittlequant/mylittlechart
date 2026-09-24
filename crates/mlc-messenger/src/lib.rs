//! `mlc-messenger` — sans-I/O Matrix Client-Server-subset sync + E2EE
//! (Olm/Megolm) engine for MyLittleChart's messenger
//! (`docs/mlc/plans/messenger-proto-client-crates-2026-09-24.md`).
//!
//! # Contract
//!
//! This crate performs **no I/O of any kind**: no network sockets, no
//! filesystem access, no browser storage calls, and no `async`/`.await`
//! anywhere in its source (plan §0). Every side effect crosses the
//! boundary as plain data:
//!
//! - **Network**: the core hands out [`wire::OutgoingRequest`] values; a
//!   shell (native or web) performs the actual HTTP call and feeds the
//!   result back in as an [`wire::HttpResponseDescriptor`]. The core never
//!   opens a socket itself.
//! - **Disk/persistence**: the core owns an in-memory working set and
//!   marks records dirty as it mutates state; [`persist::FlushGate`]
//!   batches dirty/deleted records into [`persist::FlushBatch`] values a
//!   shell writes to durable storage (the filesystem on native, IndexedDB
//!   on web) and acks back in. The core never calls a storage API itself.
//!
//! # Flush-before-send barrier
//!
//! [`persist::FlushGate::can_release_requests`] is the barrier the plan's
//! manager decision #3 requires: a state mutation (an Olm/Megolm ratchet
//! advance, most critically) must be durably flushed and acknowledged
//! *before* the ciphertext that used it is allowed to leave the process.
//! See `persist`'s module doc for the exact ordering guarantee this makes.
//!
//! # Synchronous by construction
//!
//! No function in this crate's public API returns a `Future` or is
//! declared `async fn`. This is what lets the same source compile and
//! unit-test identically on native and `wasm32-unknown-unknown`: the only
//! platform-divergent code is which transport/executor a *shell* wires in,
//! never this crate.
//!
//! # This piece (M1)
//!
//! Only the wire-boundary primitives land here: Matrix identifier newtypes
//! ([`ids`]), this crate's error type ([`error`]), the
//! `OutgoingRequest`/`HttpResponseDescriptor` shapes ([`wire`]), and the
//! flush-before-send barrier's data structures ([`persist`]). The sync
//! engine, room/crypto state, and store traits land in later pieces
//! (plan §5, M2-M15) and are not present in this crate yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod ids;
pub mod persist;
pub mod wire;

pub use error::MessengerError;
pub use ids::{DeviceId, EventId, RequestId, RoomId, TxnId, UserId};
pub use persist::{FlushBatch, FlushGate, RecordKey, SealedRecord};
pub use wire::{HttpMethod, HttpResponseDescriptor, OutgoingRequest, OutgoingRequestKind};
