//! `ExchangeSymbols` / `ExchangeSymbolsMut` — the only doctrinally-sanctioned
//! access path to `ExchangeSymbolsManager` from outside `mlc-backend`.
//!
//! `ExchangeSymbolsManager` never touches `DataBubble` — same leaf-lock
//! shape as `Presets`/`Watchlists`.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use digdigdig3::ExchangeId;

use super::manager::ExchangeSymbolsManager;
use super::CatalogueSymbol;

/// Read-only facade over `ExchangeSymbolsManager`.
pub struct ExchangeSymbols<'a> {
    mgr: RwLockReadGuard<'a, ExchangeSymbolsManager>,
}

impl<'a> ExchangeSymbols<'a> {
    pub(crate) fn new(mgr: RwLockReadGuard<'a, ExchangeSymbolsManager>) -> Self {
        Self { mgr }
    }

    /// The whole catalogue by reference — for callers that scan across every
    /// exchange (symbol search result builder).
    pub fn as_map(&self) -> &std::collections::HashMap<ExchangeId, Vec<CatalogueSymbol>> {
        self.mgr.as_map()
    }

    /// Symbols for one exchange.
    pub fn symbols_for(&self, exchange_id: ExchangeId) -> Option<&[CatalogueSymbol]> {
        self.mgr.symbols_for(exchange_id)
    }

    /// Resolve an `ExchangeId` by its short string label (e.g. "binance").
    pub fn find_by_label(&self, label: &str) -> Option<ExchangeId> {
        self.mgr.find_by_label(label)
    }

    /// All exchange labels with a populated catalogue entry.
    pub fn exchange_labels(&self) -> impl Iterator<Item = String> + '_ {
        self.mgr.exchange_labels()
    }

    /// Tick size for `symbol` on the exchange identified by its string
    /// `label`.
    pub fn tick_size_for(&self, label: &str, symbol: &str) -> Option<f64> {
        self.mgr.tick_size_for(label, symbol)
    }

    /// Base units per contract — see
    /// [`ExchangeSymbolsManager::contract_size_for`].
    pub fn contract_size_for(&self, label: &str, symbol: &str, futures: bool) -> f64 {
        self.mgr.contract_size_for(label, symbol, futures)
    }

    /// Whether this venue quotes derivative sizes in contracts — see
    /// [`ExchangeSymbolsManager::quotes_contracts`].
    pub fn quotes_contracts(&self, label: &str) -> bool {
        self.mgr.quotes_contracts(label)
    }
}

/// Mutable facade over `ExchangeSymbolsManager`.
pub struct ExchangeSymbolsMut<'a> {
    mgr: RwLockWriteGuard<'a, ExchangeSymbolsManager>,
}

impl<'a> ExchangeSymbolsMut<'a> {
    pub(crate) fn new(mgr: RwLockWriteGuard<'a, ExchangeSymbolsManager>) -> Self {
        Self { mgr }
    }

    /// The whole catalogue by reference.
    pub fn as_map(&self) -> &std::collections::HashMap<ExchangeId, Vec<CatalogueSymbol>> {
        self.mgr.as_map()
    }

    /// Symbols for one exchange.
    pub fn symbols_for(&self, exchange_id: ExchangeId) -> Option<&[CatalogueSymbol]> {
        self.mgr.symbols_for(exchange_id)
    }

    /// Resolve an `ExchangeId` by its short string label.
    pub fn find_by_label(&self, label: &str) -> Option<ExchangeId> {
        self.mgr.find_by_label(label)
    }

    /// Tick size for `symbol` on the exchange identified by its string
    /// `label`.
    pub fn tick_size_for(&self, label: &str, symbol: &str) -> Option<f64> {
        self.mgr.tick_size_for(label, symbol)
    }

    /// Replace the whole catalogue for one exchange (`SymbolsLoaded` write
    /// site).
    pub fn set_symbols(&mut self, exchange_id: ExchangeId, symbols: Vec<CatalogueSymbol>) {
        self.mgr.set_symbols(exchange_id, symbols);
    }

    /// Replace the whole catalogue wholesale (startup seed).
    pub fn replace_all(&mut self, by_exchange: std::collections::HashMap<ExchangeId, Vec<CatalogueSymbol>>) {
        self.mgr.replace_all(by_exchange);
    }
}
