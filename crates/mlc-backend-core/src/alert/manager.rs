//! AlertManager — stateless namespace for alert operations on a DataBubble.
//!
//! All mutable methods take `b: &mut DataBubble` as their first argument.
//! All read-only methods take `b: &DataBubble`.
//! No fields — alert state lives in `DataBubble.alerts` / `alert_next_id` / `alert_last_price`.

use mlc_core::alert::{AlertCondition, AlertItem, AlertSource, AlertStatus, AlertTriggerMode, DrawingExtendMode, SignalBarState, SignalDirection, AlertTransport};

use crate::bubble::DataBubble;

/// Stateless alert namespace.  All operations read/write `DataBubble` fields.
#[derive(Clone, Debug, Default)]
pub struct AlertManager;

impl AlertManager {
    pub fn new() -> Self {
        Self
    }

    // =========================================================================
    // Read
    // =========================================================================

    /// All alert items (for sidebar display, preset snapshot, etc.).
    pub fn items(b: &DataBubble) -> &[AlertItem] {
        &b.alerts
    }

    /// Find an alert by ID.
    pub fn get(b: &DataBubble, id: u64) -> Option<&AlertItem> {
        b.alerts.iter().find(|a| a.id == id)
    }

    /// Find a mutable alert by ID.
    pub fn get_mut(b: &mut DataBubble, id: u64) -> Option<&mut AlertItem> {
        b.alerts.iter_mut().find(|a| a.id == id)
    }

    /// Number of alerts.
    pub fn len(b: &DataBubble) -> usize {
        b.alerts.len()
    }

    pub fn is_empty(b: &DataBubble) -> bool {
        b.alerts.is_empty()
    }

    // =========================================================================
    // Create
    // =========================================================================

    /// Create a new alert from an `AlertSource` and return its ID.
    pub fn create(
        b: &mut DataBubble,
        source: AlertSource,
        name: &str,
        price: f64,
        condition: AlertCondition,
    ) -> u64 {
        let id = b.alert_next_id;
        b.alert_next_id += 1;
        let alert = AlertItem::new(id, source, name, price, condition, AlertStatus::Active);
        b.alerts.push(alert);
        id
    }

    /// Convenience wrapper: create a horizontal price alert for a symbol.
    pub fn create_price_alert(
        b: &mut DataBubble,
        symbol: &str,
        name: &str,
        price: f64,
        condition: AlertCondition,
    ) -> u64 {
        Self::create(
            b,
            AlertSource::Price {
                symbol: symbol.to_string(),
            },
            name,
            price,
            condition,
        )
    }

    /// Insert a single alert item verbatim (for undo/redo restore).
    pub fn restore_one(b: &mut DataBubble, item: AlertItem) {
        if item.id >= b.alert_next_id {
            b.alert_next_id = item.id + 1;
        }
        b.alerts.push(item);
    }

    // =========================================================================
    // Update
    // =========================================================================

    /// Update an existing alert's core fields. Returns true if found.
    pub fn update(
        b: &mut DataBubble,
        id: u64,
        name: &str,
        price: f64,
        condition: AlertCondition,
    ) -> bool {
        if let Some(alert) = Self::get_mut(b, id) {
            alert.name = name.to_string();
            alert.price = price;
            alert.condition = condition;
            true
        } else {
            false
        }
    }

    /// Update all fields of an existing alert. Returns true if found.
    pub fn update_full(
        b: &mut DataBubble,
        id: u64,
        source: AlertSource,
        name: &str,
        price: f64,
        price2: f64,
        percentage: f64,
        condition: AlertCondition,
        trigger_mode: AlertTriggerMode,
        transports: Vec<AlertTransport>,
    ) -> bool {
        if let Some(alert) = Self::get_mut(b, id) {
            alert.source = source;
            alert.name = name.to_string();
            alert.price = price;
            alert.price2 = price2;
            alert.percentage = percentage;
            alert.condition = condition;
            alert.trigger_mode = trigger_mode;
            alert.transports = transports;
            true
        } else {
            false
        }
    }

    // =========================================================================
    // Delete
    // =========================================================================

    /// Remove an alert by ID. Returns true if found and removed.
    pub fn remove(b: &mut DataBubble, id: u64) -> bool {
        let before = b.alerts.len();
        b.alerts.retain(|a| a.id != id);
        b.alerts.len() != before
    }

