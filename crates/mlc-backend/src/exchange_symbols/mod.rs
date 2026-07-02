//! Exchange symbols manager + facade module.
//!
//! `ExchangeSymbolsManager` wraps the per-exchange symbol catalogue cache
//! (previously a bare `HashMap<ExchangeId, Vec<SymbolInfo>>` field on
//! `BackendCore`). App code accesses it exclusively through the
//! `ExchangeSymbols` / `ExchangeSymbolsMut` facade (see
//! `BackendCore::exchange_symbols` / `exchange_symbols_mut` in
//! `crates/mlc-backend/src/lib.rs`).

mod manager;
mod facade;
pub use manager::ExchangeSymbolsManager;
pub use facade::{ExchangeSymbols, ExchangeSymbolsMut};
