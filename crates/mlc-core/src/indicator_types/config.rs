use serde::{Deserialize, Serialize};

/// Configuration for an indicator that belongs to a data bubble.
///
/// Bound to the *bubble* (its windows), NOT to an instrument. Each peer window
/// recalculates the indicator against whatever bars its current symbol
/// produces — switching symbol/exchange/account_type does not migrate the
/// indicator.
///
/// Legacy `symbol` / `exchange` / `account_type` fields are kept on disk via
/// `#[serde(default)]` for backwards-compatible profile loading, but they are
/// never read by runtime code.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndicatorGroupConfig {
    pub id: u64,
    pub type_id: String,
    pub name: String,
    /// Simplified string parameters — typed params can be added later.
    pub params: std::collections::HashMap<String, String>,
    pub pane: u32,
    pub visible: bool,
    /// Legacy: symbol this indicator was created for. Unused at runtime —
    /// indicators are window-scoped, not symbol-scoped.
    #[serde(default, skip_serializing)]
    pub symbol: String,
    /// Legacy: exchange. Unused at runtime.
    #[serde(default, skip_serializing)]
    pub exchange: String,
    /// Legacy: account type. Unused at runtime.
    #[serde(default, skip_serializing)]
    pub account_type: String,
}
