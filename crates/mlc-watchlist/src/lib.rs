//! WatchlistManager — manages multiple named watchlists with groups.
//!
//! Provides types for watchlist presets, colored groups, and per-symbol membership.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use mlc_core::instrument::InstrumentAddress;

// =============================================================================
// WatchlistColumnConfig
// =============================================================================

/// Configuration for which columns to show in watchlist sidebar.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistColumnConfig {
    pub show_exchange: bool,
    pub show_last_price: bool,
    pub show_change_pct: bool,
    pub show_change_abs: bool,
    pub show_volume: bool,
    pub show_high_low: bool,
    /// Show account type column (e.g. "S", "FC", "M"). Default false.
    #[serde(default)]
    pub show_account_type: bool,

    /// Custom separator X positions as offsets from the left edge of the usable area.
    ///
    /// Index 0 = separator between column 0 and column 1, etc.
    /// There is always one fewer separator than there are visible columns.
    ///
    /// When `None` (the default and after any column toggle), separators are
    /// placed at the default column boundaries computed by the renderer.
    /// Reset to `None` whenever a column is toggled on or off so the layout
    /// starts fresh.
    ///
    /// Dragging a separator left clips the column to its left; dragging right
    /// clips the column to its right.  Column content positions are always at
    /// the default layout positions and are never moved — only clipped.
    #[serde(default)]
    pub separator_offsets: Option<Vec<f64>>,

    /// When `true` (the default), all visible columns share equal width and
    /// separator drag is disabled.  Setting to `false` lets the user drag
    /// column separators to custom widths.
    #[serde(default = "default_true")]
    pub align_columns: bool,
}

fn default_true() -> bool {
    true
}

impl Default for WatchlistColumnConfig {
    fn default() -> Self {
        Self {
            show_exchange: true,
            show_last_price: true,
            show_change_pct: true,
            show_change_abs: false,
            show_volume: false,
            show_high_low: false,
            show_account_type: false,
            separator_offsets: None,
            align_columns: true,
        }
    }
}

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
    InstrumentAddress::new(default_exchange(), symbol, mlc_core::instrument::AccountType::default())
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

/// Deserializes an optional plain string list or `InstrumentAddress` list.
fn deserialize_symbols_opt<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<InstrumentAddress>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SymbolOrString {
        Symbol(InstrumentAddress),
        Plain(String),
    }

    let opt: Option<Vec<SymbolOrString>> = Option::deserialize(deserializer)?;
    Ok(opt.map(|items| {
        items
            .into_iter()
            .map(|item| match item {
                SymbolOrString::Symbol(addr) => addr,
                SymbolOrString::Plain(s) => address_from_plain_symbol(s),
            })
            .collect()
    }))
}

// =============================================================================
// WatchlistGroup
// =============================================================================

/// A colored group/section within a watchlist.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistGroup {
    pub id: u64,
    pub name: String,
    pub color: String,
    #[serde(deserialize_with = "deserialize_symbols")]
    pub symbols: Vec<InstrumentAddress>,
    pub collapsed: bool,
}

// =============================================================================
// WatchlistList
// =============================================================================

/// A single named watchlist (preset).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[derive(Default)]
pub struct WatchlistList {
    pub id: u64,
    pub name: String,
    pub groups: Vec<WatchlistGroup>,
    /// Symbols not assigned to any group.
    #[serde(deserialize_with = "deserialize_symbols")]
    pub ungrouped: Vec<InstrumentAddress>,
    pub column_config: WatchlistColumnConfig,
    /// Per-symbol color flag. Key = symbol name, value = CSS hex color.
    ///
    /// An absent entry (or empty string value) means no flag is set.
    #[serde(default)]
    pub color_flags: HashMap<String, String>,
    /// Snapshot of the original symbol order (before any sort).
    ///
    /// When `None`, `ungrouped` IS the original order.
    /// Set when the user first activates a sort mode; cleared when the user
    /// resets to mode 0 or manually reorders symbols via drag.
    #[serde(default, deserialize_with = "deserialize_symbols_opt")]
    pub order_snapshot: Option<Vec<InstrumentAddress>>,
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
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
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
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        self.ungrouped.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == at));
        for g in &mut self.groups {
            g.symbols.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == at));
        }
    }

    /// Move a symbol into a specific group. Removes from previous location first.
    ///
    /// Matches on the full (symbol, exchange, account_type) triple.
    pub fn move_to_group(&mut self, symbol: &str, exchange: &str, account_type: &str, group_id: u64) {
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
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

    /// Add a new group.
    pub fn add_group(&mut self, id: u64, name: String, color: String) {
        self.groups.push(WatchlistGroup {
            id,
            name,
            color,
            symbols: Vec::new(),
            collapsed: false,
        });
    }

    /// Remove a group by id. Its symbols move to ungrouped.
    pub fn remove_group(&mut self, group_id: u64) {
        if let Some(idx) = self.groups.iter().position(|g| g.id == group_id) {
            let group = self.groups.remove(idx);
            self.ungrouped.extend(group.symbols);
        }
    }
}