    /// Remove all alerts.
    pub fn clear(b: &mut DataBubble) {
        b.alerts.clear();
        b.alert_next_id = 1;
        b.alert_last_price.clear();
    }

    /// Snapshot all alerts attached to a specific drawing primitive (for undo).
    pub fn alerts_for_drawing(b: &DataBubble, primitive_id: u64) -> Vec<AlertItem> {
        b.alerts
            .iter()
            .filter(|a| matches!(&a.source, AlertSource::Drawing { primitive_id: pid, .. } if *pid == primitive_id))
            .cloned()
            .collect()
    }

    /// Remove all alerts attached to a specific drawing primitive.
    pub fn remove_alerts_for_drawing(b: &mut DataBubble, primitive_id: u64) {
        b.alerts.retain(|a| {
            !matches!(&a.source, AlertSource::Drawing { primitive_id: pid, .. } if *pid == primitive_id)
        });
    }

    /// Remove all alerts attached to a specific indicator instance.
    pub fn remove_alerts_for_indicator(b: &mut DataBubble, indicator_id: u64) {
        b.alerts.retain(|a| {
            match &a.source {
                AlertSource::Indicator { indicator_id: iid, .. } if *iid == indicator_id => false,
                AlertSource::Signal { indicator_id: iid, .. } if *iid == indicator_id => false,
                _ => true,
            }
        });
    }

    // =========================================================================
    // Dynamic crossing detection (Drawing / Indicator alerts)
    // =========================================================================

