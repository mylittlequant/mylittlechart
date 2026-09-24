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
//! # Pieces landed so far (M1, M2)
//!
//! **M1** — the wire-boundary primitives: Matrix identifier newtypes
//! ([`ids`]), this crate's error type ([`error`]), the
//! `OutgoingRequest`/`HttpResponseDescriptor` shapes ([`wire`]), and the
//! flush-before-send barrier's data structures ([`persist`]).
//!
//! **M2** — the working-set store ([`store`]): [`store::Store`], the
//! in-memory `CryptoStore`/`StateStore` implementation later pieces
//! (crypto, room, sync engine) code against, sealed for export through
//! [`persist::FlushGate`] by a [`store::RecordCodec`] (this piece ships only
//! `store::InsecurePlainCodecForTests`, gated behind the `test-support`
//! Cargo feature; the real AES-256-GCM codec is a later piece, M14).
//!
//! **M3** — the `/sync` and event wire types ([`wire::events`],
//! [`wire::sync`]): [`wire::sync::SyncResponse`] and
//! [`wire::sync::parse_sync_response`], the Matrix event envelope
//! ([`wire::events::RawEvent`], [`wire::events::StrippedStateEvent`],
//! [`wire::events::ToDeviceEvent`]), relation aggregation
//! ([`wire::events::RelatesTo`]), and typed content for the state/timeline/
//! to-device/account-data/ephemeral event types this crate reads.
//!
//! **M5** — Matrix canonical-JSON signing ([`canonical_json`]:
//! [`canonical_json::sign_json`], [`canonical_json::verify_json_signature`])
//! and the device's own Olm account ([`crypto::account::OlmAccountState`]):
//! generate-once-and-persist identity, one-time/fallback key maintenance off
//! `/sync`'s own counters, and the signed `device_keys`/`/keys/upload`
//! request bodies built from it.
//!
//! **M4** — room state, timeline assembly, and relation aggregation
//! ([`room`]): [`room::RoomState`] folded from `m.room.*` state events plus
//! [`room::RoomKind`] derivation, [`room::Timeline`] assembled from `/sync`
//! windows and `/messages` back-pages with local-echo reconciliation and
//! live v11-allow-list redaction, and [`room::RelationsBundle`] aggregation
//! (reactions, edits, replies, threads).
//!
//! Device tracking, Olm sessions, Megolm group sessions, and the sync
//! engine tying everything together (plan §5, M6-M15) are later pieces and
//! are not present in this crate yet.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod canonical_json;
pub mod crypto;
pub mod error;
pub mod ids;
pub mod persist;
pub mod room;
pub mod store;
pub mod wire;

pub use canonical_json::{sign_json, verify_json_signature, CanonicalJsonError};
pub use crypto::account::OlmAccountState;
pub use error::MessengerError;
pub use ids::{DeviceId, EventId, RequestId, RoomId, TxnId, UserId};
pub use persist::{FlushBatch, FlushGate, RecordKey, SealedRecord};
pub use room::{
    aggregate_relation, highlight, last_preview, recompute_bundle, EditRecord, GapMarker, ItemContent, MemberRole,
    MemberState, RelatingEvent, RelationsBundle, RoomKind, RoomState, SendState, StateChangeSummary, Timeline,
    TimelineItem,
};
pub use store::{CryptoStore, RecordCodec, StateStore, Store, StoreError};
pub use wire::{HttpMethod, HttpResponseDescriptor, OutgoingRequest, OutgoingRequestKind};
