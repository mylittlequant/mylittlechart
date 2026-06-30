//! `AlertSpec` / `AlertPatch` — value types for alert create/update intents.
//!
//! `AlertSpec` replaces the ad-hoc tuple of fields the create flow used to
//! destructure by hand (`settings.rs` save branch, daemon `CreateAlertIn`).
//! `AlertPatch` replaces the daemon's field-by-field `if let` block and the
//! settings.rs edit branch's `update_full` call — both now build one
//! `AlertPatch` and hand it to `AlertItem::apply_patch` /
//! `AlertManager::update`.

use super::types::{AlertCondition, AlertSource, AlertTransport, AlertTriggerMode};
use super::types::AlertItem;

/// Create-time fields for a new alert. Passed to `AlertManager::add`
/// (via `AlertItem::from_spec`).
#[derive(Clone, Debug)]
pub struct AlertSpec {
    pub source: AlertSource,
    pub name: String,
    pub price: f64,
    pub condition: AlertCondition,
    pub price2: f64,
    pub percentage: f64,
    pub trigger_mode: AlertTriggerMode,
    pub transports: Vec<AlertTransport>,
    pub exchange: String,
    pub timeframe: String,
    pub window_id_hint: Option<u64>,
    pub group_id: Option<u64>,
    /// Symbol for non-`Price` sources (Drawing / Indicator / Signal alerts
    /// store the symbol on `AlertItem.symbol`, not inside `AlertSource`).
    /// Ignored when `source` is `AlertSource::Price` — the symbol there
    /// lives inside the source itself, so `from_spec` leaves
    /// `AlertItem.symbol` blank for that case (matches the legacy
    /// "skip the symbol field" check).
    pub symbol: String,
}

impl AlertSpec {
    /// Minimal spec for a price alert with every optional field zeroed.
    /// Used by the test harness, where only `source` / `name` / `price` /
    /// `condition` matter.
    pub fn price_default() -> Self {
        Self {
            source: AlertSource::Price { symbol: String::new() },
            name: String::new(),
            price: 0.0,
            condition: AlertCondition::Crossing,
            price2: 0.0,
            percentage: 0.0,
            trigger_mode: AlertTriggerMode::OneShot,
            transports: AlertTransport::all_default(),
            exchange: String::new(),
            timeframe: String::new(),
            window_id_hint: None,
            group_id: None,
            symbol: String::new(),
        }
    }
}

/// Partial update for an existing alert. `Some(v)` overwrites the field,
/// `None` leaves it unchanged.
#[derive(Clone, Debug, Default)]
pub struct AlertPatch {
    pub source: Option<AlertSource>,
    pub name: Option<String>,
    pub price: Option<f64>,
    pub price2: Option<f64>,
    pub percentage: Option<f64>,
    pub condition: Option<AlertCondition>,
    pub trigger_mode: Option<AlertTriggerMode>,
    pub transports: Option<Vec<AlertTransport>>,
}

impl AlertPatch {
    /// Apply this patch onto `item` in place. `Some(v)` overwrites,
    /// `None` keeps the existing value.
    pub fn apply(&self, item: &mut AlertItem) {
        if let Some(ref source) = self.source {
            item.source = source.clone();
        }
        if let Some(ref name) = self.name {
            item.name = name.clone();
        }
        if let Some(price) = self.price {
            item.price = price;
        }
        if let Some(price2) = self.price2 {
            item.price2 = price2;
        }
        if let Some(percentage) = self.percentage {
            item.percentage = percentage;
        }
        if let Some(condition) = self.condition {
            item.condition = condition;
        }
        if let Some(trigger_mode) = self.trigger_mode {
            item.trigger_mode = trigger_mode;
        }
        if let Some(ref transports) = self.transports {
            item.transports = transports.clone();
        }
    }
}
