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
use zengeld_chart::drawing::primitives_v2::Primitive;
use zengeld_chart::drawing::{SignalManager, TradeManager};
use zengeld_chart::MarkerManager;
use zengeld_chart::state::history::CommandHistory;
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
// Read accessors (S2.7.1) — flip reads from `ChartContainer.field` to
// `branch_meta`. These are the post-cutover canonical reads.
// =============================================================================
//
// Convention: read helpers return `Option<&T>` (or `Option<T>` for Copy
// types) referring into the branch_meta entry. Callers fall back via
// `.unwrap_or("")` / `.unwrap_or_default()` as appropriate.
//
// Lifetime: borrows the `branch_meta` map for the lifetime of the read.
// Callers that need to mutate `branch_meta` afterwards must drop the
// borrow first (use `.map(String::from)` or `.copied()` to detach).

#[inline]
pub fn read_symbol<'a>(
    branch_meta: &'a HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<&'a str> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .and_then(|m| m.symbol.as_deref())
}

#[inline]
pub fn read_timeframe<'a>(
    branch_meta: &'a HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<&'a Timeframe> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .and_then(|m| m.timeframe.as_ref())
}

#[inline]
pub fn read_exchange<'a>(
    branch_meta: &'a HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<&'a str> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .and_then(|m| m.exchange.as_deref())
}

#[inline]
pub fn read_account_type<'a>(
    branch_meta: &'a HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<&'a str> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .and_then(|m| m.account_type.as_deref())
}

#[inline]
pub fn read_group_id(
    branch_meta: &HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<SyncGroupId> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .and_then(|m| m.group_id)
}

#[inline]
pub fn read_prev_close_price(
    branch_meta: &HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<f64> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .and_then(|m| m.prev_close_price)
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

// =============================================================================
// Group N (S2.4) — pre_tag_indicator_ids / stashed_command_history /
//                  stashed_primitives (separate map; Box<dyn Primitive>
//                  cannot live on BranchMeta because it would break
//                  BranchMeta's Clone derive)
// =============================================================================

/// Mirror the pre-tag indicator id list into branch_meta. Silent no-op
/// if the entry is missing.
pub fn mirror_pre_tag_ids(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    ids: Vec<u64>,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.pre_tag_indicator_ids = ids;
    }
}

/// Mirror the stashed window-local command history into branch_meta.
/// Silent no-op if the entry is missing.
pub fn mirror_stashed_command_history(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    history: Option<CommandHistory>,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.stashed_command_history = history;
    }
}

/// Store an owned vec of stashed primitives for `chart_id`. Replaces any
/// existing stash. `Box<dyn Primitive>` has no Clone — callers must pass
/// ownership (typically built via `iter().map(|p| p.clone_box()).collect()`
/// from the source vec).
pub fn stash_primitives(
    stashed_primitives: &mut std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    prims: Vec<Box<dyn Primitive>>,
) {
    stashed_primitives.insert(branch_key_for_chart(chart_id), prims);
}

/// Take ownership of the stashed primitives for `chart_id`, leaving the
/// entry removed. Returns `None` if there was no stash. Used on desync
/// to move stashed primitives back into the live drawing manager.
pub fn take_stashed_primitives(
    stashed_primitives: &mut std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Option<Vec<Box<dyn Primitive>>> {
    stashed_primitives.remove(&branch_key_for_chart(chart_id))
}

/// Clear the stashed-primitives entry for `chart_id` without taking
/// ownership of its contents.
pub fn clear_stashed_primitives(
    stashed_primitives: &mut std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) {
    stashed_primitives.remove(&branch_key_for_chart(chart_id));
}

// =============================================================================
// Group O (S2.5) — signal_manager / trade_manager / marker_manager
// =============================================================================
//
// These mirrors do NOT lazy-insert: same policy as Group M/N — the entry
// MUST already exist (seeded at chart construction). Silent no-op on miss.

/// Mirror signal_manager into branch_meta. Silent no-op if entry missing.
pub fn mirror_signal_manager(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    manager: SignalManager,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.signal_manager = manager;
    }
}

