//! `ExchangeSymbolsManager` — per-exchange symbol catalogue cache.
//!
//! Wraps `HashMap<ExchangeId, Vec<CatalogueSymbol>>` (previously a bare
//! field on `BackendCore`, and a raw `Vec<SymbolInfo>` before the
//! 2026-09-05 lean-catalogue cutover — see `crate::catalogue_ingest`).
//! `LiveUpdate::SymbolsLoaded` only enqueues the exchange id;
//! `BackendCore::ingest_catalogues` is what actually calls `set_symbols`
//! once an exchange's catalogue finishes its budget-sliced conversion.
//! Read frequently by the symbol search overlay and the watchlist. Never
//! touches `DataBubble`.

use std::collections::HashMap;

use digdigdig3::ExchangeId;

use super::catalogue_symbol::{contract_size_key, CatalogueSymbol};

#[derive(Debug, Default)]
pub struct ExchangeSymbolsManager {
    by_exchange: HashMap<ExchangeId, Vec<CatalogueSymbol>>,
}

impl ExchangeSymbolsManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The whole catalogue by reference — for callers that scan across every
    /// exchange (symbol search result builder).
    pub fn as_map(&self) -> &HashMap<ExchangeId, Vec<CatalogueSymbol>> {
        &self.by_exchange
    }

    /// Symbols for one exchange.
    pub fn symbols_for(&self, exchange_id: ExchangeId) -> Option<&[CatalogueSymbol]> {
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

    /// Base units per contract for `symbol` on the venue identified by
    /// `label`. `1.0` whenever the venue quotes sizes in base units already,
    /// or when the catalogue cannot answer.
    ///
    /// Several venues report derivative sizes in CONTRACTS, not coin: Gate's
    /// BTC_USDT perp contract is 0.0001 BTC, OKX's BTC-USDT-SWAP is 0.01 BTC,
    /// KuCoin's XBTUSDTM is 0.001 BTC. Read as coin, a 369-contract Gate
    /// liquidation prices out at $23M instead of $2.3k — a factor of ten
    /// thousand, which is enough for one bad print to set the scale for
    /// every real one (owner report 2026-08-02: the liquidation overlay drew
    /// every genuine print at minimum radius).
    ///
    /// The multiplier is read from the venue's own exchange-info record —
    /// [`CatalogueSymbol::contract_multiplier`], extracted from the raw
    /// `SymbolInfo.extra` payload ONCE at ingest — under the key that venue
    /// uses. The key table is per-venue on purpose: a blind search for "any
    /// field that looks like a multiplier" would find one on venues that
    /// quote in base units and silently rescale correct data.
    pub fn contract_size_for(&self, label: &str, symbol: &str, futures: bool) -> f64 {
        if !futures {
            return 1.0;
        }
        if contract_size_key(label).is_none() {
            return 1.0;
        }
        let Some(syms) = self.by_exchange.iter().find(|(eid, _)| eid.as_str() == label).map(|(_, v)| v)
        else { return 1.0 };
        let req = canonical_form(symbol);
        // The primary match is the venue's OWN id for this instrument,
        // derived with the same normalizer the subscription path uses.
        // Neither typed-field path is reliable on a derivative record:
        // OKX leaves `baseCcy`/`quoteCcy` EMPTY on SWAP rows (they are spot
        // fields) so base+quote is "", and Gate puts `underlying:
        // "BTC_USDT"` in the base and `settle: "usdt"` in the quote so
        // base+quote reads "BTC_USDTUSDT". The normalizer knows both, plus
        // KuCoin's XBT alias and its `M` suffix.
        let native = native_futures_id(label, symbol);
        let matches_symbol = |s: &CatalogueSymbol| {
            s.symbol == symbol
                || native.as_deref() == Some(s.symbol.as_str())
                || (req.is_some()
                    && (canonical_form(&s.symbol) == req || canonical_form_of(s) == req))
        };
        let is_futures = |s: &CatalogueSymbol| {
            !matches!(s.account_type, digdigdig3::AccountType::Spot | digdigdig3::AccountType::Margin)
        };
        // Prefer a futures record, but do not require one: only a
        // derivative record carries the multiplier field at all, so a
        // catalogue that mislabels the market type still resolves.
        syms.iter()
            .find(|s| is_futures(s) && matches_symbol(s) && s.contract_multiplier.is_some())
            .or_else(|| syms.iter().find(|s| matches_symbol(s) && s.contract_multiplier.is_some()))
            .and_then(|s| s.contract_multiplier)
            .unwrap_or(1.0)
    }

    /// Whether this venue quotes derivative sizes in CONTRACTS — i.e.
    /// whether [`Self::contract_size_for`] returning `1.0` means "one base
    /// unit per contract" or "the catalogue has not answered yet".
    ///
    /// A caller that cannot tell those apart will price a Gate print at
    /// 10 000× while the symbol list is still loading, and that single
    /// print then sets the scale for every later one.
    pub fn quotes_contracts(&self, label: &str) -> bool {
        contract_size_key(label).is_some()
    }

    /// Replace the whole catalogue for one exchange (`SymbolsLoaded` write
    /// site).
    pub fn set_symbols(&mut self, exchange_id: ExchangeId, symbols: Vec<CatalogueSymbol>) {
        self.by_exchange.insert(exchange_id, symbols);
    }

    /// Replace the whole catalogue wholesale (startup seed).
    pub fn replace_all(&mut self, by_exchange: HashMap<ExchangeId, Vec<CatalogueSymbol>>) {
        self.by_exchange = by_exchange;
    }
}

