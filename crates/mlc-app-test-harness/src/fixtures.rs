//! Shared fixtures: bar synthesis, BubbleStore + TagManager setup, chart
//! factories.
//!
//! Member↔bubble routing (`attach_chart_to_tag` / `tag_for_chart`) lives on
//! `TagManager`, not `BubbleStore` (Step 6 doctrine cutover) — fixtures
//! return both, paired 1:1 by `BubbleId`.

use mlc_backend::bubble::BubbleStore;
use mlc_backend::tag::TagManager;
use mlc_core::ids::BubbleId;
use mlc_core::state::{ChartId, generate_chart_id, Timeframe};
use mlc_core::tag::Tag;
use mlc_core::Bar;

/// Synthesise a bar with monotonically-increasing timestamp.
pub fn mk_bar(ts: i64) -> Bar {
    Bar {
        timestamp: ts,
        open: 100.0,
        high: 101.0,
        low: 99.0,
        close: 100.5,
        volume: 1.0,
    }
}

/// Synthesise `n` consecutive bars at 60-second spacing.
pub fn mk_bars(n: usize) -> Vec<Bar> {
    (0..n).map(|i| mk_bar(1_780_000_000 + i as i64 * 60)).collect()
}

/// Set up a `BubbleStore` + paired `TagManager` with `n` charts, each in its
/// own auto-bubble carrying the SAME instrument (exchange / account_type /
/// symbol). Mimics the live state when several tabs all show e.g. BTCUSDT 1m
/// Spot on binance.
pub fn n_charts_same_instrument(
    n: usize,
    exchange: &str,
    account_type: &str,
    symbol: &str,
    tf: Timeframe,
) -> (BubbleStore, TagManager, Vec<(ChartId, BubbleId)>) {
    let mut store = BubbleStore::new();
    let mut tags = TagManager::new();
    let mut out = Vec::new();
    for _ in 0..n {
        let chart_id = generate_chart_id();
        let color = [0.0, 0.0, 0.0, 0.0];
        let bid = store.create_group_auto(symbol.to_string(), tf.clone());
        store.set_bubble_instrument(bid, exchange.to_string(), account_type.to_string());
        tags.insert(Tag::new_auto(bid, color));
        tags.attach_chart_to_tag(chart_id, bid).expect("attach");
        out.push((chart_id, bid));
    }
    (store, tags, out)
}

/// Set up a single chart in an auto-bubble where the bubble's instrument
/// identity is INTENTIONALLY missing the (exchange / account_type / symbol)
/// fields but where `symbol_for_chart` / `exchange_for_chart` etc still
/// resolve through a DIFFERENT bubble — reproducing the live state seen
/// in the running binary where `bubble_for_instrument` returns a different
/// bubble than `tag_for_chart(cid)`.
///
/// Returns: (store, tags, chart_id, current_bubble_attached_to_chart,
/// phantom_bubble_with_matching_instrument).
pub fn chart_with_phantom_instrument_bubble(
    exchange: &str,
    account_type: &str,
    symbol: &str,
    tf: Timeframe,
) -> (BubbleStore, TagManager, ChartId, BubbleId, BubbleId) {
    let mut store = BubbleStore::new();
    let mut tags = TagManager::new();
    let chart_id = generate_chart_id();
    let color = [0.0; 4];

    // Chart's attached bubble — empty instrument identity (live bug shape).
    let attached_bid = store.create_group_auto(String::new(), tf.clone());
    tags.insert(Tag::new_auto(attached_bid, color));
    tags.attach_chart_to_tag(chart_id, attached_bid).expect("attach");

    // A SEPARATE bubble that carries the matching instrument identity but
    // no chart attached. This is what `bubble_for_instrument` would return.
    let phantom_bid = store.create_group_auto(symbol.to_string(), tf);
    store.set_bubble_instrument(phantom_bid, exchange.to_string(), account_type.to_string());
    tags.insert(Tag::new_auto(phantom_bid, color));

    (store, tags, chart_id, attached_bid, phantom_bid)
}
