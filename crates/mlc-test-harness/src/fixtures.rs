//! Shared fixtures: bar synthesis, BubbleStore setup, chart factories.

use mlc_backend_core::bubble::{BubbleStore};
use mlc_core::ids::BubbleId;
use mlc_core::state::{ChartId, generate_chart_id, Timeframe};
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

/// Set up a `BubbleStore` with `n` charts, each in its own auto-bubble
/// carrying the SAME instrument (exchange / account_type / symbol). Mimics
/// the live state when several tabs all show e.g. BTCUSDT 1m Spot on binance.
pub fn n_charts_same_instrument(
    n: usize,
    exchange: &str,
    account_type: &str,
    symbol: &str,
    tf: Timeframe,
) -> (BubbleStore, Vec<(ChartId, BubbleId)>) {
    let mut store = BubbleStore::new();
    let mut out = Vec::new();
    for _ in 0..n {
        let chart_id = generate_chart_id();
        let bid = store.create_group_auto([0.0, 0.0, 0.0, 0.0], symbol.to_string(), tf.clone());
        if let Some(b) = store.bubble_mut(bid) {
            b.exchange = exchange.to_string();
            b.account_type = account_type.to_string();
        }
        store.attach_chart_to_bubble(chart_id, bid).expect("attach");
        out.push((chart_id, bid));
    }
    (store, out)
}

/// Set up a single chart in an auto-bubble where the bubble's instrument
/// identity is INTENTIONALLY missing the (exchange / account_type / symbol)
/// fields but where `symbol_for_chart` / `exchange_for_chart` etc still
/// resolve through a DIFFERENT bubble — reproducing the live state seen
/// in the running binary where `bubble_for_instrument` returns a different
/// bubble than `bubble_for_chart(cid)`.
///
/// Returns: (store, chart_id, current_bubble_attached_to_chart,
/// phantom_bubble_with_matching_instrument).
pub fn chart_with_phantom_instrument_bubble(
    exchange: &str,
    account_type: &str,
    symbol: &str,
    tf: Timeframe,
) -> (BubbleStore, ChartId, BubbleId, BubbleId) {
    let mut store = BubbleStore::new();
    let chart_id = generate_chart_id();

    // Chart's attached bubble — empty instrument identity (live bug shape).
    let attached_bid = store.create_group_auto([0.0; 4], String::new(), tf.clone());
    store.attach_chart_to_bubble(chart_id, attached_bid).expect("attach");

    // A SEPARATE bubble that carries the matching instrument identity but
    // no chart attached. This is what `bubble_for_instrument` would return.
    let phantom_bid = store.create_group_auto([0.0; 4], symbol.to_string(), tf);
    if let Some(b) = store.bubble_mut(phantom_bid) {
        b.exchange = exchange.to_string();
        b.account_type = account_type.to_string();
    }

    (store, chart_id, attached_bid, phantom_bid)
}
