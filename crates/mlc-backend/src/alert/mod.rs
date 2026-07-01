//! Alert logic: AlertManager + crossing detection.  Value types live
//! in `mlc-core::alert`; this module owns the manager that mutates
//! the bubble's alert slice.
//!
//! `AlertManager` itself is `pub(crate)`-surfaced only — app code accesses
//! it exclusively through the `Alerts` / `AlertsMut` facade (see
//! `BackendCore::alerts` / `alerts_mut` in `crates/mlc-backend/src/lib.rs`).

mod manager;
mod crossing;
mod facade;
pub use manager::AlertManager;
pub use crossing::check_crossings;
pub use facade::{Alerts, AlertsMut};
