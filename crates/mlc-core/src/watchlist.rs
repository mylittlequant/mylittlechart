//! Watchlist value types — pure domain data, no UI/presentation fields.
//!
//! `WatchlistList` / `WatchlistGroup` hold the user's named watchlists,
//! groups, and per-instrument membership. Presentation concerns (group
//! color, group collapsed state, column visibility, sort-order snapshot,
//! which list is the active tab) live in `mlc-app::watchlist_ui` — see
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1B.
//!
//! `color_flags` (user-authored symbol annotations) stays on `WatchlistList`
//! — it's domain data per the §1B verdict table ("user annotation on
//! instruments, not layout; any UI over the daemon shows the same marks"),
//! not presentation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::instrument::InstrumentAddress;

// =============================================================================
// Serde migration helpers
// =============================================================================

/// Default exchange for migration of old watchlists (plain-string entries
/// predating the exchange field).
fn default_exchange() -> String {
    "binance".to_string()
}

/// Constructs an [`InstrumentAddress`] for a bare-string legacy watchlist
/// entry (pre-exchange-field format). Account type defaults to Spot.
fn address_from_plain_symbol(symbol: String) -> InstrumentAddress {
    InstrumentAddress::new(default_exchange(), symbol, crate::instrument::AccountType::default())
}

/// Deserializes either a plain string list or an `InstrumentAddress` list.
/// Old format: ["BTCUSDT", "ADAUSDT"]
/// New format: [{"symbol": "BTCUSDT", "exchange": "binance", "account_type": "S"}, ...]
fn deserialize_symbols<'de, D>(deserializer: D) -> Result<Vec<InstrumentAddress>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SymbolOrString {
        Symbol(InstrumentAddress),
        Plain(String),
    }

    let items: Vec<SymbolOrString> = Vec::deserialize(deserializer)?;
    Ok(items
        .into_iter()
        .map(|item| match item {
            SymbolOrString::Symbol(addr) => addr,
            SymbolOrString::Plain(s) => address_from_plain_symbol(s),
        })
        .collect())
}

// =============================================================================
// WatchlistGroup
// =============================================================================

/// A named group/section within a watchlist. Presentation fields (`color`,
/// `collapsed`) live in `mlc-app::watchlist_ui::GroupUiState`, sidecar-keyed
/// by `id`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistGroup {
    pub id: u64,
    pub name: String,
    #[serde(deserialize_with = "deserialize_symbols")]
    pub symbols: Vec<InstrumentAddress>,
}

// =============================================================================
// WatchlistList
// =============================================================================

/// A single named watchlist (preset). Presentation fields (`column_config`,
/// `order_snapshot`) live in `mlc-app::watchlist_ui`.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct WatchlistList {
    pub id: u64,
    pub name: String,
    pub groups: Vec<WatchlistGroup>,
    /// Symbols not assigned to any group.
    #[serde(deserialize_with = "deserialize_symbols")]
    pub ungrouped: Vec<InstrumentAddress>,
    /// Per-symbol color flag — user-authored annotation, not layout.
    /// Key = `"symbol:exchange:account_type"`, value = CSS hex color.
    /// An absent entry (or empty string value) means no flag is set.
    #[serde(default)]
    pub color_flags: HashMap<String, String>,
}

impl WatchlistList {
    /// Color-flag map key for `address`.
    ///
    /// NOT `InstrumentAddress::routing_key()` — that format is
    /// `"exchange:symbol:account_type"`, while `color_flags` has always
    /// persisted `"symbol:exchange:account_type"` (see the pre-cutover
    /// `WatchlistList::set_color_flag`/`get_color_flag`, which built this
    /// exact string from three loose `&str` params). Changing the order
    /// would silently orphan every color flag already saved in
    /// `watchlists.json`. Kept as its own private helper — the ONLY place
    /// this string is built — so the format is centralized even though it
    /// intentionally diverges from `routing_key()`.
    fn color_flag_key(address: &InstrumentAddress) -> String {
        format!("{}:{}:{}", address.symbol, address.exchange, address.account_type)
    }

    /// Set a color flag for an instrument.
    ///
    /// Passing an empty `color` removes the flag.
    pub fn set_color_flag(&mut self, address: &InstrumentAddress, color: &str) {
        let key = Self::color_flag_key(address);
        if color.is_empty() {
            self.color_flags.remove(&key);
        } else {
            self.color_flags.insert(key, color.to_string());
        }
    }

    /// Get the color flag for an instrument, or `None` if none is set.
    pub fn get_color_flag(&self, address: &InstrumentAddress) -> Option<&str> {
        let key = Self::color_flag_key(address);
        self.color_flags.get(&key).map(|s| s.as_str())
    }

    /// All symbols in this list (groups + ungrouped), preserving order.
    pub fn all_symbols(&self) -> Vec<&InstrumentAddress> {
        let mut out = Vec::new();
        for g in &self.groups {
            for s in &g.symbols {
                out.push(s);
            }
        }
        for s in &self.ungrouped {
            out.push(s);
        }
        out
    }

    /// Check if an instrument is in this list.
    pub fn contains(&self, address: &InstrumentAddress) -> bool {
        self.ungrouped.iter().any(|s| s == address)
            || self.groups.iter().any(|g| g.symbols.iter().any(|s| s == address))
    }

    /// Check if a symbol (any exchange) is in this list.
    pub fn contains_symbol(&self, symbol: &str) -> bool {
        self.ungrouped.iter().any(|s| s.symbol == symbol)
            || self.groups.iter().any(|g| g.symbols.iter().any(|s| s.symbol == symbol))
    }

    /// Add an instrument to the ungrouped section. No-op if already present.
    pub fn add_symbol(&mut self, address: InstrumentAddress) {
        if !self.contains(&address) {
            self.ungrouped.push(address);
        }
    }