// =============================================================================
// WatchlistManager
// =============================================================================

/// Manages multiple watchlists.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistManager {
    pub lists: Vec<WatchlistList>,
    pub active_list_id: u64,
    next_id: u64,
}

impl WatchlistManager {
    /// Create with a single default list containing the given symbols.
    pub fn new(default_symbols: Vec<InstrumentAddress>) -> Self {
        let list = WatchlistList {
            id: 1,
            name: "Watchlist 1".to_string(), groups: Vec::new(),
            ungrouped: default_symbols,
            column_config: WatchlistColumnConfig::default(),
            color_flags: HashMap::new(),
            order_snapshot: None,
        };
        Self {
            lists: vec![list],
            active_list_id: 1,
            next_id: 2,
        }
    }

    /// Get the active watchlist.
    pub fn active_list(&self) -> Option<&WatchlistList> {
        self.lists.iter().find(|l| l.id == self.active_list_id)
    }

    /// Get the active watchlist mutably.
    pub fn active_list_mut(&mut self) -> Option<&mut WatchlistList> {
        self.lists.iter_mut().find(|l| l.id == self.active_list_id)
    }

    /// Check if a (symbol, exchange, account_type) triple is in the active watchlist.
    pub fn contains(&self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.active_list().map(|l| l.contains(symbol, exchange, account_type)).unwrap_or(false)
    }

    /// Check if a (symbol, exchange, account_type) triple is in the active watchlist.
    pub fn contains_with_type(&self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.active_list().map(|l| l.contains_with_type(symbol, exchange, account_type)).unwrap_or(false)
    }

    /// Check if a symbol (any exchange) is in the active watchlist.
    pub fn contains_symbol(&self, symbol: &str) -> bool {
        self.active_list().map(|l| l.contains_symbol(symbol)).unwrap_or(false)
    }

    /// Add symbol to active watchlist.
    pub fn add_symbol(&mut self, symbol: String, exchange: String, account_type: String) {
        if let Some(list) = self.active_list_mut() {
            list.add_symbol(symbol, exchange, account_type);
        }
    }

    /// Add symbol with explicit account_type to active watchlist.
    pub fn add_symbol_with_type(&mut self, symbol: String, exchange: String, account_type: String) {
        if let Some(list) = self.active_list_mut() {
            list.add_symbol_with_type(symbol, exchange, account_type);
        }
    }

    /// Remove a specific (symbol, exchange, account_type) triple from active watchlist.
    pub fn remove_symbol(&mut self, symbol: &str, exchange: &str, account_type: &str) {
        if let Some(list) = self.active_list_mut() {
            list.remove_symbol(symbol, exchange, account_type);
        }
    }

    /// Toggle symbol: add if missing, remove if present. Returns new state (`true` = now in list).
    pub fn toggle_symbol(&mut self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        if self.contains_with_type(symbol, exchange, account_type) {
            self.remove_symbol(symbol, exchange, account_type);
            false
        } else {
            self.add_symbol_with_type(symbol.to_string(), exchange.to_string(), account_type.to_string());
            true
        }
    }

