//! `AccountAlertStore` — account-wide alert storage, key
//! `(account_id, InstrumentAddress, condition)`.
//!
//! Replaces the retired `DataBubble.alerts` / `DataBubble.alert_next_id`
//! (bubble-scoped storage + a per-bubble id allocator). `BubbleId` is not
//! stable identity across cold loads — `BubbleId::generate()` is a
//! process-local `AtomicU64` re-minted from 1 every process start
//! (`mlc-core/src/ids.rs`), so two devices (or two loads of the same
//! device) collide on the same id for different instruments. `AlertItem`
//! self-describes its instrument via `address`/`account_id` — the fields
//! stable across devices/reloads — so this store keys on those instead of
//! any bubble handle, and owns its own id allocator (`next_id`) rather than
//! borrowing a bubble's.
//!
//! Crate-internal: never exported from `mlc-backend`. `AlertManager`
//! (`crate::alert::manager`) is the sole caller — it also resolves
//! `BubbleId` <-> `InstrumentAddress` (via `BubbleStore`) for the bid-based
//! shim surface the pre-account-wide facade (`Alerts`/`AlertsMut`) still
//! exposes. See
//! `docs/mlc/plans/alerts-account-wide-and-telegram-delivery-2026-09-06.md` §2.

use mlc_core::alert::{AlertItem, AlertPatch, AlertSpec, AlertStatus};
use mlc_core::instrument::InstrumentAddress;

/// Account-wide alert registry: every alert row for every account, in one
/// flat list.
#[derive(Clone, Debug, Default)]
pub struct AccountAlertStore {
    /// Own id allocator — NOT a bubble's `alert_next_id`. An alert's
    /// identity must survive the bubble it was created against being
    /// re-minted on the next cold load, so it cannot borrow a bubble-scoped
    /// counter.
    next_id: u64,
    items: Vec<AlertItem>,
}

impl AccountAlertStore {
    // =========================================================================
    // Read
    // =========================================================================

