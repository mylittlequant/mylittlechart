//! Phase 4 — S2.2 instrument bridge.
//!
//! Free helpers used by every write site that mutates the four instrument
//! fields (`symbol`/`exchange`/`account_type`/`timeframe`) on a
//! [`zengeld_chart::state::ChartContainer`]. Each helper mirrors the new
//! value into `ChartApp.branch_meta` using `entry().or_insert_with(...)`
//! so callers do not need to seed the entry up-front (lazy-insert hybrid).
//!
//! Per the S2.2 audit (`docs/plans/phase4-s2.2-instrument-field-audit.md`
//! §10): struct fields on `ChartContainer` stay read-authoritative through
//! S2.2..S2.6; S2.7 flips reads to `branch_meta` and removes the duplicate
//! fields.
//!
//! These helpers take **disjoint** `&mut HashMap<BranchId, BranchMeta>`
//! plus owned/borrowed instrument values so callers can run them after
//! dropping the `panel_grid.containers_mut()` borrow (split-block
//! pattern). The intentional avoidance of `&mut ChartApp` means callers
//! can keep their `panel_grid` borrow alive when needed and update
//! `branch_meta` in a separate block.

use std::collections::HashMap;

use uzor::panels::BranchId;
use zengeld_chart::state::{BranchMeta, ChartId, ConnectionStatus, Timeframe};
use zengeld_chart::tag_manager::SyncGroupId;

/// Map a `ChartId` to the temporary `BranchId` used by S2 (per S2.G6 the
/// keys are `BranchId(chart_id.0)` until S3 wires real uzor branch ids).
#[inline]
pub fn branch_key_for_chart(chart_id: ChartId) -> BranchId {
    BranchId(chart_id.0)
}

/// Ensure a `ChartContainer`-flavored `BranchMeta` entry exists for the
/// given chart, returning a mutable reference to it.
///
/// On miss, inserts a fresh entry seeded from the provided instrument
/// values. On hit, returns the existing entry so caller may overwrite
/// individual fields.
pub fn ensure_chart_container_meta<'a>(
    branch_meta: &'a mut HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    symbol: &str,
    timeframe: Timeframe,
    exchange: &str,
    account_type: &str,
) -> &'a mut BranchMeta {
    branch_meta
        .entry(branch_key_for_chart(chart_id))
        .or_insert_with(|| BranchMeta::chart_container(symbol, timeframe, exchange, account_type))
}

/// Mirror the full instrument tuple into `branch_meta` for the given
/// chart. Lazy-inserts the entry if missing. Used by every dual-write
/// site that changes more than one of (symbol, exchange, account_type,
/// timeframe) atomically.
pub fn mirror_instrument(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    symbol: &str,
    timeframe: Timeframe,
    exchange: &str,
    account_type: &str,
) {
    let entry = ensure_chart_container_meta(
        branch_meta,
        chart_id,
        symbol,
        timeframe.clone(),
        exchange,
        account_type,
    );
    entry.symbol = Some(symbol.to_string());
    entry.timeframe = Some(timeframe);
    entry.exchange = Some(exchange.to_string());
    entry.account_type = Some(account_type.to_string());
}

/// Mirror just the symbol into `branch_meta`. Lazy-inserts using the
/// caller-supplied defaults for the other fields when the entry is new.
pub fn mirror_symbol(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    symbol: &str,
    timeframe_for_seed: Timeframe,
    exchange_for_seed: &str,
    account_type_for_seed: &str,
) {
    let entry = ensure_chart_container_meta(
        branch_meta,
        chart_id,
        symbol,
        timeframe_for_seed,
        exchange_for_seed,
        account_type_for_seed,
    );
    entry.symbol = Some(symbol.to_string());
}

/// Mirror just the timeframe.
pub fn mirror_timeframe(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    timeframe: Timeframe,
    symbol_for_seed: &str,
    exchange_for_seed: &str,
    account_type_for_seed: &str,
) {
    let entry = ensure_chart_container_meta(
        branch_meta,
        chart_id,
        symbol_for_seed,
        timeframe.clone(),
        exchange_for_seed,
        account_type_for_seed,
    );
    entry.timeframe = Some(timeframe);
}

/// Mirror just the exchange.
pub fn mirror_exchange(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    exchange: &str,
    symbol_for_seed: &str,
    timeframe_for_seed: Timeframe,
    account_type_for_seed: &str,
) {
    let entry = ensure_chart_container_meta(
        branch_meta,
        chart_id,
        symbol_for_seed,
        timeframe_for_seed,
        exchange,
        account_type_for_seed,
    );
    entry.exchange = Some(exchange.to_string());
}

