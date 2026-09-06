//! `AlertManager` — stateful backend manager for alerts.
//!
//! Alert *data* lives in the account-wide [`AccountAlertStore`]
//! (`crate::alert::store`), keyed `(account_id, InstrumentAddress,
//! condition)` — NOT on `DataBubble` (retired `alerts`/`alert_next_id`
//! fields, Arc C3: `BubbleId` is process-local, re-minted every cold load,
//! so it cannot be a stable key). This manager holds that store plus the
//! per-bubble last-tick-price cache used for crossing detection. All
//! methods are `pub(crate)`: app code never calls `AlertManager` directly,
//! it goes through the `Alerts` / `AlertsMut` facade on `BackendCore`
//! (`crates/mlc-backend/src/alert/facade.rs`).
//!
//! Two method families:
//! - **bid-based shims** (`items`, `get`, `find_owner`, `snapshot_for_bid`,
//!   `add`, `update`, `remove`, `toggle_pause`, `restore_one`, `on_tick`,
//!   `on_signals`) — exact pre-account-wide signatures, kept so `mlc-app`
//!   (not yet rewritten — step 2 of
//!   `docs/mlc/plans/alerts-account-wide-and-telegram-delivery-2026-09-06.md`)
//!   keeps compiling unchanged. Every alert created through a bid-based
//!   shim lands under [`LOCAL_SHIM_ACCOUNT_ID`] — single local scope,
//!   matching today's single-account behavior exactly.
//! - **account-wide methods** (`for_account`, `for_address`,
//!   `add_for_account`, `update_by_id`, `remove_by_id`, `restore_item`) —
//!   the real, address-keyed API step 2/5 build on.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A,
//! `docs/mlc/plans/step-3.0-alert-manager-design.md`, and
//! `docs/mlc/plans/alerts-account-wide-and-telegram-delivery-2026-09-06.md`.

use std::collections::HashMap;

use mlc_core::alert::{
    AlertCondition, AlertItem, AlertPatch, AlertSpec, AlertStatus, AlertTriggerMode,
    DrawingExtendMode,
};
use mlc_core::ids::BubbleId;
use mlc_core::instrument::InstrumentAddress;

use crate::bubble::BubbleStore;

use super::store::AccountAlertStore;

/// Placeholder account scope for the pre-account-wide bid-based shim
/// methods below, until `mlc-app` resolves and passes a real `account_id`
/// (step 2). Every alert created through a shim call lands here — single
/// local scope, matching today's single-account behavior exactly.
pub(crate) const LOCAL_SHIM_ACCOUNT_ID: &str = "local";

/// Stateful alert manager. Owns the account-wide [`AccountAlertStore`] plus
/// the tick-to-tick last-price cache [`AlertManager::on_tick`] uses to
/// detect crossings.
#[derive(Clone, Debug, Default)]
pub struct AlertManager {
    /// Per-bubble last-tick price, used to diff against the current tick in
    /// `on_tick`. Not persisted — rebuilt from live data. Absent for a
    /// bubble until its first tick after construction/restore: that first
    /// call seeds the cache and skips crossing detection, same as the
    /// legacy `unwrap_or(0.0)` sentinel behaved before the reset.
    last_price: HashMap<BubbleId, f64>,

    /// Account-wide alert rows. Never travels with a park/unpark
    /// extraction (see [`Self::extract_for_bubbles`]'s own doc) — alerts
    /// are address-keyed, not bubble-keyed, so parking a preset's bubbles
    /// must not park its alerts away too (Arc C3.2: an alert watches
    /// "regardless of any window").
    store: AccountAlertStore,
}

impl AlertManager {
    // =========================================================================
    // Read — bid-based shim (pre-account-wide `mlc-app` compat)
    // =========================================================================

