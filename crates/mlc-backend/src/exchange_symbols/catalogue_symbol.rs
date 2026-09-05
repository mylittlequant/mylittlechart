//! `CatalogueSymbol` — MLC's own lean per-symbol catalogue record.
//!
//! Built once from a `digdigdig3::SymbolInfo` at ingest
//! (`BackendCore::ingest_catalogues` / `BackendCore::
//! seed_exchange_symbols_from_bridge`) and never stores the venue's raw
//! `extra: serde_json::Value` record — the field responsible for the
//! 2-4s render-tick stall measured 2026-09-05
//! (`docs/mlc/research/wasm-lag-measurements-2026-09-05.md`): a
//! 11 720-symbol catalogue's `Vec<SymbolInfo>` deep-clones that JSON blob
//! per record, twice (broadcast producer + every receiver's `try_recv`).
//! The ONLY fact MLC ever reads out of `extra` is the venue's contract
//! multiplier (`ExchangeSymbolsManager::contract_size_for`), extracted here
//! so the raw payload can be dropped immediately after conversion.

use digdigdig3::{AccountType, SymbolInfo};

/// One venue's catalogued instrument — MLC's lean projection of
/// `digdigdig3::SymbolInfo`. Every field a chart-layer caller actually reads
/// (symbol search, watchlist, tick/contract-size lookups); nothing else —
/// see the module doc for why `extra` is not here. A caller that genuinely
/// needs the raw `SymbolInfo` (nothing in MLC does today) reads it through
/// `DataBridge::station().for_each_symbol` at that call site instead of
/// storing it.
#[derive(Debug, Clone, Default)]
pub struct CatalogueSymbol {
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub status: String,
    pub account_type: AccountType,
    pub price_precision: u8,
    pub quantity_precision: u8,
    pub tick_size: Option<f64>,
    pub step_size: Option<f64>,
    pub min_quantity: Option<f64>,
    pub max_quantity: Option<f64>,
    /// Base units per derivative contract, extracted from `SymbolInfo.extra`
    /// at ingest via [`contract_size_key`] — `None` when the venue quotes
    /// derivative sizes in base units already, or when the raw record
    /// carried no value under the venue's key. Consumed by
    /// `ExchangeSymbolsManager::contract_size_for`.
    pub contract_multiplier: Option<f64>,
}

impl CatalogueSymbol {
    /// Convert one station catalogue record. `label` is the venue's short
    /// slug (`ExchangeId::as_str()`) — selects which `extra` field, if any,
    /// carries the contract multiplier (see [`contract_size_key`]).
    pub fn from_symbol_info(label: &str, s: &SymbolInfo) -> Self {
        let contract_multiplier = contract_size_key(label)
            .and_then(|key| s.extra.get(key))
            .and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|t| t.parse().ok())))
            .filter(|m: &f64| m.is_finite() && *m > 0.0);
        Self {
            symbol: s.symbol.clone(),
            base_asset: s.base_asset.clone(),
            quote_asset: s.quote_asset.clone(),
            status: s.status.clone(),
            account_type: s.account_type,
            price_precision: s.price_precision,
            quantity_precision: s.quantity_precision,
            tick_size: s.tick_size,
            step_size: s.step_size,
            min_quantity: s.min_quantity,
            max_quantity: s.max_quantity,
            contract_multiplier,
        }
    }
}

/// The exchange-info field a venue reports its contract multiplier under,
/// or `None` when the venue quotes derivative sizes in base units.
///
/// Verified against each venue's own raw record (`SymbolInfo.extra`):
/// Gate `quanto_multiplier` ("0.0001" for BTC_USDT), OKX `ctVal` ("0.01"
/// for BTC-USDT-SWAP), KuCoin `multiplier` (0.001 for XBTUSDTM), HTX
/// `contract_size`. Binance and Bybit report base units on their USDⓈ-M
/// perps and are deliberately absent.
///
/// INVERSE (coin-margined) contracts are a different question — there the
/// size is quoted in QUOTE currency and a base-unit multiplier does not
/// apply. Nothing here subscribes them today; a caller that starts to must
/// handle that case rather than reuse this.
pub(crate) fn contract_size_key(label: &str) -> Option<&'static str> {
    match label {
        "gateio" => Some("quanto_multiplier"),
        "okx" => Some("ctVal"),
        "kucoin" => Some("multiplier"),
        "htx" => Some("contract_size"),
        _ => None,
    }
}
