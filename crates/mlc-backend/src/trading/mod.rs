//! Trading manager + facade module.
//!
//! `TradingManager` (order/position/session registry, paper-trading
//! engines) lives here per doctrine 3.7c — value types
//! (`Order`/`Position`/`Fill`/`TradingConfig`/`TradingSnapshot`/…) stay in
//! `mlc-backend-trading-engine`, which this module depends on for
//! construction. App code accesses the manager exclusively through the
//! `Trading` / `TradingMut` facade (see `BackendCore::trading` /
//! `trading_mut` in `crates/mlc-backend/src/lib.rs`).

mod order_manager;
mod position_tracker;
mod paper_engine;
mod manager;
mod facade;

pub use manager::TradingManager;
pub use facade::{Trading, TradingMut};
