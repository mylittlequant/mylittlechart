//! `AlertManager` — stateful backend manager for alerts.
//!
//! Holds the per-bubble last-tick-price cache used for crossing detection.
//! The alert *data* (`alerts: Vec<AlertItem>`, `alert_next_id`) stays on
//! `DataBubble` — only the tick-to-tick scratch cache lives here. All
//! methods are `pub(crate)`: app code never calls `AlertManager` directly,
//! it goes through the `Alerts` / `AlertsMut` facade on `BackendCore`
//! (`crates/mlc-backend/src/alert/facade.rs`).
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A and
//! `docs/mlc/plans/step-3.0-alert-manager-design.md`.

use std::collections::HashMap;

use mlc_core::alert::{
    AlertCondition, AlertItem, AlertPatch, AlertSpec, AlertStatus, AlertTriggerMode,
    DrawingExtendMode,
};
use mlc_core::ids::BubbleId;

use crate::bubble::BubbleStore;

/// Stateful alert manager. Owns the tick-to-tick last-price cache used by
/// [`AlertManager::on_tick`] to detect crossings; everything else (the
/// alert rows themselves, the id allocator) is bubble data.
#[derive(Clone, Debug, Default)]
pub struct AlertManager {
    /// Per-bubble last-tick price, used to diff against the current tick in
    /// `on_tick`. Not persisted — rebuilt from live data. Absent for a
    /// bubble until its first tick after construction/restore: that first
    /// call seeds the cache and skips crossing detection, same as the
    /// legacy `unwrap_or(0.0)` sentinel behaved before the reset.
    last_price: HashMap<BubbleId, f64>,
}

impl AlertManager {
    // =========================================================================
    // Read
    // =========================================================================

