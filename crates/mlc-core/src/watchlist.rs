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
    /// Set a color flag for a symbol on a specific exchange and account type.
    ///
    /// Passing an empty `color` removes the flag.
    /// Key format: `"symbol:exchange:account_type"` (e.g. `"BTCUSDT:Binance:FC"`).
    pub fn set_color_flag(&mut self, symbol: &str, exchange: &str, account_type: &str, color: &str) {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
        if color.is_empty() {
            self.color_flags.remove(&key);
        } else {
            self.color_flags.insert(key, color.to_string());
        }
    }

    /// Get the color flag for a symbol on a specific exchange and account type, or `None` if none is set.
    pub fn get_color_flag(&self, symbol: &str, exchange: &str, account_type: &str) -> Option<&str> {
        let key = format!("{}:{}:{}", symbol, exchange, account_type);
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

    /// Check if a (symbol, exchange, account_type) triple is in this list.
    pub fn contains(&self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.contains_with_type(symbol, exchange, account_type)
    }

    /// Check if a (symbol, exchange, account_type) triple is in this list.
    pub fn contains_with_type(&self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        let at: crate::instrument::AccountType = account_type.parse().unwrap_or_default();
        self.ungrouped.iter().any(|s| s.symbol == symbol && s.exchange == exchange && s.account_type == at)
            || self.groups.iter().any(|g| g.symbols.iter().any(|s| s.symbol == symbol && s.exchange == exchange && s.account_type == at))
    }

    /// Check if a symbol (any exchange) is in this list.
    pub fn contains_symbol(&self, symbol: &str) -> bool {
        self.ungrouped.iter().any(|s| s.symbol == symbol)
            || self.groups.iter().any(|g| g.symbols.iter().any(|s| s.symbol == symbol))
    }

    /// Add a symbol to the ungrouped section. No-op if the same (symbol, exchange, account_type) triple already present.
    pub fn add_symbol(&mut self, symbol: String, exchange: String, account_type: String) {
        self.add_symbol_with_type(symbol, exchange, account_type);
    }

    /// Add a symbol with an explicit account_type. No-op if the same (symbol, exchange, account_type) triple already present.
    pub fn add_symbol_with_type(&mut self, symbol: String, exchange: String, account_type: String) {
        if !self.contains_with_type(&symbol, &exchange, &account_type) {
            let at = account_type.parse().unwrap_or_default();
            self.ungrouped.push(InstrumentAddress::new(exchange, symbol, at));
        }
    }

    /// Remove a specific (symbol, exchange, account_type) triple from everywhere in this list.
    pub fn remove_symbol(&mut self, symbol: &str, exchange: &str, account_type: &str) {
        let at: crate::instrument::AccountType = account_type.parse().unwrap_or_default();
        self.ungrouped.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == at));
        for g in &mut self.groups {
            g.symbols.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == at));
        }
    }

    /// Move a symbol into a specific group. Removes from previous location first.
    ///
    /// Matches on the full (symbol, exchange, account_type) triple.
    pub fn move_to_group(&mut self, symbol: &str, exchange: &str, account_type: &str, group_id: u64) {
        let at: crate::instrument::AccountType = account_type.parse().unwrap_or_default();
        // Find and remove from current location, keeping the address.
        let mut found: Option<InstrumentAddress> = None;
        if let Some(pos) = self.ungrouped.iter().position(|s| s.symbol == symbol && s.exchange == exchange && s.account_type == at) {
            found = Some(self.ungrouped.remove(pos));
        } else {
            for g in &mut self.groups {
                if let Some(pos) = g.symbols.iter().position(|s| s.symbol == symbol && s.exchange == exchange && s.account_type == at) {
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
        list.add_symbol("BTCUSDT".to_string(), "Binance".to_string(), String::new());
        list.add_symbol("BTCUSDT".to_string(), "OKX".to_string(), String::new());
        assert_eq!(list.ungrouped.len(), 2);
        assert!(list.contains("BTCUSDT", "Binance", ""));
        assert!(list.contains("BTCUSDT", "OKX", ""));
        assert!(!list.contains("BTCUSDT", "KuCoin", ""));
        list.remove_symbol("BTCUSDT", "OKX", "");
        assert!(list.contains("BTCUSDT", "Binance", ""));
        assert!(!list.contains("BTCUSDT", "OKX", ""));
    }

    #[test]
    fn test_add_symbol_no_duplicates() {
        let mut list = WatchlistList { id: 1, ..Default::default() };
        list.add_symbol("AAPL".to_string(), "Binance".to_string(), String::new());
        list.add_symbol("AAPL".to_string(), "Binance".to_string(), String::new());
        assert_eq!(list.ungrouped.iter().filter(|s| s.symbol == "AAPL").count(), 1);
    }

    #[test]
    fn test_group_operations() {
        let mut list = WatchlistList { id: 1, ..Default::default() };
        list.add_symbol("BTC".to_string(), "Binance".to_string(), String::new());
        list.add_group(10, "Crypto".to_string());
        list.move_to_group("BTC", "Binance", "", 10);
        assert!(list.ungrouped.is_empty());
        assert_eq!(
            list.groups[0].symbols,
            vec![InstrumentAddress::new("Binance", "BTC", AccountType::Spot)]
        );
        list.remove_group(10);
        assert!(list.groups.is_empty());
        assert_eq!(list.ungrouped, vec![InstrumentAddress::new("Binance", "BTC", AccountType::Spot)]);
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
    fn test_toggle_does_not_cross_account_types() {
        let mut list = WatchlistList { id: 1, ..Default::default() };
        list.add_symbol_with_type("BTCUSDT".to_string(), "binance".to_string(), "S".to_string());
        list.add_symbol_with_type("BTCUSDT".to_string(), "binance".to_string(), "F".to_string());
        list.remove_symbol("BTCUSDT", "binance", "F");
        assert!(list.contains_with_type("BTCUSDT", "binance", "S"));
        assert!(!list.contains_with_type("BTCUSDT", "binance", "F"));
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
