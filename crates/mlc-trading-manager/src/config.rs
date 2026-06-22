use std::collections::HashMap;
use std::path::Path;
use crate::types::{ExchangeId, AccountType, SizePreset};
use crate::error::TradingResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TradingConfig {
    pub default_order_type: String,
    pub default_tif: String,
    pub size_presets: Vec<SizePreset>,
    pub max_order_notional: Option<f64>,
    pub max_position_notional: Option<f64>,
    pub paper_initial_balances: HashMap<String, f64>,
    pub paper_mode_enabled: HashMap<String, bool>,
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            default_order_type: "Market".into(),
            default_tif: "GTC".into(),
            size_presets: vec![
                SizePreset::BalancePct(0.25),
                SizePreset::BalancePct(0.50),
                SizePreset::BalancePct(0.75),
                SizePreset::BalancePct(1.00),
            ],
            max_order_notional: None,
            max_position_notional: None,
            paper_initial_balances: HashMap::from([("USDT".into(), 10_000.0)]),
            paper_mode_enabled: HashMap::new(),
        }
    }
}

/// Convert a `&Path` to a storage key.
///
/// The path is used verbatim (forward-slash normalised) as the key.
/// `TradingConfig` paths are typically under the app-data directory;
/// the storage backend maps them to the appropriate underlying store.
fn path_to_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl TradingConfig {
    pub fn load(path: &Path) -> TradingResult<Self> {
        let storage = mlc_core::global_storage();
        let key = path_to_key(path);
        match storage.read(&key)? {
            Some(bytes) => {
                let s = String::from_utf8(bytes)
                    .map_err(|e| std::io::Error::other(format!("UTF-8: {e}")))?;
                Ok(serde_json::from_str(&s)?)
            }
            None => Ok(Self::default()),
        }
    }

    pub fn save(&self, path: &Path) -> TradingResult<()> {
        let json = serde_json::to_string_pretty(self)?;
        let storage = mlc_core::global_storage();
        storage.write(&path_to_key(path), json.as_bytes())?;
        Ok(())
    }

    pub fn is_paper(&self, exchange_id: ExchangeId, account_type: AccountType) -> bool {
        let key = format!("{:?}:{:?}", exchange_id, account_type);
        self.paper_mode_enabled.get(&key).copied().unwrap_or(true)
    }
}