    /// Every alert belonging to `account_id`, in store order.
    pub fn for_account<'a>(&'a self, account_id: &'a str) -> impl Iterator<Item = &'a AlertItem> + 'a {
        self.items.iter().filter(move |a| a.account_id == account_id)
    }

    /// Every alert belonging to `account_id` on `address` — the shape
    /// per-tick crossing evaluation needs: one live tick names one address,
    /// and only that account's rows on it are eligible to fire for it.
    pub fn for_address<'a>(
        &'a self,
        account_id: &'a str,
        address: &'a InstrumentAddress,
    ) -> impl Iterator<Item = &'a AlertItem> + 'a {
        self.items
            .iter()
            .filter(move |a| a.account_id == account_id && a.targets_instrument(address))
    }

    /// `true` when ANY account has a live alert targeting `address` —
    /// releasing a bubble tears down its only subscription to that address,
    /// so any account's alert on it (not just one caller's own) must block
    /// the release. Backs the C1 release guard mirror,
    /// [`super::manager::AlertManager::resync_has_alerts`] /
    /// `DataBubble::has_active_alerts`.
    pub fn any_for_address(&self, address: &InstrumentAddress) -> bool {
        self.items.iter().any(|a| a.targets_instrument(address))
    }

    /// Find an alert by id, regardless of owning account.
    pub fn get(&self, id: u64) -> Option<&AlertItem> {
        self.items.iter().find(|a| a.id == id)
    }

    /// Every alert in the store, across every account — sidebar global
    /// list, daemon fold, and the legacy flat `preset.alerts` save
    /// (`mlc-app/src/input/mod.rs`'s `preset.alerts = core.alerts().items_all()`
    /// line, unaffected by this store's own persistence shape).
    pub fn items_all(&self) -> &[AlertItem] {
        &self.items
    }

    /// Total alert count across every account.
    pub fn len_total(&self) -> usize {
        self.items.len()
    }

    /// Mutable access to every item targeting `address`, regardless of
    /// account — used by `AlertManager::on_tick`/`on_signals` to update
    /// trigger state in place on crossing/signal detection.
    pub(crate) fn iter_mut_matching<'a>(
        &'a mut self,
        address: &'a InstrumentAddress,
    ) -> impl Iterator<Item = &'a mut AlertItem> + 'a {
        self.items.iter_mut().filter(move |a| a.targets_instrument(address))
    }

    // =========================================================================
    // Create / Update / Delete
    // =========================================================================

    /// Create a new alert for `account_id` from an [`AlertSpec`]. Returns
    /// the new alert's id — own counter (`next_id`), not a bubble's.
    pub fn add(&mut self, account_id: impl Into<String>, spec: AlertSpec) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let mut item = AlertItem::from_spec(id, spec);
        item.account_id = account_id.into();
        self.items.push(item);
        id
    }

    /// Apply a partial update to an existing alert. Returns `true` if found.
    pub fn update(&mut self, id: u64, patch: AlertPatch) -> bool {
        let Some(item) = self.items.iter_mut().find(|a| a.id == id) else { return false };
        patch.apply(item);
        true
    }

    /// Remove an alert by id. Returns `true` if found and removed.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|a| a.id != id);
        self.items.len() != before
    }

    /// Flip `Active <-> Paused`. Returns the new status, or `None` if the
    /// alert was not found.
    pub fn toggle_pause(&mut self, id: u64) -> Option<AlertStatus> {
        let item = self.items.iter_mut().find(|a| a.id == id)?;
        item.status = match item.status {
            AlertStatus::Active => AlertStatus::Paused,
            AlertStatus::Paused => AlertStatus::Active,
            other => other,
        };
        Some(item.status)
    }

    /// Insert a single alert item verbatim — migration fold (legacy
    /// `preset.alerts` + `SyncGroupSnapshot.alerts`, step 2) and sync-pull
    /// entry (Arc C3.4). Bumps `next_id` so future `add` calls don't
    /// collide with the restored id.
    pub fn restore_one(&mut self, item: AlertItem) {
        if item.id >= self.next_id {
            self.next_id = item.id + 1;
        }
        self.items.push(item);
    }

    // =========================================================================
    // Bulk removal — drawing / indicator cleanup
    // =========================================================================

    /// Remove every alert attached to `primitive_id`, across every account.
    /// Returns the removed items so the caller can push undo commands (and
    /// resync the release-guard mirror per affected address).
    pub fn clear_for_drawing(&mut self, primitive_id: u64) -> Vec<AlertItem> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.items.len() {
            let matches = matches!(
                &self.items[i].source,
                mlc_core::alert::AlertSource::Drawing { primitive_id: pid, .. } if *pid == primitive_id
            );
            if matches {
                removed.push(self.items.remove(i));
            } else {
                i += 1;
            }
        }
        removed
    }

    /// Remove every alert attached to `indicator_id` (Indicator or Signal
    /// source), across every account. Returns the removed items (caller
    /// reports `.len()` where the old `usize`-only contract still applies,
    /// and resyncs the release-guard mirror per affected address).
    pub fn clear_for_indicator(&mut self, indicator_id: u64) -> Vec<AlertItem> {
        let mut removed = Vec::new();
        let mut i = 0;
        while i < self.items.len() {
            let matches = matches!(
                &self.items[i].source,
                mlc_core::alert::AlertSource::Indicator { indicator_id: iid, .. } if *iid == indicator_id
            ) || matches!(
                &self.items[i].source,
                mlc_core::alert::AlertSource::Signal { indicator_id: iid, .. } if *iid == indicator_id
            );
            if matches {
                removed.push(self.items.remove(i));
            } else {
                i += 1;
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlc_core::alert::{AlertCondition, AlertSource};
    use mlc_core::instrument::AccountType;

    fn spec(symbol: &str, exchange: &str, price: f64) -> AlertSpec {
        AlertSpec {
            source: AlertSource::Price { symbol: symbol.to_string() },
            exchange: exchange.to_string(),
            price,
            ..AlertSpec::price_default()
        }
    }

    #[test]
    fn add_keys_by_account_address_condition_not_by_any_bubble_handle() {
        let mut store = AccountAlertStore::default();
        let addr = InstrumentAddress::new("binance", "BTCUSDT", AccountType::Spot);

        let id_a = store.add("account-a", spec("BTCUSDT", "binance", 65_000.0));
        let id_b = store.add("account-b", spec("BTCUSDT", "binance", 70_000.0));

        // Two different accounts, same instrument: both rows coexist,
        // distinguished by account_id — never by a bubble id (none was
        // ever passed to `add`).
        assert_ne!(id_a, id_b);
        assert_eq!(store.for_account("account-a").count(), 1);
        assert_eq!(store.for_account("account-b").count(), 1);
        assert_eq!(store.for_address("account-a", &addr).count(), 1);
        assert_eq!(store.for_address("account-b", &addr).count(), 1);
        // Cross-account read must not leak the other account's row.
        assert!(store.for_account("account-a").all(|a| a.account_id == "account-a"));
    }

    #[test]
    fn for_address_is_account_scoped_even_on_the_same_instrument() {
        let mut store = AccountAlertStore::default();
        let addr = InstrumentAddress::new("binance", "ETHUSDT", AccountType::Spot);
        let other_addr = InstrumentAddress::new("binance", "SOLUSDT", AccountType::Spot);
        store.add("acct", spec("ETHUSDT", "binance", 3_000.0));
        store.add("acct", spec("SOLUSDT", "binance", 150.0));

        assert_eq!(store.for_address("acct", &addr).count(), 1);
        assert_eq!(store.for_address("acct", &other_addr).count(), 1);
        assert_eq!(store.for_address("someone-else", &addr).count(), 0);
    }

    #[test]
    fn own_id_allocator_survives_bubble_id_re_minting_across_loads() {
        // Simulates two "cold loads": a fresh store each time, restoring an
        // alert whose `id` came from a PRIOR process where `BubbleId`
        // counters also reset to 1 — the store's own `next_id` must track
        // the alert id space, never a bubble's, so a subsequent `add`
        // cannot collide with a restored high id.
        let mut store = AccountAlertStore::default();
        let mut restored = AlertItem::from_spec(41, spec("BTCUSDT", "binance", 1.0));
        restored.account_id = "acct".to_string();
        store.restore_one(restored);

        let new_id = store.add("acct", spec("ETHUSDT", "binance", 1.0));
        assert_eq!(new_id, 42, "next_id must continue past the highest restored id");
    }

    #[test]
    fn remove_and_update_operate_by_global_id_regardless_of_account() {
        let mut store = AccountAlertStore::default();
        let id = store.add("acct", spec("BTCUSDT", "binance", 1.0));

        assert!(store.update(id, AlertPatch { price: Some(2.0), ..Default::default() }));
        assert_eq!(store.get(id).map(|a| a.price), Some(2.0));

        assert!(store.remove(id));
        assert!(store.get(id).is_none());
        assert!(!store.remove(id), "removing twice must report not-found");
    }

    #[test]
    fn any_for_address_ignores_account_type_like_targets_instrument() {
        let mut store = AccountAlertStore::default();
        store.add("acct", spec("BTCUSDT", "binance", 1.0));
        // `AlertSpec` carries no account_type — `from_spec` always stamps
        // `AccountType::default()` (Spot). A Futures bubble's address must
        // still be recognised as alerted (see `AlertItem::targets_instrument`'s
        // own doc for why account_type is deliberately excluded).
        let futures_addr = InstrumentAddress::new("binance", "BTCUSDT", AccountType::FuturesCross);
        assert!(store.any_for_address(&futures_addr));
    }

    #[test]
    fn clear_for_drawing_and_indicator_remove_across_accounts() {
        let mut store = AccountAlertStore::default();
        let drawing_spec = AlertSpec {
            source: AlertSource::Drawing { primitive_id: 7, label: String::new() },
            condition: AlertCondition::CrossingUp,
            ..spec("BTCUSDT", "binance", 1.0)
        };
        store.add("acct-a", drawing_spec.clone());
        store.add("acct-b", drawing_spec);

        let removed = store.clear_for_drawing(7);
        assert_eq!(removed.len(), 2, "must remove the primitive's alerts from every account");
        assert_eq!(store.len_total(), 0);
    }
}