    /// Create a new empty watchlist and return its id.
    pub fn create_list(&mut self, name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.lists.push(WatchlistList {
            id,
            name,
            groups: Vec::new(),
            ungrouped: Vec::new(),
            column_config: WatchlistColumnConfig::default(),
            color_flags: HashMap::new(),
            order_snapshot: None,
        });
        id
    }

    /// Delete a watchlist by id. Cannot delete the last one. Returns `true` on success.
    pub fn delete_list(&mut self, id: u64) -> bool {
        if self.lists.len() <= 1 {
            return false;
        }
        if let Some(idx) = self.lists.iter().position(|l| l.id == id) {
            self.lists.remove(idx);
            if self.active_list_id == id {
                self.active_list_id = self.lists[0].id;
            }
            true
        } else {
            false
        }
    }

    /// Generate next unique ID (for groups etc).
    pub fn next_unique_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Reorder a symbol in the active list's ungrouped section.
    ///
    /// Moves the symbol at position `from_idx` to position `to_idx`.
    /// Both indices are clamped to the valid range; no-op if `from_idx == to_idx`
    /// or if the active list has no ungrouped symbols at those positions.
    pub fn reorder_symbol(&mut self, from_idx: usize, to_idx: usize) {
        if from_idx == to_idx {
            return;
        }
        if let Some(list) = self.active_list_mut() {
            let len = list.ungrouped.len();
            if from_idx >= len || to_idx >= len {
                return;
            }
            let symbol = list.ungrouped.remove(from_idx);
            // Adjust target index after removal.
            let insert_at = if to_idx > from_idx {
                to_idx.min(list.ungrouped.len())
            } else {
                to_idx
            };
            list.ungrouped.insert(insert_at, symbol);
        }
    }
}

impl Default for WatchlistManager {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_manager_has_one_list() {
        let mgr = WatchlistManager::new(vec![InstrumentAddress::new(
            "Binance", "BTCUSDT", mlc_core::instrument::AccountType::Spot,
        )]);
        assert_eq!(mgr.lists.len(), 1);
        assert_eq!(mgr.active_list_id, 1);
    }

    #[test]
    fn test_toggle_symbol_add_remove() {
        let mut mgr = WatchlistManager::default();
        assert!(!mgr.contains_with_type("ETHUSDT", "Binance", ""));
        let added = mgr.toggle_symbol("ETHUSDT", "Binance", "");
        assert!(added);
        assert!(mgr.contains_with_type("ETHUSDT", "Binance", ""));
        let removed = mgr.toggle_symbol("ETHUSDT", "Binance", "");
        assert!(!removed);
        assert!(!mgr.contains_with_type("ETHUSDT", "Binance", ""));
    }

    #[test]
    fn test_same_symbol_different_exchanges() {
        let mut mgr = WatchlistManager::default();
        mgr.add_symbol("BTCUSDT".to_string(), "Binance".to_string(), String::new());
        mgr.add_symbol("BTCUSDT".to_string(), "OKX".to_string(), String::new());
        let list = mgr.active_list().unwrap();
        assert_eq!(list.ungrouped.len(), 2);
        assert!(mgr.contains("BTCUSDT", "Binance", ""));
        assert!(mgr.contains("BTCUSDT", "OKX", ""));
        assert!(!mgr.contains("BTCUSDT", "KuCoin", ""));
        // Remove only the OKX one
        mgr.remove_symbol("BTCUSDT", "OKX", "");
        assert!(mgr.contains("BTCUSDT", "Binance", ""));
        assert!(!mgr.contains("BTCUSDT", "OKX", ""));
    }

    #[test]
    fn test_create_and_delete_list() {
        let mut mgr = WatchlistManager::default();
        let id = mgr.create_list("My List".to_string());
        assert_eq!(mgr.lists.len(), 2);
        let deleted = mgr.delete_list(id);
        assert!(deleted);
        assert_eq!(mgr.lists.len(), 1);
    }

    #[test]
    fn test_cannot_delete_last_list() {
        let mut mgr = WatchlistManager::default();
        let deleted = mgr.delete_list(1);
        assert!(!deleted);
        assert_eq!(mgr.lists.len(), 1);
    }

