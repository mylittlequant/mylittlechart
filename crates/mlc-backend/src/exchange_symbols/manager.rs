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
    ///
    /// Matches the raw catalogue string FIRST (works unmodified for venues
    /// where canonical == native, e.g. Binance), then falls back to a
    /// canonical `base+quote` comparison so a dash/underscore/slash-native
    /// venue (OKX `"ETH-USDT"`, Gate.io `"ETH_USDT"`, ...) still resolves
    /// when `symbol` was passed in the OTHER form (e.g. continuous
    /// `"ETHUSDT"`, or vice versa) — mirrors the same tolerance
    /// `DataBridge::wire_symbol` and `build_symbol_search_results`'
    /// `canonical_key` already rely on elsewhere. Bug found 2026-07-25:
    /// `build_symbol_search_results` hands back the RAW native form for a
    /// symbol whenever its canonical key collides with another catalogue
    /// entry, so callers (trading-container rebind, standalone DOM spawn)
    /// cannot assume `symbol` is always in one fixed form.
    pub fn tick_size_for(&self, label: &str, symbol: &str) -> Option<f64> {
        let syms = self.by_exchange.iter().find(|(eid, _)| eid.as_str() == label)?.1;
        if let Some(hit) = syms.iter().find(|s| s.symbol == symbol) {
            return hit.tick_size;
        }
        let req_canon = canonical_form(symbol)?;
        syms.iter()
            .find(|s| canonical_form_of(s).as_deref() == Some(req_canon.as_str()))
            .and_then(|s| s.tick_size)
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

/// Canonical `BASE+QUOTE` key (uppercase, no separator) for a catalogue
/// entry — `None` when either asset field is empty. Mirrors `ChartApp::
/// build_symbol_search_results`'s local `canonical_key` helper (mlc-app).
fn canonical_form_of(s: &SymbolInfo) -> Option<String> {
    if s.base_asset.is_empty() || s.quote_asset.is_empty() {
        return None;
    }
    Some(format!("{}{}", s.base_asset, s.quote_asset).to_uppercase())
}

/// Canonical `BASE+QUOTE` key for an arbitrary symbol STRING — accepts
/// `"ETH-USDT"` / `"ETH/USDT"` / `"ETH_USDT"` (split on the separator) or
/// continuous `"ETHUSDT"` (quote-suffix stripping). `None` when no quote
/// asset could be determined either way. Mirrors `DataBridge`'s
/// `parse_canonical_symbol` (mlc-backend-data-bridge).
fn canonical_form(symbol: &str) -> Option<String> {
    if let Some((b, q)) = symbol.split_once(['-', '/', '_']) {
        return (!b.is_empty() && !q.is_empty()).then(|| format!("{}{}", b, q).to_uppercase());
    }
    let upper = symbol.to_uppercase();
    ["USDT", "USDC", "USD", "BTC", "ETH", "BUSD", "EUR", "JPY"]
        .into_iter()
        .find(|q| upper.strip_suffix(q).is_some_and(|base| !base.is_empty()))
        .map(|_| upper)
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

    /// Dash-native venue (OKX) — `tick_size_for` must resolve regardless of
    /// which form (raw native `"ETH-USDT"` or continuous canonical
    /// `"ETHUSDT"`) the caller passes, since `build_symbol_search_results`
    /// can legitimately hand back EITHER form depending on catalogue
    /// collisions (2026-07-25 OKX rebind tick-size bug).
    #[test]
    fn tick_size_for_tolerates_dash_native_symbol() {
        let mut mgr = ExchangeSymbolsManager::new();
        let okx_eth = SymbolInfo {
            symbol: "ETH-USDT".to_string(),
            base_asset: "ETH".to_string(),
            quote_asset: "USDT".to_string(),
            tick_size: Some(0.01),
            ..Default::default()
        };
        mgr.set_symbols(ExchangeId::OKX, vec![okx_eth]);

        // Raw native form — exact match, unchanged behavior.
        assert_eq!(mgr.tick_size_for("okx", "ETH-USDT"), Some(0.01));
        // Continuous canonical form — falls back to base+quote match.
        assert_eq!(mgr.tick_size_for("okx", "ETHUSDT"), Some(0.01));
        // Slash/underscore separators also tolerated.
        assert_eq!(mgr.tick_size_for("okx", "ETH/USDT"), Some(0.01));
        assert_eq!(mgr.tick_size_for("okx", "ETH_USDT"), Some(0.01));
        // A genuinely different pair still misses.
        assert_eq!(mgr.tick_size_for("okx", "BTCUSDT"), None);
    }
}