/// Mirror just the account_type.
pub fn mirror_account_type(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    account_type: &str,
    symbol_for_seed: &str,
    timeframe_for_seed: Timeframe,
    exchange_for_seed: &str,
) {
    let entry = ensure_chart_container_meta(
        branch_meta,
        chart_id,
        symbol_for_seed,
        timeframe_for_seed,
        exchange_for_seed,
        account_type,
    );
    entry.account_type = Some(account_type.to_string());
}

// =============================================================================
// Group M (S2.3) — group_id / prev_close_price / connection_status
// =============================================================================
//
// These mirrors do NOT lazy-insert: the entry MUST already exist when a
// group_id / prev_close_price / connection_status write fires (all such
// writes happen on charts that S2.2 already seeded). A miss is logged
// (silent-skip) rather than synthesized so callers don't accidentally
// create a half-shaped entry without instrument context.

/// Mirror the sync `group_id` for a chart's branch into branch_meta.
/// Silent no-op if the entry is missing (entry should exist post-S2.2).
pub fn mirror_group_id(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    group_id: Option<SyncGroupId>,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.group_id = group_id;
    }
}

/// Mirror the previous-close price (derived from `bars[0].open`) into
/// branch_meta. Silent no-op if the entry is missing.
pub fn mirror_prev_close_price(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    prev_close_price: Option<f64>,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.prev_close_price = prev_close_price;
    }
}

/// Mirror the connection status into branch_meta. Silent no-op if the
/// entry is missing.
pub fn mirror_connection_status(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    status: ConnectionStatus,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.connection_status = Some(status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn tf() -> Timeframe {
        Timeframe::h1()
    }

    #[test]
    fn mirror_instrument_lazy_inserts() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(7), "BTCUSDT", tf(), "Binance", "S");
        let key = branch_key_for_chart(ChartId(7));
        let entry = bm.get(&key).expect("entry seeded");
        assert_eq!(entry.symbol.as_deref(), Some("BTCUSDT"));
        assert_eq!(entry.exchange.as_deref(), Some("Binance"));
        assert_eq!(entry.account_type.as_deref(), Some("S"));
        assert!(entry.timeframe.is_some());
    }

    #[test]
    fn mirror_instrument_updates_existing_without_recreating() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(3), "BTC", tf(), "Binance", "S");
        // mutate label to detect re-insertion
        bm.get_mut(&branch_key_for_chart(ChartId(3))).unwrap().label =
            Some("sticky".to_string());
        mirror_instrument(&mut bm, ChartId(3), "ETH", tf(), "OKX", "M");
        let entry = bm.get(&branch_key_for_chart(ChartId(3))).unwrap();
        assert_eq!(entry.symbol.as_deref(), Some("ETH"));
        assert_eq!(entry.exchange.as_deref(), Some("OKX"));
        assert_eq!(entry.account_type.as_deref(), Some("M"));
        assert_eq!(entry.label.as_deref(), Some("sticky"), "label preserved across update");
    }

    #[test]
    fn mirror_symbol_only_updates_symbol() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(1), "BTC", tf(), "Binance", "S");
        mirror_symbol(&mut bm, ChartId(1), "ETHUSDT", tf(), "Binance", "S");
        let entry = bm.get(&branch_key_for_chart(ChartId(1))).unwrap();
        assert_eq!(entry.symbol.as_deref(), Some("ETHUSDT"));
        assert_eq!(entry.exchange.as_deref(), Some("Binance"));
    }

    #[test]
    fn mirror_group_id_no_lazy_insert_when_missing() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        // Entry not seeded — mirror should be silent no-op.
        mirror_group_id(&mut bm, ChartId(99), Some(SyncGroupId(7)));
        assert!(bm.is_empty());
    }

    #[test]
    fn mirror_group_id_writes_on_existing_entry() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(5), "BTC", tf(), "Binance", "S");
        mirror_group_id(&mut bm, ChartId(5), Some(SyncGroupId(42)));
        let entry = bm.get(&branch_key_for_chart(ChartId(5))).unwrap();
        assert_eq!(entry.group_id, Some(SyncGroupId(42)));
    }

    #[test]
    fn mirror_prev_close_price_writes_on_existing_entry() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(11), "BTC", tf(), "Binance", "S");
        mirror_prev_close_price(&mut bm, ChartId(11), Some(50_000.0));
        let entry = bm.get(&branch_key_for_chart(ChartId(11))).unwrap();
        assert_eq!(entry.prev_close_price, Some(50_000.0));
    }

    #[test]
    fn mirror_connection_status_writes_on_existing_entry() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(13), "BTC", tf(), "Binance", "S");
        mirror_connection_status(&mut bm, ChartId(13), ConnectionStatus::Delayed);
        let entry = bm.get(&branch_key_for_chart(ChartId(13))).unwrap();
        assert_eq!(entry.connection_status, Some(ConnectionStatus::Delayed));
    }
}