    #[test]
    fn test_add_symbol_no_duplicates() {
        let mut mgr = WatchlistManager::default();
        mgr.add_symbol("AAPL".to_string(), "Binance".to_string(), String::new());
        mgr.add_symbol("AAPL".to_string(), "Binance".to_string(), String::new());
        let list = mgr.active_list().unwrap();
        assert_eq!(
            list.ungrouped.iter().filter(|s| s.symbol == "AAPL").count(),
            1
        );
    }

    #[test]
    fn test_group_operations() {
        let mut mgr = WatchlistManager::default();
        mgr.add_symbol("BTC".to_string(), "Binance".to_string(), String::new());
        {
            let list = mgr.active_list_mut().unwrap();
            list.add_group(10, "Crypto".to_string(), "#f59e0b".to_string());
            list.move_to_group("BTC", "Binance", "", 10);
            assert!(list.ungrouped.is_empty());
            assert_eq!(
                list.groups[0].symbols,
                vec![InstrumentAddress::new("Binance", "BTC", mlc_core::instrument::AccountType::Spot)]
            );
            list.remove_group(10);
            assert!(list.groups.is_empty());
            assert_eq!(
                list.ungrouped,
                vec![InstrumentAddress::new("Binance", "BTC", mlc_core::instrument::AccountType::Spot)]
            );
        }
    }

    #[test]
    fn test_all_symbols_order() {
        let mut list = WatchlistList {
            id: 1,
            name: "Test".to_string(),
            groups: Vec::new(),
            ungrouped: vec![InstrumentAddress::new("Binance", "AAPL", mlc_core::instrument::AccountType::Spot)],
            column_config: WatchlistColumnConfig::default(),
            color_flags: HashMap::new(),
            order_snapshot: None,
        };
        list.add_group(1, "Grp".to_string(), "#fff".to_string());
        list.groups[0]
            .symbols
            .push(InstrumentAddress::new("Binance", "BTC", mlc_core::instrument::AccountType::Spot));
        let syms = list.all_symbols();
        assert_eq!(syms[0].symbol, "BTC");
        assert_eq!(syms[1].symbol, "AAPL");
    }

    #[test]
    fn test_toggle_does_not_cross_account_types() {
        // Regression: toggling BTCUSDT:binance:F must not remove BTCUSDT:binance:S.
        let mut mgr = WatchlistManager::default();
        mgr.add_symbol_with_type("BTCUSDT".to_string(), "binance".to_string(), "S".to_string());
        mgr.add_symbol_with_type("BTCUSDT".to_string(), "binance".to_string(), "F".to_string());
        // Toggle off the futures entry.
        let removed = mgr.toggle_symbol("BTCUSDT", "binance", "F");
        assert!(!removed);
        // Spot entry must still be present.
        assert!(mgr.contains_with_type("BTCUSDT", "binance", "S"));
        assert!(!mgr.contains_with_type("BTCUSDT", "binance", "F"));
    }

    #[test]
    fn test_remove_does_not_cross_account_types() {
        // Regression: removing BTCUSDT:binance:F must not touch BTCUSDT:binance:S.
        let mut mgr = WatchlistManager::default();
        mgr.add_symbol_with_type("BTCUSDT".to_string(), "binance".to_string(), "S".to_string());
        mgr.add_symbol_with_type("BTCUSDT".to_string(), "binance".to_string(), "F".to_string());
        mgr.remove_symbol("BTCUSDT", "binance", "F");
        assert!(mgr.contains_with_type("BTCUSDT", "binance", "S"));
        assert!(!mgr.contains_with_type("BTCUSDT", "binance", "F"));
    }

