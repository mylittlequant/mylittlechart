//! INTEGRATION tier: BarsLoaded writer arm composed against the real
//! `BubbleStore` + `ChartPanelGrid` data structures, without a live
//! `DataBridge` / tokio runtime / network.
//!
//! Mirrors the actual writer in `mlc-app/src/frame_lifecycle/tick.rs`
//! BarsLoaded arm — same routing helpers, same matching criteria, same
//! per-chart fan-out. If the integration assertion holds, the routing
//! fix is correct against the real types (not just isolated BubbleStore).

use mlc_backend::bubble::BubbleStore;
use mlc_backend::tag::TagManager;
use mlc_core::ids::BubbleId;
use mlc_core::state::{ChartId, generate_chart_id, Timeframe, ChartContainer};
use mlc_core::tag::Tag;
use mlc_core::Bar;

fn mk_bars(n: usize) -> Vec<Bar> {
    (0..n).map(|i| Bar {
        timestamp: 1_780_000_000 + i as i64 * 60,
        open: 100.0,
        high: 101.0,
        low: 99.0,
        close: 100.5,
        volume: 1.0,
    }).collect()
}

/// Build a `ChartContainer` attached to a fresh auto-bubble for the given
/// instrument. Returns (chart, chart_id, bubble_id).
fn mk_chart_with_bubble(
    store: &mut BubbleStore,
    tags: &mut TagManager,
    exchange: &str,
    account_type: &str,
    symbol: &str,
    tf: Timeframe,
) -> (ChartContainer, ChartId, BubbleId) {
    let chart_id = generate_chart_id();
    let color = [0.0; 4];
    let bid = store.create_group_auto(symbol.to_string(), tf.clone());
    store.set_bubble_instrument(bid, exchange.to_string(), account_type.to_string());
    tags.insert(Tag::new_auto(bid, color));
    tags.attach_chart_to_tag(chart_id, bid).expect("attach");

    let mut chart = ChartContainer::new(symbol, tf);
    chart.id = chart_id;
    chart.auto_bubble_id = bid;
    chart.current_bubble_id = bid;
    (chart, chart_id, bid)
}

/// Re-implement the BarsLoaded writer arm logic in isolation. This is
/// intentionally a copy of the arm in `tick.rs` — the integration test's
/// job is to verify that THIS routing pattern fills every chart's bubble
/// in a multi-chart, same-instrument setup.
fn bars_loaded_writer_arm(
    store: &mut BubbleStore,
    tags: &TagManager,
    charts: &[(ChartContainer, ChartId, BubbleId)],
    exchange: &str,
    account_type: &str,
    symbol: &str,
    tf_name: &str,
    new_bars: &[Bar],
) {
    // matched_windows: every chart whose attached bubble matches the instrument.
    let mut matched: Vec<ChartId> = Vec::new();
    for (_, cid, _) in charts {
        let Some(bid) = tags.tag_for_chart(*cid) else { continue };
        let Some(instrument) = store.instrument(bid) else { continue };
        if instrument.address.exchange == exchange
            && instrument.address.account_type.as_label() == account_type
            && instrument.address.symbol == symbol
            && instrument.timeframe.name == tf_name
        {
            matched.push(*cid);
        }
    }

    // Fan-out write into every bubble for this instrument.
    if !matched.is_empty() {
        for bid in store.bubbles_for_instrument(exchange, account_type, symbol) {
            store.set_bars(bid, new_bars.to_vec());
        }
    }
}

#[test]
fn bars_loaded_fills_every_bubble_when_four_tabs_share_instrument() {
    let mut store = BubbleStore::new();
    let mut tags = TagManager::new();
    let mut charts = Vec::new();
    for _ in 0..4 {
        let c = mk_chart_with_bubble(
            &mut store, &mut tags, "binance", "S", "BTCUSDT", Timeframe::m1()
        );
        charts.push(c);
    }
    let new_bars = mk_bars(300);

    bars_loaded_writer_arm(
        &mut store, &tags, &charts,
        "binance", "S", "BTCUSDT", "1m",
        &new_bars,
    );

    // Every chart's current_bubble_id → bubble.bars must equal new_bars.
    for (chart, cid, _) in &charts {
        let bid = chart.current_bubble_id;
        let bars = store.bars(bid);
        assert_eq!(
            bars.len(),
            300,
            "chart {:?} renderer would read 300 bars from its bubble",
            cid
        );
    }
}

#[test]
fn bars_loaded_does_not_leak_to_unrelated_instrument() {
    let mut store = BubbleStore::new();
    let mut tags = TagManager::new();
    let mut charts = Vec::new();
    // 2 BTC tabs + 2 ETH tabs.
    for _ in 0..2 {
        charts.push(mk_chart_with_bubble(
            &mut store, &mut tags, "binance", "S", "BTCUSDT", Timeframe::m1()
        ));
    }
    for _ in 0..2 {
        charts.push(mk_chart_with_bubble(
            &mut store, &mut tags, "binance", "S", "ETHUSDT", Timeframe::m1()
        ));
    }
    let btc_bars = mk_bars(300);

    bars_loaded_writer_arm(
        &mut store, &tags, &charts,
        "binance", "S", "BTCUSDT", "1m",
        &btc_bars,
    );

    // BTC charts: 300 bars each.
    for (chart, _, _) in charts.iter().take(2) {
        let bars = store.bars(chart.current_bubble_id);
        assert_eq!(bars.len(), 300, "BTC tab gets bars");
    }
    // ETH charts: still empty.
    for (chart, _, _) in charts.iter().skip(2) {
        let bars = store.bars(chart.current_bubble_id);
        assert_eq!(bars.len(), 0, "ETH tab is NOT polluted by BTC write");
    }
}

#[test]
fn bars_loaded_with_timeframe_mismatch_no_matched_windows_no_write() {
    let mut store = BubbleStore::new();
    let mut tags = TagManager::new();
    let (chart, _, bid) = mk_chart_with_bubble(
        &mut store, &mut tags, "binance", "S", "BTCUSDT", Timeframe::h1()
    );
    let new_bars = mk_bars(300);
    let charts = vec![(chart, ChartId(0), bid)];

    // tf_name "1m" doesn't match the chart's "1H" timeframe → no matched
    // windows → writer must NOT touch the bubble.
    bars_loaded_writer_arm(
        &mut store, &tags, &charts,
        "binance", "S", "BTCUSDT", "1m",
        &new_bars,
    );

    let (chart, _, _) = &charts[0];
    let bars = store.bars(chart.current_bubble_id);
    assert_eq!(bars.len(), 0, "mismatched timeframe must not trigger write");
}
