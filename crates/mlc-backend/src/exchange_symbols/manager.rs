//! `ExchangeSymbolsManager` — per-exchange symbol catalogue cache.
//!
//! Wraps `HashMap<ExchangeId, Vec<SymbolInfo>>` (previously a bare field on
//! `BackendCore`). Populated asynchronously from
//! `LiveUpdate::SymbolsLoaded` (once per exchange); read frequently by the
//! symbol search overlay and the watchlist. Never touches `DataBubble`.

use std::collections::HashMap;

use digdigdig3::ExchangeId;
use mlc_backend_data_bridge::SymbolInfo;

#[derive(Debug, Default)]
pub struct ExchangeSymbolsManager {
    by_exchange: HashMap<ExchangeId, Vec<SymbolInfo>>,
}

impl ExchangeSymbolsManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The whole catalogue by reference — for callers that scan across every
    /// exchange (symbol search result builder).
    pub fn as_map(&self) -> &HashMap<ExchangeId, Vec<SymbolInfo>> {
        &self.by_exchange
    }

    /// Symbols for one exchange.
    pub fn symbols_for(&self, exchange_id: ExchangeId) -> Option<&[SymbolInfo]> {
        self.by_exchange.get(&exchange_id).map(Vec::as_slice)
    }

    /// Resolve an `ExchangeId` by its short string label (e.g. "binance").
    /// Only matches exchanges that already have a populated catalogue entry.
    pub fn find_by_label(&self, label: &str) -> Option<ExchangeId> {
        self.by_exchange.keys().find(|eid| eid.as_str() == label).copied()
    }

    /// All exchange labels with a populated catalogue entry.
    pub fn exchange_labels(&self) -> impl Iterator<Item = String> + '_ {
        self.by_exchange.keys().map(|e| e.as_str().to_string())
    }

    /// Tick size for `symbol` on the exchange identified by its string
    /// `label`. Returns `None` if the exchange or symbol isn't catalogued,
    /// or the catalogue entry has no tick size.
    pub fn tick_size_for(&self, label: &str, symbol: &str) -> Option<f64> {
        self.by_exchange
            .iter()
            .find(|(eid, _)| eid.as_str() == label)
            .and_then(|(_, syms)| syms.iter().find(|s| s.symbol == symbol).and_then(|s| s.tick_size))
    }

    /// Replace the whole catalogue for one exchange (`SymbolsLoaded` write
    /// site).
    pub fn set_symbols(&mut self, exchange_id: ExchangeId, symbols: Vec<SymbolInfo>) {
        self.by_exchange.insert(exchange_id, symbols);
    }

    /// Replace the whole catalogue wholesale (startup seed).
    pub fn replace_all(&mut self, by_exchange: HashMap<ExchangeId, Vec<SymbolInfo>>) {
        self.by_exchange = by_exchange;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(symbol: &str, tick_size: Option<f64>) -> SymbolInfo {
        SymbolInfo {
            symbol: symbol.to_string(),
            tick_size,
            ..Default::default()
        }
    }

    #[test]
    fn find_by_label_and_tick_size_roundtrip() {
        let mut mgr = ExchangeSymbolsManager::new();
        mgr.set_symbols(ExchangeId::Binance, vec![sym("BTCUSDT", Some(0.1))]);

        assert_eq!(mgr.find_by_label("binance"), Some(ExchangeId::Binance));
        assert_eq!(mgr.tick_size_for("binance", "BTCUSDT"), Some(0.1));
        assert_eq!(mgr.tick_size_for("binance", "ETHUSDT"), None);
        assert_eq!(mgr.tick_size_for("bybit", "BTCUSDT"), None);
    }

    #[test]
    fn replace_all_swaps_catalogue() {
        let mut mgr = ExchangeSymbolsManager::new();
        mgr.set_symbols(ExchangeId::Binance, vec![sym("BTCUSDT", None)]);

        let mut replacement = HashMap::new();
        replacement.insert(ExchangeId::Bybit, vec![sym("ETHUSDT", None)]);
        mgr.replace_all(replacement);

        assert!(mgr.symbols_for(ExchangeId::Binance).is_none());
        assert!(mgr.symbols_for(ExchangeId::Bybit).is_some());
    }
}
