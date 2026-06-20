//! E2E tier: drive the REAL `ChartApp::tick()` loop with a synthetic
//! `LiveUpdate::BarsLoaded`, then assert that every chart's bubble has
//! `bars.len() == 300`.
//!
//! Uses `DataBridge::debug_broadcast_tx()` to inject the event without a
//! network connection. Spins up the bridge's tokio runtime, but never
//! requests bars / connectors — purely passive routing test.
//!
//! NOT a substitute for the live `mylittlechart.exe` (see
//! `feedback_unit_tests_for_debug_only`). Owner-facing "it works" still
//! requires the live binary on `main`.

use std::sync::Arc;
use std::time::Duration;

use mlc_app::ChartApp;
use mlc_core::Bar;
use mlc_core::state::{generate_chart_id, Timeframe, ChartContainer};
use mlc_live_data::{DataBridge, LiveUpdate};
use digdigdig3::{ExchangeId, AccountType};

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

/// Add `count` charts to `app`, each in its own auto-bubble for the given
/// instrument. Returns the list of chart IDs that were added (so the test
/// can ignore the initial chart that `ChartApp::new_empty` created with no
/// attached bubble — its `current_bubble_id` is the sentinel `BubbleId(0)`).
fn add_charts(app: &mut ChartApp, count: usize, symbol: &str, exchange: &str, tf: Timeframe) -> Vec<mlc_core::state::ChartId> {
    let mut out = Vec::new();
    for _ in 0..count {
        let chart_id = generate_chart_id();
        let bid = app.core.bubble_mut().bubble.create_group_auto(
            [0.0; 4], symbol.to_string(), tf.clone(),
        );
        if let Some(b) = app.core.bubble_mut().bubble.bubble_mut(bid) {
            b.exchange = exchange.to_string();
            b.account_type = "S".to_string();
        }
        let _ = app.core.bubble_mut().bubble.attach_chart_to_bubble(chart_id, bid);

        let mut chart = ChartContainer::new(symbol, tf.clone());
        chart.id = chart_id;
        chart.auto_bubble_id = bid;
        chart.current_bubble_id = bid;
        app.panel_app.panel_grid.containers_mut().insert(chart_id, chart);
        out.push(chart_id);
    }
    out
}

/// Live-bug repro: insert a `phantom` auto-bubble that holds matching
/// instrument identity but has no chart attached, plus a chart in its own
/// (separate) auto-bubble for the same instrument. Mirrors the runtime
/// state the diag logs showed (chart attached to BubbleId(94) while
/// `bubble_for_instrument` returns BubbleId(70)).
fn add_chart_plus_phantom(
    app: &mut ChartApp,
    symbol: &str,
    exchange: &str,
    tf: Timeframe,
) -> mlc_core::state::ChartId {
    // Phantom — no chart attached.
    let phantom_bid = app.core.bubble_mut().bubble.create_group_auto(
        [0.0; 4], symbol.to_string(), tf.clone(),
    );
    if let Some(b) = app.core.bubble_mut().bubble.bubble_mut(phantom_bid) {
        b.exchange = exchange.to_string();
        b.account_type = "S".to_string();
    }

    // Real chart in its own auto-bubble, also carrying matching instrument.
    let cids = add_charts(app, 1, symbol, exchange, tf);
    cids[0]
}

