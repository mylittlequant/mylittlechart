//! Standalone crossing detection utility (stateless).
//!
//! For most use cases, prefer `AlertManager::check_crossings()` which
//! tracks `last_price` internally.

use crate::types::{AlertCondition, AlertStatus, AlertItem};

/// Check a single alert against prev/current price. Returns true if triggered.
pub fn check_crossings(alert: &AlertItem, prev_price: f64, current_price: f64) -> bool {
    if alert.status != AlertStatus::Active {
        return false;
    }
    match alert.condition {
        AlertCondition::CrossingUp => prev_price < alert.price && current_price >= alert.price,
        AlertCondition::CrossingDown => prev_price > alert.price && current_price <= alert.price,
        AlertCondition::Crossing => {
            (prev_price < alert.price && current_price >= alert.price)
                || (prev_price > alert.price && current_price <= alert.price)
        }
        AlertCondition::GreaterThan => current_price > alert.price,
        AlertCondition::LessThan => current_price < alert.price,
        _ => false,
    }
}
