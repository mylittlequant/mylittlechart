//! Watchlist manager + facade module.
//!
//! `WatchlistManager` lives here (Step 9 extraction into `mlc-watchlist` was
//! a cycle-breaking staging step, reversed by the doctrine 1B split — see
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B). App code accesses
//! it exclusively through the `Watchlists` / `WatchlistsMut` facade (see
//! `BackendCore::watchlists` / `watchlists_mut` in `crates/mlc-backend/src/lib.rs`).

mod manager;
mod facade;
pub use manager::WatchlistManager;
pub use facade::{Watchlists, WatchlistsMut};