#[test]
fn four_tabs_same_instrument_all_bubbles_get_bars_after_tick() {
    // 1. Spin up a real DataBridge (real tokio runtime, no network requests).
    let (bridge, _initial_rx, _connector_ready_rx) = DataBridge::new();
    let bridge = Arc::new(bridge);
    let tx = bridge.debug_broadcast_tx();

    // 2. Build a ChartApp on this bridge. new_empty does NOT preload a chart,
    //    so we add the 4 BTCUSDT 1m Spot charts manually to mirror the live
    //    "4 tabs same instrument" symptom.
    let mut app = ChartApp::new_empty(bridge.clone());
    let chart_ids = add_charts(&mut app, 4, "BTCUSDT", "binance", Timeframe::m1());
    assert_eq!(chart_ids.len(), 4, "expected 4 attached BTCUSDT charts");

    // 3. Inject a synthetic BarsLoaded carrying 300 bars.
    let bars = mk_bars(300);
    tx.send(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1m".to_string(),
        bars: bars.clone(),
    }).expect("broadcast send");

    // Give broadcast a moment to land in the receiver.
    std::thread::sleep(Duration::from_millis(50));

    // 4. Tick the app. The BarsLoaded writer arm should fan-out to every
    //    bubble whose instrument matches.
    app.tick(0);

    // 5. Every chart's renderer reads bars via its current_bubble_id. After
    //    the new (plural) writer, all four bubbles must carry 300 bars.
    let mut empty: Vec<mlc_core::state::ChartId> = Vec::new();
    let mut filled = 0;
    for cid in &chart_ids {
        let win = app.panel_app.panel_grid.containers().get(cid).expect("chart");
        let bubble_bars = app.core.bubble_mut().bubble.bubble(win.current_bubble_id)
            .map(|b| b.bars.len())
            .unwrap_or(0);
        if bubble_bars == 300 { filled += 1; } else { empty.push(*cid); }
    }
    assert!(empty.is_empty(), "no bubble should remain empty — bug repro left {:?} empty", empty);
    assert_eq!(filled, 4, "all 4 bubbles must be filled after the fan-out write");
}

#[test]
fn live_bug_repro_chart_with_phantom_bubble_still_gets_bars() {
    // This is the EXACT runtime shape the diag dump exposed: a chart attached
    // to one auto-bubble, plus a separate phantom bubble holding the matching
    // instrument identity. The old singular `bubble_for_instrument` path
    // would write into the phantom and leave the renderer's bubble empty.
    let (bridge, _initial_rx, _connector_ready_rx) = DataBridge::new();
    let bridge = Arc::new(bridge);
    let tx = bridge.debug_broadcast_tx();

    let mut app = ChartApp::new_empty(bridge.clone());
    let chart_id = add_chart_plus_phantom(&mut app, "BTCUSDT", "binance", Timeframe::m1());

    let bars = mk_bars(300);
    tx.send(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1m".to_string(),
        bars: bars.clone(),
    }).expect("broadcast send");

    std::thread::sleep(Duration::from_millis(50));
    app.tick(0);

    let win = app.panel_app.panel_grid.containers().get(&chart_id).expect("chart");
    let renderer_bars = app.core.bubble_mut().bubble.bubble(win.current_bubble_id)
        .map(|b| b.bars.len())
        .unwrap_or(0);
    assert_eq!(renderer_bars, 300,
        "chart's renderer (which reads via current_bubble_id) must see 300 bars \
         — the phantom bubble must NOT swallow the write");
}

#[test]
fn bars_loaded_for_different_instrument_does_not_pollute_eth_tab() {
    let (bridge, _initial_rx, _connector_ready_rx) = DataBridge::new();
    let bridge = Arc::new(bridge);
    let tx = bridge.debug_broadcast_tx();

    let mut app = ChartApp::new_empty(bridge.clone());
    let btc_ids = add_charts(&mut app, 2, "BTCUSDT", "binance", Timeframe::m1());
    let eth_ids = add_charts(&mut app, 2, "ETHUSDT", "binance", Timeframe::m1());

    let btc_bars = mk_bars(300);
    tx.send(LiveUpdate::BarsLoaded {
        exchange_id: ExchangeId::Binance,
        account_type: AccountType::Spot,
        symbol: "BTCUSDT".to_string(),
        timeframe: "1m".to_string(),
        bars: btc_bars,
    }).expect("send");

    std::thread::sleep(Duration::from_millis(50));
    app.tick(0);

    let mut btc_filled = 0;
    let mut eth_filled = 0;
    for cid in &btc_ids {
        let win = app.panel_app.panel_grid.containers().get(cid).unwrap();
        let guard = app.core.bubble();
        let bubble = guard.bubble.bubble(win.current_bubble_id).unwrap();
        if bubble.bars.len() == 300 { btc_filled += 1; }
    }
    for cid in &eth_ids {
        let win = app.panel_app.panel_grid.containers().get(cid).unwrap();
        let guard = app.core.bubble();
        let bubble = guard.bubble.bubble(win.current_bubble_id).unwrap();
        if !bubble.bars.is_empty() { eth_filled += 1; }
    }
    assert_eq!(btc_filled, 2, "both BTC tabs filled");
    assert_eq!(eth_filled, 0, "ETH tabs not polluted by BTC BarsLoaded");
}
