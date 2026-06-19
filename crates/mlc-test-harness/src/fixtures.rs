//! Shared fixtures: bar synthesis, BubbleStore setup, chart factories.

use mlc_core::tag_manager::{BubbleStore, BubbleId};
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
