//! Value types + config for the trading system.
//!
//! `TradingManager` (the stateful registry: sessions, order managers,
//! position trackers, paper-trading engines) moved to
//! `mlc_backend::trading::manager` in the 3.7c manager lift (mirrors 3.7a's
//! indicator manager lift) — this crate keeps only compute-free value types
//! (re-exported `digdigdig3` domain types + `Fill`/`SessionInfo`/…),
//! `TradingConfig` (load/save + paper-mode flags), `TradingError`, and
//! `TradingSnapshot` (the read-only published state consumers subscribe to).

pub mod types;
pub mod error;
pub mod config;
pub mod snapshot;

pub use snapshot::{TradingSnapshot, SharedTradingSnapshot};
pub use error::{TradingError, TradingResult};
pub use config::TradingConfig;
pub use types::*;
