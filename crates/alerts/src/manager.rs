//! AlertManager — central owner of all alerts.
//!
//! Provides CRUD operations, snapshot/restore for presets, and crossing detection.

use crate::types::{AlertCondition, AlertItem, AlertSource, AlertStatus, AlertTriggerMode, DrawingExtendMode};

/// Central alert manager. Owns all alert items and the ID counter.
///
/// Chart-app creates one instance. Sidebar reads `items()` for display.
/// Preset save/restore uses `snapshot()` / `restore()`.
#[derive(Clone, Debug)]
pub struct AlertManager {
    items: Vec<AlertItem>,
    next_id: u64,
    /// Price of the active symbol as of the previous tick — for crossing detection.
    last_price: f64,
}

impl Default for AlertManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AlertManager {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            last_price: 0.0,
        }
    }

    // =========================================================================
    // Read
    // =========================================================================

    /// All alert items (for sidebar display, preset snapshot, etc.).
    pub fn items(&self) -> &[AlertItem] {
        &self.items
    }

    /// Find an alert by ID.
    pub fn get(&self, id: u64) -> Option<&AlertItem> {
        self.items.iter().find(|a| a.id == id)
    }

    /// Find a mutable alert by ID.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut AlertItem> {
        self.items.iter_mut().find(|a| a.id == id)
    }

    /// Number of alerts.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Current next_id (for diagnostics).
    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    // =========================================================================
    // Create
    // =========================================================================

    /// Create a new alert from an `AlertSource` and return its ID.
    pub fn create(
        &mut self,
        source: AlertSource,
        name: &str,
        price: f64,
        condition: AlertCondition,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let alert = AlertItem::new(id, source, name, price, condition, AlertStatus::Active);
        self.items.push(alert);
        id
    }

    /// Convenience wrapper: create a horizontal price alert for a symbol.
    ///
    /// This keeps backward compatibility for call sites that previously called
    /// `create(&symbol, name, price, condition)` before the `AlertSource` refactor.
    pub fn create_price_alert(
        &mut self,
        symbol: &str,
        name: &str,
        price: f64,
        condition: AlertCondition,
    ) -> u64 {
        self.create(
            AlertSource::Price {
                symbol: symbol.to_string(),
            },
            name,
            price,
            condition,
        )
    }

    // =========================================================================
    // Update
    // =========================================================================

    /// Update an existing alert's core fields. Returns true if found.
    pub fn update(
        &mut self,
        id: u64,
        name: &str,
        price: f64,
        condition: AlertCondition,
    ) -> bool {
        if let Some(alert) = self.get_mut(id) {
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
        &mut self,
        id: u64,
        source: AlertSource,
        name: &str,
        price: f64,
        price2: f64,
        percentage: f64,
        condition: AlertCondition,
        trigger_mode: AlertTriggerMode,
        transports: Vec<crate::types::AlertTransport>,
    ) -> bool {
        if let Some(alert) = self.get_mut(id) {
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
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|a| a.id != id);
        self.items.len() != before
    }

    /// Remove all alerts.
    pub fn clear(&mut self) {
        self.items.clear();
        self.next_id = 1;
        self.last_price = 0.0;
    }

    /// Remove all alerts attached to a specific drawing primitive.
    ///
    /// Called automatically when a drawing primitive is deleted so that no
    /// orphaned `AlertSource::Drawing` entries remain in the manager.
    pub fn remove_alerts_for_drawing(&mut self, primitive_id: u64) {
        self.items.retain(|a| {
            !matches!(&a.source, AlertSource::Drawing { primitive_id: pid, .. } if *pid == primitive_id)
        });
    }

    /// Remove all alerts attached to a specific indicator instance.
    ///
    /// Called automatically when an indicator is removed so that no orphaned
    /// `AlertSource::Indicator` or `AlertSource::Signal` entries remain.
    pub fn remove_alerts_for_indicator(&mut self, indicator_id: u64) {
        self.items.retain(|a| {
            match &a.source {
                AlertSource::Indicator { indicator_id: iid, .. } if *iid == indicator_id => false,
                AlertSource::Signal { indicator_id: iid, .. } if *iid == indicator_id => false,
                _ => true,
            }
        });
    }

    // =========================================================================
    // Crossing detection (called every tick)
    // =========================================================================

    /// Check all active Price alerts against the current price.
    /// Returns list of alert IDs that triggered this tick.
    ///
    /// TODO(Phase 4): expand to support non-Price sources (Drawing, Indicator, CrossingPair).
    pub fn check_crossings(&mut self, price: f64) -> Vec<u64> {
        let prev = self.last_price;
        self.last_price = price;

        // Skip first tick (no previous price to compare against).
        if prev == 0.0 {
            return Vec::new();
        }

        let mut triggered_ids = Vec::new();

        for alert in &mut self.items {
            if alert.status != AlertStatus::Active {
                continue;
            }
            let triggered = match alert.condition {
                AlertCondition::CrossingUp => prev < alert.price && price >= alert.price,
                AlertCondition::CrossingDown => prev > alert.price && price <= alert.price,
                AlertCondition::Crossing => {
                    (prev < alert.price && price >= alert.price)
                        || (prev > alert.price && price <= alert.price)
                }
                AlertCondition::GreaterThan => price > alert.price,
                AlertCondition::LessThan => price < alert.price,
                _ => false,
            };
            if triggered {
                alert.trigger_count += 1;
                alert.last_triggered = Some("just now".to_string());
                triggered_ids.push(alert.id);

                // Apply trigger mode: deactivate on OneShot, or when TimesN exhausted.
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
    // Dynamic crossing detection (Drawing / Indicator alerts)
    // =========================================================================

    /// Resolve the effective price level for an alert at the given bar.
    ///
    /// - `Price` alerts: returns `alert.price` (fixed horizontal line).
    /// - `Drawing` alerts: linearly interpolates the primitive's price at `current_bar`,
    ///   respecting `DrawingExtendMode` so alerts only fire within the visible segment.
    /// - `Indicator` alerts: returns the last value in the provided values slice.
    /// - `CrossingPair`: returns `None` (not yet supported).
    ///
    /// The `drawing_points` tuple is `(primitive_id, points, extend_mode)`.
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

                // Respect the extend mode: only extrapolate in the allowed direction(s).
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
                    DrawingExtendMode::Both => {
                        // Infinite line — always extrapolate, no bounds check.
                    }
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
            AlertSource::CrossingPair { .. } => None, // TODO
            AlertSource::Signal { .. } => {
                // Signal alerts don't have a price level to resolve —
                // they fire based on signal events, not price crossing.
                // Return None so no horizontal line is drawn.
                None
            }
        }
    }

    /// Check all active alerts using dynamic price resolution.
    ///
    /// For `Price` alerts this behaves identically to `check_crossings`.
    /// For `Drawing` and `Indicator` alerts the level is resolved dynamically
    /// from the provided external data so the alert line tracks the object shape.
    /// Drawing alerts respect `DrawingExtendMode` — they will not fire outside
    /// the allowed extent of the primitive.
    ///
    /// The `drawing_points` tuple is `(primitive_id, points, extend_mode)`.
    ///
    /// Returns list of triggered alert IDs.
    pub fn check_crossings_dynamic(
        &mut self,
        current_price: f64,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Vec<u64> {
        let mut triggered_ids = Vec::new();

        for alert in &mut self.items {
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
            let prev_price = self.last_price;

            let triggered = match alert.condition {
                AlertCondition::CrossingUp => {
                    // Skip first tick when prev state is uninitialised.
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

            // Always update the previous dynamic price so the next tick has
            // a valid reference even when no crossing occurred.
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

        self.last_price = current_price;
        triggered_ids
    }

    // =========================================================================
    // Preset snapshot / restore
    // =========================================================================

    /// Take a snapshot of all alerts (for preset save).
    pub fn snapshot(&self) -> Vec<AlertItem> {
        self.items.clone()
    }

    /// Restore alerts from a preset snapshot.
    pub fn restore(&mut self, alerts: Vec<AlertItem>) {
        self.next_id = alerts.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        self.items = alerts;
        self.last_price = 0.0;
    }
}
