use std::collections::HashMap;
use std::path::Path;
use crate::types::{ExchangeId, AccountType, SizePreset};
use crate::error::TradingResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TradingConfig {
    #[serde(default = "default_order_type")]
    pub default_order_type: String,
    #[serde(default = "default_tif")]
    pub default_tif: String,
    #[serde(default = "default_size_presets")]
    pub size_presets: Vec<SizePreset>,
    #[serde(default)]
    pub max_order_notional: Option<f64>,
    #[serde(default)]
    pub max_position_notional: Option<f64>,
    /// Starting balance per asset for a freshly-created `PaperEngine`
    /// (one copy per `(exchange, account_type)` paper session — see
    /// `TradingManager::on_connector_ready`). `#[serde(default)]` so an
    /// older on-disk `trading_config.json` predating this field (or one
    /// saved with an empty map) still deserializes instead of failing
    /// `TradingConfig::load` outright.
    #[serde(default = "default_paper_initial_balances")]
    pub paper_initial_balances: HashMap<String, f64>,
    #[serde(default)]
    pub paper_mode_enabled: HashMap<String, bool>,
}

fn default_order_type() -> String { "Market".into() }
fn default_tif() -> String { "GTC".into() }
fn default_size_presets() -> Vec<SizePreset> {
    vec![
        SizePreset::BalancePct(0.25),
        SizePreset::BalancePct(0.50),
        SizePreset::BalancePct(0.75),
        SizePreset::BalancePct(1.00),
    ]
}
fn default_paper_initial_balances() -> HashMap<String, f64> {
    HashMap::from([("USDT".into(), 10_000.0)])
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            default_order_type: default_order_type(),
            default_tif: default_tif(),
            size_presets: default_size_presets(),
            max_order_notional: None,
            max_position_notional: None,
            paper_initial_balances: default_paper_initial_balances(),
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