/// The venue's own perpetual-futures id for a canonical symbol, e.g.
/// `("okx", "BTCUSDT") -> "BTC-USDT-SWAP"`, `("kucoin", "BTCUSDT") ->
/// "XBTUSDTM"`. `None` when the venue label or the pair does not resolve.
///
/// Same derivation the subscription path uses, so a catalogue lookup and a
/// stream key agree on which record describes the instrument.
fn native_futures_id(label: &str, symbol: &str) -> Option<String> {
    let eid = digdigdig3::ExchangeId::from_str(label)?;
    let canonical = if let Some((b, q)) = symbol.split_once(['-', '/', '_']) {
        digdigdig3::Symbol::new(b, q)
    } else {
        let upper = symbol.to_uppercase();
        ["USDT", "USDC", "USD", "BTC", "ETH", "BUSD", "EUR", "JPY"]
            .into_iter()
            .find_map(|q| {
                upper
                    .strip_suffix(q)
                    .filter(|base| !base.is_empty())
                    .map(|base| digdigdig3::Symbol::new(base, q))
            })?
    };
    digdigdig3::core::utils::SymbolNormalizer::to_exchange(
        eid,
        &canonical,
        digdigdig3::AccountType::FuturesCross,
    )
    .ok()
}

/// Canonical `BASE+QUOTE` key (uppercase, no separator) for a catalogue
/// entry — `None` when either asset field is empty. Mirrors `ChartApp::
/// build_symbol_search_results`'s local `canonical_key` helper (mlc-app).
fn canonical_form_of(s: &CatalogueSymbol) -> Option<String> {
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
    use digdigdig3::SymbolInfo;

    fn sym(symbol: &str, tick_size: Option<f64>) -> CatalogueSymbol {
        CatalogueSymbol {
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

    /// Gate's futures record is the reason `contract_size_for` matches on
    /// the canonicalised NATIVE symbol and not only on base+quote: the
    /// venue reports `underlying` as the base asset and `settle` as the
    /// quote, so the base+quote key is "BTC_USDTUSDT".
    #[test]
    fn contract_size_reads_gate_quanto_multiplier() {
        let mut mgr = ExchangeSymbolsManager::new();
        // Raw station record, as the venue's exchange_info connector would
        // hand it to the bridge — `extra` carries the native payload.
        let gate_btc_raw = SymbolInfo {
            symbol: "BTC_USDT".to_string(),
            base_asset: "BTC_USDT".to_string(),
            quote_asset: "usdt".to_string(),
            account_type: digdigdig3::AccountType::FuturesCross,
            extra: serde_json::json!({ "name": "BTC_USDT", "quanto_multiplier": "0.0001" }),
            ..Default::default()
        };
        // `from_symbol_info` is the ONLY place `extra` is ever read (ingest
        // time) — proves the multiplier survives the raw → lean conversion.
        let gate_btc = CatalogueSymbol::from_symbol_info("gateio", &gate_btc_raw);
        assert_eq!(gate_btc.contract_multiplier, Some(0.0001));
        mgr.set_symbols(ExchangeId::GateIO, vec![gate_btc]);

        assert_eq!(mgr.contract_size_for("gateio", "BTCUSDT", true), 0.0001);
        assert_eq!(mgr.contract_size_for("gateio", "BTC_USDT", true), 0.0001);
        // Spot never carries a contract multiplier.
        assert_eq!(mgr.contract_size_for("gateio", "BTCUSDT", false), 1.0);
        // A venue that quotes base units has no key and must stay at 1.
        assert_eq!(mgr.contract_size_for("binance", "BTCUSDT", true), 1.0);
    }

    /// OKX leaves `baseCcy`/`quoteCcy` EMPTY on SWAP records — they are
    /// spot-only fields — so neither typed path can match and the
    /// normalizer-derived native id is the only thing that resolves it.
    #[test]
    fn contract_size_reads_okx_ctval() {
        let mut mgr = ExchangeSymbolsManager::new();
        let okx_btc_raw = SymbolInfo {
            symbol: "BTC-USDT-SWAP".to_string(),
            base_asset: String::new(),
            quote_asset: String::new(),
            account_type: digdigdig3::AccountType::FuturesCross,
            extra: serde_json::json!({ "instId": "BTC-USDT-SWAP", "ctVal": "0.01" }),
            ..Default::default()
        };
        let okx_btc = CatalogueSymbol::from_symbol_info("okx", &okx_btc_raw);
        assert_eq!(okx_btc.contract_multiplier, Some(0.01));
        mgr.set_symbols(ExchangeId::OKX, vec![okx_btc]);

        assert_eq!(mgr.contract_size_for("okx", "BTCUSDT", true), 0.01);
        assert_eq!(mgr.contract_size_for("okx", "BTC-USDT-SWAP", true), 0.01);
    }

    /// Dash-native venue (OKX) — `tick_size_for` must resolve regardless of
    /// which form (raw native `"ETH-USDT"` or continuous canonical
    /// `"ETHUSDT"`) the caller passes, since `build_symbol_search_results`
    /// can legitimately hand back EITHER form depending on catalogue
    /// collisions (2026-07-25 OKX rebind tick-size bug).
    #[test]
    fn tick_size_for_tolerates_dash_native_symbol() {
        let mut mgr = ExchangeSymbolsManager::new();
        let okx_eth = CatalogueSymbol {
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
