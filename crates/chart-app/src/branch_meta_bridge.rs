//! Phase 1 DataBubble migration — instrument bridge.
//!
//! Free helpers used by every write site that mutates the four instrument
//! fields (`symbol`/`exchange`/`account_type`/`timeframe`) on a
//! [`zengeld_chart::state::ChartContainer`]. Each helper dual-writes the new
//! value into both `ChartApp.branch_meta` (still authoritative for reads in
//! Phase 1.2) AND `BubbleStore.bubble_mut(bid)` via
//! `bubble_store.bubble_for_chart(chart_id)`.
//!
//! Phase 1.3 will flip the read helpers to read from bubble only; Phase 1.4
//! deletes branch_meta entirely.

use std::collections::HashMap;

use uzor::panels::BranchId;
use zengeld_chart::drawing::primitives_v2::Primitive;
use zengeld_chart::state::history::CommandHistory;
use zengeld_chart::state::{BranchMeta, ChartId, Timeframe};
use zengeld_chart::tag_manager::{BubbleId, BubbleStore};

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

/// Mirror the full instrument tuple into `branch_meta` AND `bubble_store`
/// for the given chart. Lazy-inserts the branch_meta entry if missing.
/// Silently skips the bubble write if the chart has no bubble yet.
pub fn mirror_instrument(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    bubble_store: &mut BubbleStore,
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
    entry.timeframe = Some(timeframe.clone());
    entry.exchange = Some(exchange.to_string());
    entry.account_type = Some(account_type.to_string());

    if let Some(bid) = bubble_store.bubble_for_chart(chart_id) {
        if let Some(bubble) = bubble_store.bubble_mut(bid) {
            bubble.symbol = symbol.to_string();
            bubble.timeframe = timeframe;
            bubble.exchange = exchange.to_string();
            bubble.account_type = account_type.to_string();
        }
    }
}

/// Mirror just the timeframe into `branch_meta` AND `bubble_store`.
pub fn mirror_timeframe(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    bubble_store: &mut BubbleStore,
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
    entry.timeframe = Some(timeframe.clone());

    if let Some(bid) = bubble_store.bubble_for_chart(chart_id) {
        if let Some(bubble) = bubble_store.bubble_mut(bid) {
            bubble.timeframe = timeframe;
        }
    }
}

/// Mirror just the exchange into `branch_meta` AND `bubble_store`.
pub fn mirror_exchange(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    bubble_store: &mut BubbleStore,
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

    if let Some(bid) = bubble_store.bubble_for_chart(chart_id) {
        if let Some(bubble) = bubble_store.bubble_mut(bid) {
            bubble.exchange = exchange.to_string();
        }
    }
}

// =============================================================================
// Read accessors (S2.7.1) — flip reads from `ChartContainer.field` to
// `branch_meta`. These are the post-cutover canonical reads.
// Phase 1.3 will flip the bodies to read from bubble instead.
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
) -> Option<BubbleId> {
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
// Group M — prev_close_price
// =============================================================================

/// Mirror the previous-close price into `branch_meta` AND `bubble_store`.
/// Silent no-op on branch_meta miss; still writes to bubble if chart has one.
pub fn mirror_prev_close_price(
    branch_meta: &mut HashMap<BranchId, BranchMeta>,
    bubble_store: &mut BubbleStore,
    chart_id: ChartId,
    prev_close_price: Option<f64>,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.prev_close_price = prev_close_price;
    }

    if let Some(bid) = bubble_store.bubble_for_chart(chart_id) {
        if let Some(bubble) = bubble_store.bubble_mut(bid) {
            bubble.prev_close_price = prev_close_price;
        }
    }
}

// =============================================================================
// Group N — pre_tag_indicator_ids / stashed_command_history /
//           stashed_primitives (separate map; Box<dyn Primitive>
//           cannot live on BranchMeta because it would break
//           BranchMeta's Clone derive)
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

/// Push a single indicator id into the pre-tag list. Silent no-op if
/// the entry is missing.
pub fn push_pre_tag_id(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    id: u64,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.pre_tag_indicator_ids.push(id);
    }
}

/// Retain only the ids for which `predicate` returns `true`. Silent no-op
/// if the entry is missing.
pub fn retain_pre_tag_ids(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
    predicate: impl Fn(&u64) -> bool,
) {
    if let Some(entry) = branch_meta.get_mut(&branch_key_for_chart(chart_id)) {
        entry.pre_tag_indicator_ids.retain(predicate);
    }
}

/// Return a reference to the pre-tag indicator id list, or `None` if
/// the entry does not exist.
pub fn read_pre_tag_ids<'a>(
    branch_meta: &'a std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<&'a Vec<u64>> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .map(|m| &m.pre_tag_indicator_ids)
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