/// Mirror trade_manager into branch_meta. Silent no-op if entry missing.
pub fn mirror_trade_manager(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    manager: TradeManager,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.trade_manager = manager;
    }
}

/// Mirror marker_manager into branch_meta. Silent no-op if entry missing.
pub fn mirror_marker_manager(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    manager: MarkerManager,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.marker_manager = manager;
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

    #[test]
    fn mirror_pre_tag_ids_writes_on_existing_entry() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(15), "BTC", tf(), "Binance", "S");
        mirror_pre_tag_ids(&mut bm, ChartId(15), vec![10, 20, 30]);
        let entry = bm.get(&branch_key_for_chart(ChartId(15))).unwrap();
        assert_eq!(entry.pre_tag_indicator_ids, vec![10, 20, 30]);
    }

    #[test]
    fn mirror_pre_tag_ids_no_lazy_insert() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_pre_tag_ids(&mut bm, ChartId(99), vec![1, 2]);
        assert!(bm.is_empty());
    }

    #[test]
    fn mirror_stashed_command_history_round_trips() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(17), "BTC", tf(), "Binance", "S");
        let history = CommandHistory::new(50);
        mirror_stashed_command_history(&mut bm, ChartId(17), Some(history));
        let entry = bm.get(&branch_key_for_chart(ChartId(17))).unwrap();
        assert!(entry.stashed_command_history.is_some());
    }

    #[test]
    fn stashed_primitives_round_trip_take() {
        let mut sp: HashMap<BranchId, Vec<Box<dyn Primitive>>> = HashMap::new();
        // empty stash inserted then taken
        stash_primitives(&mut sp, ChartId(19), Vec::new());
        let taken = take_stashed_primitives(&mut sp, ChartId(19));
        assert!(taken.is_some());
        assert!(taken.unwrap().is_empty());
        assert!(sp.is_empty(), "take should remove the entry");
    }

    #[test]
    fn take_stashed_primitives_returns_none_when_missing() {
        let mut sp: HashMap<BranchId, Vec<Box<dyn Primitive>>> = HashMap::new();
        let taken = take_stashed_primitives(&mut sp, ChartId(123));
        assert!(taken.is_none());
    }

    // -------------------------------------------------------------------------
    // Group O (S2.5) tests
    // -------------------------------------------------------------------------

    #[test]
    fn mirror_signal_manager_round_trip() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(21), "BTC", tf(), "Binance", "S");
        let mgr = SignalManager::default();
        mirror_signal_manager(&mut bm, ChartId(21), mgr);
        // Entry exists and has the default signal_manager value.
        assert!(bm.contains_key(&branch_key_for_chart(ChartId(21))));
    }

    #[test]
    fn mirror_signal_manager_no_lazy_insert_when_missing() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_signal_manager(&mut bm, ChartId(99), SignalManager::default());
        assert!(bm.is_empty());
    }

    #[test]
    fn mirror_trade_manager_round_trip() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(23), "ETH", tf(), "Binance", "S");
        mirror_trade_manager(&mut bm, ChartId(23), TradeManager::default());
        assert!(bm.contains_key(&branch_key_for_chart(ChartId(23))));
    }

    #[test]
    fn mirror_trade_manager_no_lazy_insert_when_missing() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_trade_manager(&mut bm, ChartId(99), TradeManager::default());
        assert!(bm.is_empty());
    }

    #[test]
    fn mirror_marker_manager_round_trip() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_instrument(&mut bm, ChartId(25), "SOL", tf(), "Binance", "S");
        mirror_marker_manager(&mut bm, ChartId(25), MarkerManager::default());
        assert!(bm.contains_key(&branch_key_for_chart(ChartId(25))));
    }

    #[test]
    fn mirror_marker_manager_no_lazy_insert_when_missing() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_marker_manager(&mut bm, ChartId(99), MarkerManager::default());
        assert!(bm.is_empty());
    }
}
