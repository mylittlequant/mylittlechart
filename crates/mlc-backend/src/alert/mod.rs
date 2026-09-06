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
mod store;
pub use manager::AlertManager;
pub use crossing::check_crossings;
pub use facade::{Alerts, AlertsMut};

/// Ring capacity for a bubble created purely to back a Price alert with no
/// open window (`BackendCore::ensure_alert_bubble`) — a short tail, not the
/// 200k-bar default `create_bubble_auto` bubbles otherwise carry (an alert
/// only ever needs the live tick, never display-depth history). See
/// `docs/mlc/plans/alerts-account-wide-and-telegram-delivery-2026-09-06.md` §2 C3.2.
pub(crate) const ALERT_BUBBLE_KLINE_LAYER_CAPACITY: usize = 64;