    /// Every alert targeting `bid`'s instrument, across every account (for
    /// sidebar display, preset snapshot, etc.).
    pub(crate) fn items(&self, bubbles: &BubbleStore, bid: BubbleId) -> Vec<AlertItem> {
        let Some(address) = bubbles.bubble(bid).map(|b| &b.address) else { return Vec::new() };
        self.store.items_all().iter().filter(|a| a.targets_instrument(address)).cloned().collect()
    }

    /// All alert items across every account (sidebar global list, daemon
    /// fold, legacy flat `preset.alerts` save).
    pub(crate) fn items_all(&self) -> Vec<AlertItem> {
        self.store.items_all().to_vec()
    }

    /// Find an alert by ID, scoped to `bid`'s instrument.
    pub(crate) fn get<'a>(&'a self, bubbles: &BubbleStore, bid: BubbleId, id: u64) -> Option<&'a AlertItem> {
        let address = bubbles.bubble(bid).map(|b| b.address.clone())?;
        self.store.get(id).filter(|a| a.targets_instrument(&address))
    }

    /// Search the whole store for an alert id; resolves the live bubble (if
    /// any) whose instrument the alert targets. Replaces the recurring
    /// `bm.bubbles().find_map(|b| AlertManager::get(b, id)...)` pattern.
    pub(crate) fn find_owner(&self, bubbles: &BubbleStore, id: u64) -> Option<(BubbleId, AlertItem)> {
        let item = self.store.get(id)?.clone();
        let bid = bubbles.bubbles().find(|b| item.targets_instrument(&b.address)).map(|b| b.id)?;
        Some((bid, item))
    }

    /// Clone of every alert targeting `bid`'s instrument (for preset save).
    pub(crate) fn snapshot_for_bid(&self, bubbles: &BubbleStore, bid: BubbleId) -> Vec<AlertItem> {
        self.items(bubbles, bid)
    }

    /// Total alert count across every account.
    pub(crate) fn len_total(&self) -> usize {
        self.store.len_total()
    }

    // =========================================================================
    // Create / Update / Delete — bid-based shim
    // =========================================================================

    /// Create a new alert on `bid`'s instrument from an [`AlertSpec`],
    /// under [`LOCAL_SHIM_ACCOUNT_ID`]. Returns the new alert's id, or 0 if
    /// `bid` does not resolve to a bubble.
    pub(crate) fn add(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, spec: AlertSpec) -> u64 {
        let Some(address) = bubbles.bubble(bid).map(|b| b.address.clone()) else { return 0 };
        let id = self.store.add(LOCAL_SHIM_ACCOUNT_ID, spec);
        self.resync_has_alerts(bubbles, &address);
        id
    }

    /// Apply a partial update to an existing alert, scoped to `bid`'s
    /// instrument. Returns `true` if found.
    pub(crate) fn update(&mut self, bubbles: &BubbleStore, bid: BubbleId, id: u64, patch: AlertPatch) -> bool {
        let Some(address) = bubbles.bubble(bid).map(|b| b.address.clone()) else { return false };
        match self.store.get(id) {
            Some(item) if item.targets_instrument(&address) => self.store.update(id, patch),
            _ => false,
        }
    }

    /// Remove an alert by ID, scoped to `bid`'s instrument. Returns `true`
    /// if found and removed.
    pub(crate) fn remove(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, id: u64) -> bool {
        let Some(address) = bubbles.bubble(bid).map(|b| b.address.clone()) else { return false };
        let owned = self.store.get(id).is_some_and(|item| item.targets_instrument(&address));
        if !owned {
            return false;
        }
        let removed = self.store.remove(id);
        if removed {
            self.resync_has_alerts(bubbles, &address);
        }
        removed
    }

    /// Flip `Active <-> Paused`, scoped to `bid`'s instrument. Returns the
    /// new status, or `None` if the alert was not found.
    pub(crate) fn toggle_pause(&mut self, bubbles: &BubbleStore, bid: BubbleId, id: u64) -> Option<AlertStatus> {
        let address = bubbles.bubble(bid).map(|b| b.address.clone())?;
        let owned = self.store.get(id).is_some_and(|item| item.targets_instrument(&address));
        if !owned {
            return None;
        }
        self.store.toggle_pause(id)
    }

    /// Insert a single alert item verbatim (for undo/redo restore, preset
    /// restore). `bid` is trusted, unchecked — same permissiveness as the
    /// pre-account-wide restore path. Stamps [`LOCAL_SHIM_ACCOUNT_ID`] when
    /// `item.account_id` is empty (legacy snapshot).
    pub(crate) fn restore_one(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, mut item: AlertItem) {
        let Some(address) = bubbles.bubble(bid).map(|b| b.address.clone()) else { return };
        if item.account_id.is_empty() {
            item.account_id = LOCAL_SHIM_ACCOUNT_ID.to_string();
        }
        self.store.restore_one(item);
        self.resync_has_alerts(bubbles, &address);
    }

    // =========================================================================
    // Bulk removal — drawing / indicator cleanup
    // =========================================================================

    /// Remove every alert attached to `primitive_id`, across every account.
    /// Returns the removed items so the caller can push undo commands.
    pub(crate) fn clear_for_drawing(&mut self, bubbles: &mut BubbleStore, primitive_id: u64) -> Vec<AlertItem> {
        let removed = self.store.clear_for_drawing(primitive_id);
        self.resync_for_removed(bubbles, &removed);
        removed
    }

    /// Remove every alert attached to `indicator_id` (Indicator or Signal
    /// source), across every account. Returns the removed count — no undo
    /// path exists for this callsite today.
    pub(crate) fn clear_for_indicator(&mut self, bubbles: &mut BubbleStore, indicator_id: u64) -> usize {
        let removed = self.store.clear_for_indicator(indicator_id);
        self.resync_for_removed(bubbles, &removed);
        removed.len()
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

    /// Check all active alerts targeting `bid`'s instrument using dynamic
    /// price resolution. Diffs `current_price` against the manager's
    /// cached `last_price[bid]` (seeding it on the first call for a bubble
    /// — no crossing fires on that seed tick, matching the legacy "no
    /// previous price" behavior).
    ///
    /// Returns the list of triggered alert IDs.
    pub(crate) fn on_tick(
        &mut self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        current_price: f64,
        current_bar: f64,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Vec<u64> {
        let Some(address) = bubbles.bubble(bid).map(|b| b.address.clone()) else { return Vec::new() };

        let prev_price = self.last_price.get(&bid).copied().unwrap_or(0.0);

        let mut triggered_ids = Vec::new();

        for alert in self.store.iter_mut_matching(&address) {
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

    /// Check all active `AlertSource::Signal` alerts targeting `bid`'s
    /// instrument against a batch of newly-emitted signals. Returns the
    /// list of triggered alert IDs.
    pub(crate) fn on_signals(
        &mut self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        new_signals: &[(u64, usize, i8, u8, String)],
    ) -> Vec<u64> {
        let Some(address) = bubbles.bubble(bid).map(|b| b.address.clone()) else { return Vec::new() };

        let mut triggered_ids = Vec::new();

        for alert in self.store.iter_mut_matching(&address) {
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

    // =========================================================================
    // Account-wide API (Arc C3 — real address-keyed surface)
    // =========================================================================

    /// Every alert belonging to `account_id`, across every instrument.
    pub(crate) fn for_account(&self, account_id: &str) -> Vec<AlertItem> {
        self.store.for_account(account_id).cloned().collect()
    }

    /// Every alert belonging to `account_id` on `address`.
    pub(crate) fn for_address(&self, account_id: &str, address: &InstrumentAddress) -> Vec<AlertItem> {
        self.store.for_address(account_id, address).cloned().collect()
    }

    /// Create a new alert for `account_id` from an [`AlertSpec`] — own
    /// counter, no bubble required to already exist (callers ensure a
    /// subscription separately via `BackendCore::ensure_alert_bubble`
    /// before calling this, same as the bid-based shim's `bid` had to
    /// already resolve to a bubble). Returns the new alert's id.
    pub(crate) fn add_for_account(
        &mut self,
        bubbles: &mut BubbleStore,
        account_id: impl Into<String>,
        spec: AlertSpec,
    ) -> u64 {
        let id = self.store.add(account_id, spec);
        if let Some(item) = self.store.get(id) {
            let address = InstrumentAddress::new(item.exchange(), item.symbol(), item.address.account_type);
            self.resync_has_alerts(bubbles, &address);
        }
        id
    }

    /// Apply a partial update to an existing alert by its global id —
    /// address never changes via a patch, so no release-guard resync is
    /// needed. Returns `true` if found.
    pub(crate) fn update_by_id(&mut self, id: u64, patch: AlertPatch) -> bool {
        self.store.update(id, patch)
    }

    /// Remove an alert by its global id. Returns `true` if found and removed.
    pub(crate) fn remove_by_id(&mut self, bubbles: &mut BubbleStore, id: u64) -> bool {
        let address = self.store.get(id).map(|item| {
            InstrumentAddress::new(item.exchange(), item.symbol(), item.address.account_type)
        });
        let removed = self.store.remove(id);
        if removed {
            if let Some(address) = address {
                self.resync_has_alerts(bubbles, &address);
            }
        }
        removed
    }

    /// Insert a single alert item verbatim under its own `account_id`
    /// (migration fold + sync-pull entry, Arc C3.3/C3.4).
    pub(crate) fn restore_item(&mut self, bubbles: &mut BubbleStore, item: AlertItem) {
        let address = InstrumentAddress::new(item.exchange(), item.symbol(), item.address.account_type);
        self.store.restore_one(item);
        self.resync_has_alerts(bubbles, &address);
    }

    // =========================================================================
    // Release-guard mirror (C1, Arc C3.2)
    // =========================================================================

    /// Recompute `DataBubble.has_active_alerts` for every bubble targeting
    /// `address`, immediately after a store mutation that could have
    /// changed alert presence there.
    ///
    /// Alert data moved off `DataBubble` into this manager's account-wide
    /// [`AccountAlertStore`], but `BubbleStore::clear_all_kline_layers`'s
    /// C1 release guard ("an alerted bubble is never released") runs on a
    /// standalone, already-extracted `BubbleStore` — a parked preset tab's
    /// cache entry with no live `AlertManager` reference to ask. This
    /// boolean mirror is the answer it can still consult on its own.
    fn resync_has_alerts(&self, bubbles: &mut BubbleStore, address: &InstrumentAddress) {
        let has_alerts = self.store.any_for_address(address);
        bubbles.set_has_active_alerts(address, has_alerts);
    }

    /// Recompute the release-guard mirror for every distinct instrument
    /// among `removed` — used after a bulk removal (`clear_for_drawing`/
    /// `clear_for_indicator`) that may span several instruments at once.
    fn resync_for_removed(&self, bubbles: &mut BubbleStore, removed: &[AlertItem]) {
        for item in removed {
            let address = InstrumentAddress::new(item.exchange(), item.symbol(), item.address.account_type);
            self.resync_has_alerts(bubbles, &address);
        }
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
    // arc, §5.3). Replaces whole-manager `swap()`. Alert *data* is
    // account-wide, address-keyed [`AccountAlertStore`] state — it never
    // travels with a park/unpark extraction (Arc C3.2: an alert watches
    // "regardless of any window", so parking a preset's bubbles away must
    // not park its alerts away too). This manager only extracts/merges the
    // tick-to-tick `last_price` cache, a simple key-subset move.
    // =========================================================================

    /// Remove and return only the `last_price` entries for `bubble_ids` as a
    /// fresh, small `AlertManager` (its `store` stays empty — see the field
    /// doc on `AlertManager::store`).
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
