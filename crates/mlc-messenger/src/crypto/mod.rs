//! The Olm/Megolm/cross-signing state machine — plan §3's `crypto/`.
//!
//! # Pieces landed so far (M5)
//!
//! **M5** — the device's own Olm account ([`account::OlmAccountState`]):
//! generate-once-and-persist identity, one-time/fallback key maintenance
//! off `/sync`'s own counters, and building the signed `device_keys`/
//! `/keys/upload` request bodies (via [`crate::canonical_json`]).
//!
//! Device tracking, Olm sessions, Megolm group sessions, withheld/key-request
//! bookkeeping, SAS verification, key backup, and cross-signing (plan's
//! `device_tracker.rs`, `olm_sessions.rs`, `group_sessions.rs`,
//! `withheld.rs`, `verification.rs`, `backup.rs`, `cross_signing.rs`, and the
//! `CryptoMachine` type tying them together) are later pieces and are not
//! present in this crate yet.

pub mod account;
