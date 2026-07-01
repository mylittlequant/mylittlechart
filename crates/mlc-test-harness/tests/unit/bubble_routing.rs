//! UNIT tier: BubbleStore routing logic in isolation.
//!
//! Reproduces the live spinner symptom (chart stays on "loading" because
//! its bubble.bars is empty after BarsLoaded) at the level of the routing
//! decision alone — no ChartApp, no tick(), no bridge.

use mlc_core::ids::BubbleId;
use mlc_core::state::Timeframe;
use mlc_test_harness::fixtures::{n_charts_same_instrument, mk_bars, chart_with_phantom_instrument_bubble};

#[test]
fn bubbles_for_instrument_returns_all_matching_bubbles() {
    // The plural helper is the writer's primary tool: it returns every
    // bubble that matches (exchange, account_type, symbol), so the writer
    // can iterate and fill all of them in one pass.
    let (store, charts) = n_charts_same_instrument(
        4, "binance", "S", "BTCUSDT", Timeframe::m1()
    );
    let found = store.bubbles_for_instrument("binance", "S", "BTCUSDT");
    assert_eq!(found.len(), 4, "all 4 charts' bubbles are returned");

    // No false positives on a different instrument.
    let none = store.bubbles_for_instrument("binance", "S", "ETHUSDT");
    assert!(none.is_empty(), "empty for unrelated symbol");

    // Sanity: every chart's own bubble appears in the returned set.
    let returned: std::collections::HashSet<BubbleId> = found.into_iter().collect();
    for (_, bid) in &charts {
        assert!(returned.contains(bid));
    }
}

#[test]
fn bubble_for_instrument_picks_only_first_when_many_charts_share_instrument() {
    // Setup: 4 tabs, all showing binance Spot BTCUSDT 1m. Each has its own
    // auto-bubble (the live default when nothing explicitly tags them).
    let (store, charts) = n_charts_same_instrument(
        4, "binance", "S", "BTCUSDT", Timeframe::m1()
    );

    // bubble_for_instrument scans bubbles and returns the FIRST match.
    let found = store
        .bubble_for_instrument("binance", "S", "BTCUSDT")
        .expect("should find at least one");

    let all_bubble_ids: std::collections::HashSet<BubbleId> =
        charts.iter().map(|(_, b)| *b).collect();
    assert!(all_bubble_ids.contains(&found));
    assert_eq!(all_bubble_ids.len(), 4, "should have 4 distinct auto-bubbles");
}

#[test]
fn old_writer_pattern_leaves_3_of_4_bubbles_empty() {
    // Old pattern (the one currently shipped on `main` and matching the
    // live bug): write to the SINGLE bubble returned by bubble_for_instrument.
    let (mut store, charts) = n_charts_same_instrument(
        4, "binance", "S", "BTCUSDT", Timeframe::m1()
    );
    let new_bars = mk_bars(300);

    let bid = store
        .bubble_for_instrument("binance", "S", "BTCUSDT")
        .expect("at least one bubble");
    store.set_bars(bid, new_bars.clone());

    let mut filled = 0;
    let mut empty = 0;
    for (_, bubble_id) in &charts {
        let bars_len = store.bars(*bubble_id).len();
        if bars_len == 300 { filled += 1; } else { empty += 1; }
    }
    assert_eq!(filled, 1, "old writer fills exactly one bubble");
    assert_eq!(empty, 3, "old writer leaves the other three empty — this is the bug");
}

#[test]
fn new_writer_pattern_fills_all_attached_bubbles() {
    // New pattern: for every matched chart_id, resolve its own bubble via
    // bubble_for_chart and write into ALL of them.
    let (mut store, charts) = n_charts_same_instrument(
        4, "binance", "S", "BTCUSDT", Timeframe::m1()
    );
    let new_bars = mk_bars(300);

    let target_bubbles: std::collections::HashSet<BubbleId> = charts.iter()
        .filter_map(|(cid, _)| store.bubble_for_chart(*cid))
        .collect();
    assert_eq!(target_bubbles.len(), 4, "must resolve all 4 bubbles, not just one");

    for bid in target_bubbles {
        store.set_bars(bid, new_bars.clone());
    }

    for (_, bubble_id) in &charts {
        let bars_len = store.bars(*bubble_id).len();
        assert_eq!(bars_len, 300, "every chart's own bubble has bars now");
    }
}

#[test]
fn live_bug_repro_writer_via_instrument_misses_chart_bubble() {
    // Live bug repro: chart is attached to a bubble whose instrument fields
    // are unset; a SEPARATE phantom bubble holds the matching instrument.
    // Writer routed via `bubble_for_instrument` writes into the phantom and
    // the chart's renderer (which reads via current_bubble_id) sees zero bars.
    let (mut store, chart_id, attached_bid, phantom_bid) =
        chart_with_phantom_instrument_bubble("binance", "S", "BTCUSDT", Timeframe::m1());
    let new_bars = mk_bars(300);

    // OLD pattern: write via bubble_for_instrument.
    let bid = store.bubble_for_instrument("binance", "S", "BTCUSDT").expect("phantom");
    assert_eq!(bid, phantom_bid, "instrument lookup returns the phantom, not the chart's bubble");
    store.set_bars(bid, new_bars.clone());

    // Renderer reads via bubble_for_chart — still empty.
    let renderer_bid = store.bubble_for_chart(chart_id).expect("chart attached");
    assert_eq!(renderer_bid, attached_bid);
    assert!(store.bars(renderer_bid).is_empty(), "spinner repro");
}

#[test]
fn new_writer_via_bubble_for_chart_fills_renderer_bubble() {
    // Same setup, but the new writer routes via bubble_for_chart(matched_cid).
    let (mut store, chart_id, attached_bid, _phantom_bid) =
        chart_with_phantom_instrument_bubble("binance", "S", "BTCUSDT", Timeframe::m1());
    let new_bars = mk_bars(300);

    // NEW pattern: write directly to the chart's attached bubble.
    let bid = store.bubble_for_chart(chart_id).expect("chart attached");
    assert_eq!(bid, attached_bid);
    store.set_bars(bid, new_bars.clone());

    // Renderer reads via current_bubble_id (same path) — populated.
    let renderer_bid = store.bubble_for_chart(chart_id).expect("chart attached");
    let bars = store.bars(renderer_bid);
    assert_eq!(bars.len(), 300, "spinner retires");
}

#[test]
fn renderer_read_path_after_new_writer() {
    // Renderer reads bars via window.current_bubble_id (here simulated as
    // bubble_for_chart). After the new writer, every chart's read sees bars.
    let (mut store, charts) = n_charts_same_instrument(
        4, "binance", "S", "BTCUSDT", Timeframe::m1()
    );
    let new_bars = mk_bars(300);

    let target_bubbles: std::collections::HashSet<BubbleId> = charts.iter()
        .filter_map(|(cid, _)| store.bubble_for_chart(*cid))
        .collect();
    for bid in target_bubbles {
        store.set_bars(bid, new_bars.clone());
    }

    for (cid, _) in &charts {
        let bid = store.bubble_for_chart(*cid).expect("chart attached");
        let bars = store.bars(bid);
        assert_eq!(bars.len(), 300, "renderer sees 300 bars for chart {:?}", cid);
        assert!(!bars.is_empty(), "renderer's is_empty() check → spinner retires");
    }
}
