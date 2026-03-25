//! WatchlistManager — manages multiple named watchlists with groups.
//!
//! Provides types for watchlist presets, colored groups, and per-symbol membership.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

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
        }
    }
}

// =============================================================================
// WatchlistSymbol
// =============================================================================

/// A symbol entry with its exchange affiliation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WatchlistSymbol {
    pub symbol: String,
    /// Exchange identifier string (e.g. "binance", "okx").
    /// Defaults to "binance" for migration of old watchlists.
    #[serde(default = "default_exchange")]
    pub exchange: String,
    /// Account type short label (e.g. "FC" for FuturesCross, "M" for Margin).
    ///
    /// Empty string for Spot (the common case).
    /// Uses `String` to avoid a dependency on digdigdig3 types.
    #[serde(default)]
    pub account_type: String,
}

fn default_exchange() -> String {
    "binance".to_string()
}

impl WatchlistSymbol {
    pub fn new(symbol: String, exchange: String) -> Self {
        Self { symbol, exchange, account_type: String::new() }
    }
}

// =============================================================================
// Serde migration helpers
// =============================================================================

/// Deserializes either a plain string list or a WatchlistSymbol list.
/// Old format: ["BTCUSDT", "ADAUSDT"]
/// New format: [{"symbol": "BTCUSDT", "exchange": "binance"}, ...]
fn deserialize_symbols<'de, D>(deserializer: D) -> Result<Vec<WatchlistSymbol>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SymbolOrString {
        Symbol(WatchlistSymbol),
        Plain(String),
    }

    let items: Vec<SymbolOrString> = Vec::deserialize(deserializer)?;
    Ok(items
        .into_iter()
        .map(|item| match item {
            SymbolOrString::Symbol(ws) => ws,
            SymbolOrString::Plain(s) => WatchlistSymbol::new(s, "binance".to_string()),
        })
        .collect())
}