    /// Remove a specific instrument from everywhere in this list.
    pub fn remove_symbol(&mut self, address: &InstrumentAddress) {
        self.ungrouped.retain(|s| s != address);
        for g in &mut self.groups {
            g.symbols.retain(|s| s != address);
        }
    }

    /// Move an instrument into a specific group. Removes from previous location first.
    pub fn move_to_group(&mut self, address: &InstrumentAddress, group_id: u64) {
        // Find and remove from current location, keeping the address.
        let mut found: Option<InstrumentAddress> = None;
        if let Some(pos) = self.ungrouped.iter().position(|s| s == address) {
            found = Some(self.ungrouped.remove(pos));
        } else {
            for g in &mut self.groups {
                if let Some(pos) = g.symbols.iter().position(|s| s == address) {
                    found = Some(g.symbols.remove(pos));
                    break;
                }
            }
        }
        // Add to target group.
        if let Some(ws) = found {
            if let Some(g) = self.groups.iter_mut().find(|g| g.id == group_id) {
                g.symbols.push(ws);
            }
        }
    }

    /// Add a new (empty) group.
    pub fn add_group(&mut self, id: u64, name: String) {
        self.groups.push(WatchlistGroup { id, name, symbols: Vec::new() });
    }

    /// Remove a group by id. Its symbols move to ungrouped.
    pub fn remove_group(&mut self, group_id: u64) {
        if let Some(idx) = self.groups.iter().position(|g| g.id == group_id) {
            let group = self.groups.remove(idx);
            self.ungrouped.extend(group.symbols);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::AccountType;

    #[test]
    fn test_same_symbol_different_exchanges() {
        let mut list = WatchlistList { id: 1, ..Default::default() };
        let binance_btc = InstrumentAddress::new("Binance", "BTCUSDT", AccountType::Spot);
        let okx_btc = InstrumentAddress::new("OKX", "BTCUSDT", AccountType::Spot);
        let kucoin_btc = InstrumentAddress::new("KuCoin", "BTCUSDT", AccountType::Spot);
        list.add_symbol(binance_btc.clone());
        list.add_symbol(okx_btc.clone());
        assert_eq!(list.ungrouped.len(), 2);
        assert!(list.contains(&binance_btc));
        assert!(list.contains(&okx_btc));
        assert!(!list.contains(&kucoin_btc));
        list.remove_symbol(&okx_btc);
        assert!(list.contains(&binance_btc));
        assert!(!list.contains(&okx_btc));
    }

    #[test]
    fn test_add_symbol_no_duplicates() {
        let mut list = WatchlistList { id: 1, ..Default::default() };
        let aapl = InstrumentAddress::new("Binance", "AAPL", AccountType::Spot);
        list.add_symbol(aapl.clone());
        list.add_symbol(aapl);
        assert_eq!(list.ungrouped.iter().filter(|s| s.symbol == "AAPL").count(), 1);
    }

    #[test]
    fn test_group_operations() {
        let mut list = WatchlistList { id: 1, ..Default::default() };
        let btc = InstrumentAddress::new("Binance", "BTC", AccountType::Spot);
        list.add_symbol(btc.clone());
        list.add_group(10, "Crypto".to_string());
        list.move_to_group(&btc, 10);
        assert!(list.ungrouped.is_empty());
        assert_eq!(list.groups[0].symbols, vec![btc.clone()]);
        list.remove_group(10);
        assert!(list.groups.is_empty());
        assert_eq!(list.ungrouped, vec![btc]);
    }

    #[test]
    fn test_all_symbols_order() {
        let mut list = WatchlistList {
            id: 1,
            ungrouped: vec![InstrumentAddress::new("Binance", "AAPL", AccountType::Spot)],
            ..Default::default()
        };
        list.add_group(1, "Grp".to_string());
        list.groups[0].symbols.push(InstrumentAddress::new("Binance", "BTC", AccountType::Spot));
        let syms = list.all_symbols();
        assert_eq!(syms[0].symbol, "BTC");
        assert_eq!(syms[1].symbol, "AAPL");
    }

    #[test]
    fn test_remove_does_not_cross_account_types() {
        let mut list = WatchlistList { id: 1, ..Default::default() };
        let spot = InstrumentAddress::new("binance", "BTCUSDT", AccountType::Spot);
        let futures = InstrumentAddress::new("binance", "BTCUSDT", AccountType::FuturesCross);
        list.add_symbol(spot.clone());
        list.add_symbol(futures.clone());
        list.remove_symbol(&futures);
        assert!(list.contains(&spot));
        assert!(!list.contains(&futures));
    }

    #[test]
    fn test_deserialize_migration_plain_strings() {
        let json = r#"{
            "id": 1,
            "name": "Legacy",
            "groups": [],
            "ungrouped": ["BTCUSDT", "ETHUSDT"],
            "color_flags": {}
        }"#;
        let list: WatchlistList = serde_json::from_str(json).unwrap();
        assert_eq!(list.ungrouped.len(), 2);
        assert_eq!(list.ungrouped[0].symbol, "BTCUSDT");
        assert_eq!(list.ungrouped[1].symbol, "ETHUSDT");
    }

    #[test]
    fn test_deserialize_new_format_with_exchange() {
        let json = r#"{
            "id": 1,
            "name": "New",
            "groups": [],
            "ungrouped": [{"symbol": "BTCUSDT", "exchange": "OKX", "account_type": "S"}],
            "color_flags": {}
        }"#;
        let list: WatchlistList = serde_json::from_str(json).unwrap();
        assert_eq!(list.ungrouped[0].symbol, "BTCUSDT");
        assert_eq!(list.ungrouped[0].exchange, "OKX");
    }
}
