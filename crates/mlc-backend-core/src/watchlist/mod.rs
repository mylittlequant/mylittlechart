//! Watchlist facade module.
//!
//! `WatchlistManager` itself lives in the standalone `mlc-watchlist` crate
//! (Step 3.3/Step 9 extraction). App code accesses it exclusively through the
//! `Watchlists` / `WatchlistsMut` facade (see `BackendCore::watchlists` /
//! `watchlists_mut` in `crates/mlc-backend-core/src/lib.rs`).

mod facade;
pub use facade::{Watchlists, WatchlistsMut};