    #[test]
    fn test_deserialize_migration_plain_strings() {
        let json = r#"{
            "id": 1,
            "name": "Legacy",
            "groups": [],
            "ungrouped": ["BTCUSDT", "ETHUSDT"],
            "column_config": {
                "show_exchange": true,
                "show_last_price": true,
                "show_change_pct": true,
                "show_change_abs": false,
                "show_volume": false,
                "show_high_low": false
            },
            "color_flags": {}
        }"#;
        let list: WatchlistList = serde_json::from_str(json).unwrap();
        assert_eq!(list.ungrouped.len(), 2);
        assert_eq!(list.ungrouped[0].symbol, "BTCUSDT");
        assert_eq!(list.ungrouped[0].exchange, "Binance");
        assert_eq!(list.ungrouped[1].symbol, "ETHUSDT");
        assert_eq!(list.ungrouped[1].exchange, "Binance");
    }

    #[test]
    fn test_deserialize_new_format_with_exchange() {
        let json = r#"{
            "id": 1,
            "name": "New",
            "groups": [],
            "ungrouped": [{"symbol": "BTCUSDT", "exchange": "OKX"}],
            "column_config": {
                "show_exchange": true,
                "show_last_price": true,
                "show_change_pct": true,
                "show_change_abs": false,
                "show_volume": false,
                "show_high_low": false
            },
            "color_flags": {}
        }"#;
        let list: WatchlistList = serde_json::from_str(json).unwrap();
        assert_eq!(list.ungrouped[0].symbol, "BTCUSDT");
        assert_eq!(list.ungrouped[0].exchange, "OKX");
    }
}

// =============================================================================
// Tests — JSON persistence compatibility gate (WatchlistSymbol -> InstrumentAddress)
// =============================================================================

#[cfg(test)]
mod json_compat_tests {
    use super::*;

    /// Literal `watchlists.json` fragment produced by the pre-cutover
    /// `WatchlistSymbol` entry (flat `symbol` / `exchange` / `account_type:
    /// String` fields) — captured from the struct definition prior to this
    /// commit. Doctrine gate: this MUST still deserialize cleanly into
    /// `InstrumentAddress` entries and re-serialize to the same flat shape.
    const LEGACY_ENTRY_JSON: &str = r#"{"symbol": "BTCUSDT", "exchange": "Binance", "account_type": "FI"}"#;

    #[test]
    fn legacy_entry_json_deserializes_into_instrument_address() {
        let addr: InstrumentAddress = serde_json::from_str(LEGACY_ENTRY_JSON)
            .expect("legacy flat symbol/exchange/account_type entry must still deserialize");
        assert_eq!(addr.symbol, "BTCUSDT");
        assert_eq!(addr.exchange, "Binance");
        assert_eq!(addr.account_type, mlc_core::instrument::AccountType::FuturesIsolated);
    }

    #[test]
    fn entry_roundtrip_preserves_flat_field_shape() {
        let addr: InstrumentAddress = serde_json::from_str(LEGACY_ENTRY_JSON).expect("must parse");
        let reserialized = serde_json::to_string(&addr).expect("must re-serialize");
        let value: serde_json::Value = serde_json::from_str(&reserialized).expect("must be valid JSON");
        assert_eq!(value.get("symbol").and_then(|v| v.as_str()), Some("BTCUSDT"));
        assert_eq!(value.get("exchange").and_then(|v| v.as_str()), Some("Binance"));
        assert_eq!(value.get("account_type").and_then(|v| v.as_str()), Some("FI"));
    }

    #[test]
    fn full_list_with_legacy_entries_roundtrips() {
        let json = r##"{
            "id": 1,
            "name": "Legacy",
            "groups": [],
            "ungrouped": [
                {"symbol": "BTCUSDT", "exchange": "Binance", "account_type": "FI"},
                {"symbol": "ETHUSDT", "exchange": "Binance", "account_type": "S"}
            ],
            "column_config": {
                "show_exchange": true,
                "show_last_price": true,
                "show_change_pct": true,
                "show_change_abs": false,
                "show_volume": false,
                "show_high_low": false
            },
            "color_flags": {"BTCUSDT:Binance:FI": "#ff0000"}
        }"##;
        let list: WatchlistList = serde_json::from_str(json).expect("legacy list JSON must still deserialize");
        assert_eq!(list.ungrouped.len(), 2);
        assert_eq!(list.ungrouped[0], InstrumentAddress::new("Binance", "BTCUSDT", mlc_core::instrument::AccountType::FuturesIsolated));
        assert_eq!(list.ungrouped[1], InstrumentAddress::new("Binance", "ETHUSDT", mlc_core::instrument::AccountType::Spot));
        assert_eq!(list.get_color_flag("BTCUSDT", "Binance", "FI"), Some("#ff0000"));
    }
}