    /// All alert items on one bubble (for sidebar display, preset snapshot, etc.).
    pub(crate) fn items<'a>(&self, bubbles: &'a BubbleStore, bid: BubbleId) -> &'a [AlertItem] {
        bubbles.bubble(bid).map(|b| b.alerts.as_slice()).unwrap_or(&[])
    }

    /// All alert items across every bubble (sidebar global list, daemon fold).
    pub(crate) fn items_all(&self, bubbles: &BubbleStore) -> Vec<AlertItem> {
        bubbles.bubbles().flat_map(|b| b.alerts.iter().cloned()).collect()
    }

    /// Find an alert by ID on a known bubble.
    pub(crate) fn get<'a>(&self, bubbles: &'a BubbleStore, bid: BubbleId, id: u64) -> Option<&'a AlertItem> {
        bubbles.bubble(bid).and_then(|b| b.alerts.iter().find(|a| a.id == id))
    }

    /// Search every bubble for an alert id; returns the owning bubble plus a
    /// clone of the alert. Replaces the recurring
    /// `bm.bubbles().find_map(|b| AlertManager::get(b, id)...)` pattern.
    pub(crate) fn find_owner(&self, bubbles: &BubbleStore, id: u64) -> Option<(BubbleId, AlertItem)> {
        bubbles.bubbles().find_map(|b| {
            b.alerts.iter().find(|a| a.id == id).map(|a| (b.id, a.clone()))
        })
    }

    /// Clone of all alerts on one bubble (for preset save).
    pub(crate) fn snapshot_for_bid(&self, bubbles: &BubbleStore, bid: BubbleId) -> Vec<AlertItem> {
        bubbles.bubble(bid).map(|b| b.alerts.clone()).unwrap_or_default()
    }

    /// Total alert count across every bubble (diagnostics).
    pub(crate) fn len_total(&self, bubbles: &BubbleStore) -> usize {
        bubbles.bubbles().map(|b| b.alerts.len()).sum()
    }

    // =========================================================================
    // Create / Update / Delete
    // =========================================================================

    /// Create a new alert on `bid` from an [`AlertSpec`]. Returns the new
    /// alert's id, or 0 if `bid` does not resolve to a bubble.
    pub(crate) fn add(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, spec: AlertSpec) -> u64 {
        let Some(b) = bubbles.bubble_mut(bid) else { return 0 };
        let id = b.alert_next_id;
        b.alert_next_id += 1;
        b.alerts.push(AlertItem::from_spec(id, spec));
        id
    }

    /// Apply a partial update to an existing alert. Returns `true` if found.
    pub(crate) fn update(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, id: u64, patch: AlertPatch) -> bool {
        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let Some(alert) = b.alerts.iter_mut().find(|a| a.id == id) else { return false };
        patch.apply(alert);
        true
    }

    /// Remove an alert by ID. Returns `true` if found and removed.
    pub(crate) fn remove(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, id: u64) -> bool {
        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let before = b.alerts.len();
        b.alerts.retain(|a| a.id != id);
        b.alerts.len() != before
    }

    /// Flip `Active <-> Paused`. Returns the new status, or `None` if the
    /// alert was not found.
    pub(crate) fn toggle_pause(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, id: u64) -> Option<AlertStatus> {
        let b = bubbles.bubble_mut(bid)?;
        let alert = b.alerts.iter_mut().find(|a| a.id == id)?;
        alert.status = match alert.status {
            AlertStatus::Active => AlertStatus::Paused,
            AlertStatus::Paused => AlertStatus::Active,
            other => other,
        };
        Some(alert.status)
    }

    /// Insert a single alert item verbatim (for undo/redo restore, preset
    /// restore). Bumps `alert_next_id` so future `add` calls don't collide
    /// with the restored id.
    pub(crate) fn restore_one(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, item: AlertItem) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if item.id >= b.alert_next_id {
            b.alert_next_id = item.id + 1;
        }
        b.alerts.push(item);
    }

    // =========================================================================
    // Bulk removal — drawing / indicator cleanup
    // =========================================================================

    /// Remove every alert attached to `primitive_id`, across all bubbles.
    /// Returns the removed items so the caller can push undo commands.
    pub(crate) fn clear_for_drawing(&mut self, bubbles: &mut BubbleStore, primitive_id: u64) -> Vec<AlertItem> {
        let mut removed = Vec::new();
        for b in bubbles.bubbles_mut() {
            let mut i = 0;
            while i < b.alerts.len() {
                let matches = matches!(
                    &b.alerts[i].source,
                    mlc_core::alert::AlertSource::Drawing { primitive_id: pid, .. } if *pid == primitive_id
                );
                if matches {
                    removed.push(b.alerts.remove(i));
                } else {
                    i += 1;
                }
            }
        }
        removed
    }

    /// Remove every alert attached to `indicator_id` (Indicator or Signal
    /// source), across all bubbles. Returns the removed count — no undo
    /// path exists for this callsite today.
    pub(crate) fn clear_for_indicator(&mut self, bubbles: &mut BubbleStore, indicator_id: u64) -> usize {
        let mut count = 0;
        for b in bubbles.bubbles_mut() {
            let before = b.alerts.len();
            b.alerts.retain(|a| {
                !matches!(
                    &a.source,
                    mlc_core::alert::AlertSource::Indicator { indicator_id: iid, .. } if *iid == indicator_id
                ) && !matches!(
                    &a.source,
                    mlc_core::alert::AlertSource::Signal { indicator_id: iid, .. } if *iid == indicator_id
                )
            });
            count += before - b.alerts.len();
        }
        count
    }

    // =========================================================================
    // Dynamic crossing detection (Drawing / Indicator / Price alerts)
    // =========================================================================

    /// Resolve the effective price level for an alert at the given bar.
    ///
    /// Stateless math helper — no manager/bubble state needed, operates on
    /// a single alert. Not part of the managed surface (no bubble lookup),
    /// stays `pub` for direct use by the render path.
    pub fn resolve_price_static(
        alert: &AlertItem,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Option<f64> {
        match &alert.source {
            mlc_core::alert::AlertSource::Price { .. } => Some(alert.price),
            mlc_core::alert::AlertSource::Drawing { primitive_id, .. } => {
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
            mlc_core::alert::AlertSource::Indicator {
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
            mlc_core::alert::AlertSource::CrossingPair { .. } => None,
            mlc_core::alert::AlertSource::Signal { .. } => None,
        }
    }

    /// Check all active alerts on `bid` using dynamic price resolution.
    /// Diffs `current_price` against the manager's cached `last_price[bid]`
    /// (seeding it on the first call for a bubble — no crossing fires on
    /// that seed tick, matching the legacy "no previous price" behavior).
    ///
    /// Returns the list of triggered alert IDs.
    pub(crate) fn on_tick(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        current_price: f64,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Vec<u64> {
        let Some(b) = bubbles.bubble_mut(bid) else { return Vec::new() };

        let prev_price = self.last_price.get(&bid).copied().unwrap_or(0.0);

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

        self.last_price.insert(bid, current_price);
        triggered_ids
    }

    // =========================================================================
    // Signal alert checker
    // =========================================================================

    /// Check all active `AlertSource::Signal` alerts on `bid` against a
    /// batch of newly-emitted signals. Returns the list of triggered alert
    /// IDs.
    pub(crate) fn on_signals(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        new_signals: &[(u64, usize, i8, u8, String)],
    ) -> Vec<u64> {
        let Some(b) = bubbles.bubble_mut(bid) else { return Vec::new() };

        let mut triggered_ids = Vec::new();

        for alert in &mut b.alerts {
            if alert.status != AlertStatus::Active {
                continue;
            }

            let (indicator_id, direction_filter, bar_state, kind_filter) =
                match &alert.source {
                    mlc_core::alert::AlertSource::Signal {
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
                    mlc_core::alert::SignalDirection::Any => true,
                    mlc_core::alert::SignalDirection::Bullish => *sig_dir > 0,
                    mlc_core::alert::SignalDirection::Bearish => *sig_dir < 0,
                };
                if !dir_ok {
                    return false;
                }

                let bar_ok = match bar_state {
                    mlc_core::alert::SignalBarState::Forming => true,
                    mlc_core::alert::SignalBarState::Formed => *sig_conf >= 1,
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

    #[cfg(test)]
    pub(crate) fn set_last_price_for_test(&mut self, bid: BubbleId, price: f64) {
        self.last_price.insert(bid, price);
    }

    #[cfg(test)]
    pub(crate) fn last_price_for_test(&self, bid: BubbleId) -> Option<f64> {
        self.last_price.get(&bid).copied()
    }

    // =========================================================================
    // Scoped park/unpark — bubble-id-subset extraction (shared-BackendCore
    // arc, §5.3). Replaces whole-manager `swap()`. Alert *data* lives on
    // `DataBubble` (travels automatically with `BubbleStore` extraction);
    // this manager only owns the tick-to-tick `last_price` cache, so
    // extraction is a simple key-subset move.
    // =========================================================================

    /// Remove and return only the `last_price` entries for `bubble_ids` as a
    /// fresh, small `AlertManager`.
    ///
    /// Used exclusively by preset tab park (`mlc-app/src/persistence/
    /// profile.rs::park_active_preset`).
    pub(crate) fn extract_for_bubbles(&mut self, bubble_ids: &[BubbleId]) -> AlertManager {
        let mut out = AlertManager::default();
        for bid in bubble_ids {
            if let Some(price) = self.last_price.remove(bid) {
                out.last_price.insert(*bid, price);
            }
        }
        out
    }

    /// Re-insert every `last_price` entry from `other`.
    ///
    /// Used exclusively by preset tab unpark (`mlc-app/src/persistence/
    /// profile.rs::unpark_preset`).
    pub(crate) fn merge_for_bubbles(&mut self, other: AlertManager) {
        for (bid, price) in other.last_price {
            self.last_price.insert(bid, price);
        }
    }
}

#[cfg(test)]
mod park_unpark_tests {
    //! Round-trip coverage for `extract_for_bubbles`/`merge_for_bubbles` —
    //! the shared-BackendCore-arc scoped-park replacement for whole-manager
    //! `swap()` (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §5.3).

    use super::*;

    #[test]
    fn extract_then_merge_round_trips_to_identical_manager() {
        let mut mgr = AlertManager::default();
        let a = BubbleId(1);
        let b = BubbleId(2);
        let c = BubbleId(3);
        mgr.set_last_price_for_test(a, 100.0);
        mgr.set_last_price_for_test(b, 200.0);
        mgr.set_last_price_for_test(c, 300.0);

        let extracted = mgr.extract_for_bubbles(&[a, b]);
        assert_eq!(mgr.last_price_for_test(a), None);
        assert_eq!(mgr.last_price_for_test(b), None);
        assert_eq!(mgr.last_price_for_test(c), Some(300.0), "non-extracted entry must remain in the source manager");
        assert_eq!(extracted.last_price_for_test(a), Some(100.0));
        assert_eq!(extracted.last_price_for_test(b), Some(200.0));

        mgr.merge_for_bubbles(extracted);
        assert_eq!(mgr.last_price_for_test(a), Some(100.0));
        assert_eq!(mgr.last_price_for_test(b), Some(200.0));
        assert_eq!(mgr.last_price_for_test(c), Some(300.0));
    }

    #[test]
    fn extract_other_windows_prices_are_untouched() {
        let mut mgr = AlertManager::default();
        let window_a_bid = BubbleId(1);
        let window_b_bid = BubbleId(2);
        mgr.set_last_price_for_test(window_a_bid, 42.0);
        mgr.set_last_price_for_test(window_b_bid, 99.0);

        let parked_b = mgr.extract_for_bubbles(&[window_b_bid]);

        assert_eq!(mgr.last_price_for_test(window_a_bid), Some(42.0), "window A's price must survive parking window B's preset");
        assert_eq!(parked_b.last_price_for_test(window_a_bid), None, "extraction must not leak window A's price into window B's payload");
    }
}