/// Read the stashed command history for a chart.
pub fn read_stashed_command_history(
    branch_meta: &std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<&CommandHistory> {
    branch_meta
        .get(&branch_key_for_chart(chart_id))
        .and_then(|m| m.stashed_command_history.as_ref())
}

/// Take (drain) the stashed command history for a chart.
pub fn take_stashed_command_history(
    branch_meta: &mut std::collections::HashMap<BranchId, BranchMeta>,
    chart_id: ChartId,
) -> Option<CommandHistory> {
    branch_meta
        .get_mut(&branch_key_for_chart(chart_id))
        .and_then(|m| m.stashed_command_history.take())
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

/// Returns `true` if there are any stashed primitives for `chart_id`.
pub fn has_stashed_primitives(
    stashed_primitives: &std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> bool {
    stashed_primitives
        .get(&branch_key_for_chart(chart_id))
        .map_or(false, |v| !v.is_empty())
}

/// Read-only view of the stashed primitives slice for `chart_id`.
pub fn read_stashed_primitives(
    stashed_primitives: &std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Option<&Vec<Box<dyn Primitive>>> {
    stashed_primitives.get(&branch_key_for_chart(chart_id))
}

/// Mutable reference to the stashed primitives vec for `chart_id`.
pub fn stashed_primitives_mut(
    stashed_primitives: &mut std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Option<&mut Vec<Box<dyn Primitive>>> {
    stashed_primitives.get_mut(&branch_key_for_chart(chart_id))
}

/// Append a single primitive. Lazy-inserts the entry if missing.
pub fn push_stashed_primitive(
    stashed_primitives: &mut std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    prim: Box<dyn Primitive>,
) {
    stashed_primitives
        .entry(branch_key_for_chart(chart_id))
        .or_default()
        .push(prim);
}

/// Extend with multiple primitives. Lazy-inserts the entry if missing.
pub fn extend_stashed_primitives(
    stashed_primitives: &mut std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    prims: impl IntoIterator<Item = Box<dyn Primitive>>,
) {
    stashed_primitives
        .entry(branch_key_for_chart(chart_id))
        .or_default()
        .extend(prims);
}

/// Retain primitives by predicate. Returns `true` if any were removed.
pub fn retain_stashed_primitives(
    stashed_primitives: &mut std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
    pred: impl FnMut(&Box<dyn Primitive>) -> bool,
) -> bool {
    if let Some(v) = stashed_primitives.get_mut(&branch_key_for_chart(chart_id)) {
        let before = v.len();
        v.retain(pred);
        v.len() < before
    } else {
        false
    }
}

/// Clone all stashed primitives via `Primitive::clone_box`.
pub fn clone_stashed_primitives(
    stashed_primitives: &std::collections::HashMap<BranchId, Vec<Box<dyn Primitive>>>,
    chart_id: ChartId,
) -> Vec<Box<dyn Primitive>> {
    stashed_primitives
        .get(&branch_key_for_chart(chart_id))
        .map(|v| v.iter().map(|p| p.clone_box()).collect())
        .unwrap_or_default()
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
        let mut bs = BubbleStore::new();
        mirror_instrument(&mut bm, &mut bs, ChartId(7), "BTCUSDT", tf(), "Binance", "S");
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
        let mut bs = BubbleStore::new();
        mirror_instrument(&mut bm, &mut bs, ChartId(3), "BTC", tf(), "Binance", "S");
        // mutate label to detect re-insertion
        bm.get_mut(&branch_key_for_chart(ChartId(3))).unwrap().label =
            Some("sticky".to_string());
        mirror_instrument(&mut bm, &mut bs, ChartId(3), "ETH", tf(), "OKX", "M");
        let entry = bm.get(&branch_key_for_chart(ChartId(3))).unwrap();
        assert_eq!(entry.symbol.as_deref(), Some("ETH"));
        assert_eq!(entry.exchange.as_deref(), Some("OKX"));
        assert_eq!(entry.account_type.as_deref(), Some("M"));
        assert_eq!(entry.label.as_deref(), Some("sticky"), "label preserved across update");
    }

    #[test]
    fn mirror_prev_close_price_writes_on_existing_entry() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        let mut bs = BubbleStore::new();
        mirror_instrument(&mut bm, &mut bs, ChartId(11), "BTC", tf(), "Binance", "S");
        mirror_prev_close_price(&mut bm, &mut bs, ChartId(11), Some(50_000.0));
        let entry = bm.get(&branch_key_for_chart(ChartId(11))).unwrap();
        assert_eq!(entry.prev_close_price, Some(50_000.0));
    }

    #[test]
    fn mirror_pre_tag_ids_writes_on_existing_entry() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        mirror_pre_tag_ids(&mut bm, ChartId(15), vec![10, 20, 30]);
        // No entry — silent no-op
        assert!(bm.is_empty());
        mirror_instrument(&mut bm, &mut BubbleStore::default(), ChartId(15), "BTC", tf(), "Binance", "S");
        mirror_pre_tag_ids(&mut bm, ChartId(15), vec![10, 20, 30]);
        let entry = bm.get(&branch_key_for_chart(ChartId(15))).unwrap();
        assert_eq!(entry.pre_tag_indicator_ids, vec![10, 20, 30]);
    }

    #[test]
    fn mirror_stashed_command_history_round_trips() {
        let mut bm: HashMap<BranchId, BranchMeta> = HashMap::new();
        let mut bs = BubbleStore::new();
        mirror_instrument(&mut bm, &mut bs, ChartId(17), "BTC", tf(), "Binance", "S");
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
}