    /// Resolve the effective price level for an alert at the given bar.
    ///
    /// Static method — no bubble reference needed, operates on a single alert.
    pub fn resolve_price_static(
        alert: &AlertItem,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Option<f64> {
        match &alert.source {
            AlertSource::Price { .. } => Some(alert.price),
            AlertSource::Drawing { primitive_id, .. } => {
                let (pts, extend_mode) = drawing_points
                    .iter()
                    .find(|(id, _, _)| *id == *primitive_id)
                    .map(|(_, pts, em)| (pts, *em))?;
                if pts.len() < 2 {
                    return pts.first().map(|(_, p)| *p);
                }
                let (bar1, price1) = pts[0];
                let (bar2, price2) = pts[1];
                if (bar2 - bar1).abs() < 1e-6 {
                    return Some(price1);
                }

                let min_bar = bar1.min(bar2);
                let max_bar = bar1.max(bar2);

                match extend_mode {
                    DrawingExtendMode::None => {
                        if current_bar < min_bar || current_bar > max_bar {
                            return None;
                        }
                    }
                    DrawingExtendMode::Right => {
                        if current_bar < min_bar {
                            return None;
                        }
                    }
                    DrawingExtendMode::Left => {
                        if current_bar > max_bar {
                            return None;
                        }
                    }
                    DrawingExtendMode::Both => {}
                }

                let t = (current_bar - bar1) / (bar2 - bar1);
                Some(price1 + t * (price2 - price1))
            }
            AlertSource::Indicator {
                indicator_id,
                output_index,
                ..
            } => {
                let vals = indicator_values
                    .iter()
                    .find(|(id, idx, _)| *id == *indicator_id && *idx == *output_index)
                    .map(|(_, _, v)| v)?;
                vals.last().copied()
            }
            AlertSource::CrossingPair { .. } => None,
            AlertSource::Signal { .. } => None,
        }
    }

    /// Check all active alerts using dynamic price resolution.
    ///
    /// Symbol/exchange/account_type are read from `b` directly — alerts are
    /// now owned by the bubble they target, so `matches_window` is always true
    /// within a given bubble.
    ///
    /// Returns list of triggered alert IDs.
    pub fn check_crossings_dynamic(
        b: &mut DataBubble,
        current_price: f64,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Vec<u64> {
        let symbol = b.symbol.clone();
        let exchange = b.exchange.clone();
        let account_type = b.account_type.clone();

        let tick_key = if exchange.is_empty() {
            symbol.clone()
        } else {
            format!("{}:{}:{}", exchange, symbol, account_type)
        };

        let prev_price = b.alert_last_price.get(&tick_key).copied().unwrap_or(0.0);

        let mut triggered_ids = Vec::new();

        for alert in &mut b.alerts {
            if alert.status != AlertStatus::Active {
                continue;
            }

            let level = match Self::resolve_price_static(
                alert,
                current_bar,
                drawing_points,
                indicator_values,
            ) {
                Some(l) => l,
                None => continue,
            };

            let prev_level = alert.prev_dynamic_price;

            let triggered = match alert.condition {
                AlertCondition::CrossingUp => {
                    prev_price != 0.0 && prev_level != 0.0
                        && prev_price < prev_level
                        && current_price >= level
                }
                AlertCondition::CrossingDown => {
                    prev_price != 0.0 && prev_level != 0.0
                        && prev_price > prev_level
                        && current_price <= level
                }
                AlertCondition::Crossing => {
                    prev_price != 0.0
                        && prev_level != 0.0
                        && ((prev_price < prev_level && current_price >= level)
                            || (prev_price > prev_level && current_price <= level))
                }
                AlertCondition::GreaterThan => current_price > level,
                AlertCondition::LessThan => current_price < level,
                _ => false,
            };

            alert.prev_dynamic_price = level;

            if triggered {
                alert.trigger_count += 1;
                alert.last_triggered = Some("just now".to_string());
                triggered_ids.push(alert.id);

                match alert.trigger_mode {
                    AlertTriggerMode::OneShot => {
                        alert.status = AlertStatus::Triggered;
                    }
                    AlertTriggerMode::TimesN(n) if alert.trigger_count >= n => {
                        alert.status = AlertStatus::Triggered;
                    }
                    _ => {}
                }
            }
        }

        b.alert_last_price.insert(tick_key, current_price);
        triggered_ids
    }

    // =========================================================================
    // Signal alert checker
    // =========================================================================

    /// Check all active `AlertSource::Signal` alerts against a batch of newly-emitted signals.
    ///
    /// Returns the list of triggered alert IDs.
    pub fn check_signal_alerts(
        b: &mut DataBubble,
        new_signals: &[(u64, usize, i8, u8, String)],
    ) -> Vec<u64> {
        let mut triggered_ids = Vec::new();

        for alert in &mut b.alerts {
            if alert.status != AlertStatus::Active {
                continue;
            }

            let (indicator_id, direction_filter, bar_state, kind_filter) =
                match &alert.source {
                    AlertSource::Signal {
                        indicator_id,
                        direction_filter,
                        bar_state,
                        kind_filter,
                        ..
                    } => (*indicator_id, *direction_filter, *bar_state, kind_filter.clone()),
                    _ => continue,
                };

            let matched = new_signals.iter().any(|(sig_ind_id, _bar_idx, sig_dir, sig_conf, sig_kind)| {
                if *sig_ind_id != indicator_id {
                    return false;
                }

                let dir_ok = match direction_filter {
                    SignalDirection::Any => true,
                    SignalDirection::Bullish => *sig_dir > 0,
                    SignalDirection::Bearish => *sig_dir < 0,
                };
                if !dir_ok {
                    return false;
                }

                let bar_ok = match bar_state {
                    SignalBarState::Forming => true,
                    SignalBarState::Formed => *sig_conf >= 1,
                };
                if !bar_ok {
                    return false;
                }

                if let Some(ref filter_kind) = kind_filter {
                    if sig_kind != filter_kind {
                        return false;
                    }
                }

                true
            });

            if matched {
                alert.trigger_count += 1;
                alert.last_triggered = Some("just now".to_string());
                triggered_ids.push(alert.id);

                match alert.trigger_mode {
                    AlertTriggerMode::OneShot => {
                        alert.status = AlertStatus::Triggered;
                    }
                    AlertTriggerMode::TimesN(n) if alert.trigger_count >= n => {
                        alert.status = AlertStatus::Triggered;
                    }
                    _ => {}
                }
            }
        }

        triggered_ids
    }

    // =========================================================================
    // Preset snapshot / restore (legacy flat-list path)
    // =========================================================================

    /// Take a snapshot of all alerts from a single bubble (for preset save).
    pub fn snapshot(b: &DataBubble) -> Vec<AlertItem> {
        b.alerts.clone()
    }

    /// Restore alerts from a preset snapshot into a bubble.
    pub fn restore(b: &mut DataBubble, alerts: Vec<AlertItem>) {
        b.alert_next_id = alerts.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        b.alerts = alerts;
        b.alert_last_price.clear();
    }
}