/// Deserializes an optional plain string list or WatchlistSymbol list.
fn deserialize_symbols_opt<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<WatchlistSymbol>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SymbolOrString {
        Symbol(WatchlistSymbol),
        Plain(String),
    }

    let opt: Option<Vec<SymbolOrString>> = Option::deserialize(deserializer)?;
    Ok(opt.map(|items| {
        items
            .into_iter()
            .map(|item| match item {
                SymbolOrString::Symbol(ws) => ws,
                SymbolOrString::Plain(s) => WatchlistSymbol::new(s, "binance".to_string()),
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
    pub symbols: Vec<WatchlistSymbol>,
    pub collapsed: bool,
}

// =============================================================================
// WatchlistList
// =============================================================================

/// A single named watchlist (preset).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WatchlistList {
    pub id: u64,
    pub name: String,
    pub groups: Vec<WatchlistGroup>,
    /// Symbols not assigned to any group.
    #[serde(deserialize_with = "deserialize_symbols")]
    pub ungrouped: Vec<WatchlistSymbol>,
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
    pub order_snapshot: Option<Vec<WatchlistSymbol>>,
}

impl WatchlistList {
    /// Set a color flag for a symbol on a specific exchange.
    ///
    /// Passing an empty `color` removes the flag.
    /// Key format: `"symbol:exchange"` (e.g. `"BTCUSDT:Binance"`).
    pub fn set_color_flag(&mut self, symbol: &str, exchange: &str, color: &str) {
        let key = format!("{}:{}", symbol, exchange);
        if color.is_empty() {
            self.color_flags.remove(&key);
        } else {
            self.color_flags.insert(key, color.to_string());
        }
    }

    /// Get the color flag for a symbol on a specific exchange, or `None` if none is set.
    pub fn get_color_flag(&self, symbol: &str, exchange: &str) -> Option<&str> {
        let key = format!("{}:{}", symbol, exchange);
        self.color_flags.get(&key).map(|s| s.as_str())
    }

    /// All symbols in this list (groups + ungrouped), preserving order.
    pub fn all_symbols(&self) -> Vec<&WatchlistSymbol> {
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

    /// Check if a (symbol, exchange) pair is in this list.
    pub fn contains(&self, symbol: &str, exchange: &str) -> bool {
        self.ungrouped.iter().any(|s| s.symbol == symbol && s.exchange == exchange)
            || self.groups.iter().any(|g| g.symbols.iter().any(|s| s.symbol == symbol && s.exchange == exchange))
    }

    /// Check if a (symbol, exchange, account_type) triple is in this list.
    pub fn contains_with_type(&self, symbol: &str, exchange: &str, account_type: &str) -> bool {
        self.ungrouped.iter().any(|s| s.symbol == symbol && s.exchange == exchange && s.account_type == account_type)
            || self.groups.iter().any(|g| g.symbols.iter().any(|s| s.symbol == symbol && s.exchange == exchange && s.account_type == account_type))
    }

    /// Check if a symbol (any exchange) is in this list.
    pub fn contains_symbol(&self, symbol: &str) -> bool {
        self.ungrouped.iter().any(|s| s.symbol == symbol)
            || self.groups.iter().any(|g| g.symbols.iter().any(|s| s.symbol == symbol))
    }

    /// Add a symbol to the ungrouped section. No-op if the same (symbol, exchange) pair already present.
    pub fn add_symbol(&mut self, symbol: String, exchange: String) {
        if !self.contains(&symbol, &exchange) {
            self.ungrouped.push(WatchlistSymbol::new(symbol, exchange));
        }
    }

    /// Add a symbol with an explicit account_type. No-op if the same (symbol, exchange, account_type) triple already present.
    pub fn add_symbol_with_type(&mut self, symbol: String, exchange: String, account_type: String) {
        if !self.contains_with_type(&symbol, &exchange, &account_type) {
            self.ungrouped.push(WatchlistSymbol { symbol, exchange, account_type });
        }
    }

    /// Remove a specific (symbol, exchange, account_type) triple from everywhere in this list.
    pub fn remove_symbol(&mut self, symbol: &str, exchange: &str, account_type: &str) {
        self.ungrouped.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == account_type));
        for g in &mut self.groups {
            g.symbols.retain(|s| !(s.symbol == symbol && s.exchange == exchange && s.account_type == account_type));
        }
    }

    /// Move a symbol into a specific group. Removes from previous location first.
    pub fn move_to_group(&mut self, symbol: &str, group_id: u64) {
        // Find and remove from current location, keeping the WatchlistSymbol.
        let mut found: Option<WatchlistSymbol> = None;
        if let Some(pos) = self.ungrouped.iter().position(|s| s.symbol == symbol) {
            found = Some(self.ungrouped.remove(pos));
        } else {
            for g in &mut self.groups {
                if let Some(pos) = g.symbols.iter().position(|s| s.symbol == symbol) {
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

impl Default for WatchlistList {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            groups: Vec::new(),
            ungrouped: Vec::new(),
            column_config: WatchlistColumnConfig::default(),
            color_flags: HashMap::new(),
            order_snapshot: None,
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
    pub fn new(default_symbols: Vec<WatchlistSymbol>) -> Self {
        let list = WatchlistList {
            id: 1,
            name: "Watchlist 1".to_string(),
            groups: Vec::new(),
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

    /// Check if a (symbol, exchange) pair is in the active watchlist.
    pub fn contains(&self, symbol: &str, exchange: &str) -> bool {
        self.active_list().map(|l| l.contains(symbol, exchange)).unwrap_or(false)
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
    pub fn add_symbol(&mut self, symbol: String, exchange: String) {
        if let Some(list) = self.active_list_mut() {
            list.add_symbol(symbol, exchange);
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
        let mgr = WatchlistManager::new(vec![WatchlistSymbol::new(
            "BTCUSDT".to_string(),
            "Binance".to_string(),
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
        mgr.add_symbol("BTCUSDT".to_string(), "Binance".to_string());
        mgr.add_symbol("BTCUSDT".to_string(), "OKX".to_string());
        let list = mgr.active_list().unwrap();
        assert_eq!(list.ungrouped.len(), 2);
        assert!(mgr.contains("BTCUSDT", "Binance"));
        assert!(mgr.contains("BTCUSDT", "OKX"));
        assert!(!mgr.contains("BTCUSDT", "KuCoin"));
        // Remove only the OKX one
        mgr.remove_symbol("BTCUSDT", "OKX", "");
        assert!(mgr.contains("BTCUSDT", "Binance"));
        assert!(!mgr.contains("BTCUSDT", "OKX"));
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
        mgr.add_symbol("AAPL".to_string(), "Binance".to_string());
        mgr.add_symbol("AAPL".to_string(), "Binance".to_string());
        let list = mgr.active_list().unwrap();
        assert_eq!(
            list.ungrouped.iter().filter(|s| s.symbol == "AAPL").count(),
            1
        );
    }

    #[test]
    fn test_group_operations() {
        let mut mgr = WatchlistManager::default();
        mgr.add_symbol("BTC".to_string(), "Binance".to_string());
        {
            let list = mgr.active_list_mut().unwrap();
            list.add_group(10, "Crypto".to_string(), "#f59e0b".to_string());
            list.move_to_group("BTC", 10);
            assert!(list.ungrouped.is_empty());
            assert_eq!(
                list.groups[0].symbols,
                vec![WatchlistSymbol::new("BTC".to_string(), "Binance".to_string())]
            );
            list.remove_group(10);
            assert!(list.groups.is_empty());
            assert_eq!(
                list.ungrouped,
                vec![WatchlistSymbol::new("BTC".to_string(), "Binance".to_string())]
            );
        }
    }

    #[test]
    fn test_all_symbols_order() {
        let mut list = WatchlistList {
            id: 1,
            name: "Test".to_string(),
            groups: Vec::new(),
            ungrouped: vec![WatchlistSymbol::new("AAPL".to_string(), "Binance".to_string())],
            column_config: WatchlistColumnConfig::default(),
            color_flags: HashMap::new(),
            order_snapshot: None,
        };
        list.add_group(1, "Grp".to_string(), "#fff".to_string());
        list.groups[0]
            .symbols
            .push(WatchlistSymbol::new("BTC".to_string(), "Binance".to_string()));
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
