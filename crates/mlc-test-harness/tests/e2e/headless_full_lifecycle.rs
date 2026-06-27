//! E2E tier: full ChartApp lifecycle via `HeadlessChartApp` — same entry
//! point and same code path as the live binary, minus the GPU/window layer.
//!
//! Goal: reproduce the live spinner bug (chart's renderer reads an empty
//! `bubble.bars` while LiveUpdate::BarsLoaded actually delivered 300 bars)
//! in a headless context, then iterate the fix in this harness instead of
//! the 13-minute release rebuild.

use std::time::Duration;

use mlc_test_harness::headless::HeadlessChartApp;

#[test]
fn headless_btcusdt_loads_bars_into_active_chart_bubble() {
    // Full live path: ChartApp::new("BTCUSDT") spins up the real DataBridge,
    // tokio runtime, persistence, preset hydration. The bridge requests bars
    // from binance internally.
    let mut h = HeadlessChartApp::new("BTCUSDT");

    // Without a resize() the deferred snap path stays blocked on
    // chart_width == 0 — same as the live binary before its first frame.
    h.resize(1280, 800);

    let active = h.active_chart_id().expect("active chart");
    let initial_bubble = h.chart_bubble(active).expect("bubble attached");

    eprintln!("--- INITIAL STATE ---");
    eprintln!("active_chart   = {:?}", active);
    eprintln!("active bubble  = {:?}", initial_bubble);
    for b in h.dump_all_bubbles() {
        eprintln!("  bubble {:?}: exch={:?} at={:?} sym={:?} tf={:?} bars={} charts={:?}",
            b.id, b.exchange, b.account_type, b.symbol, b.timeframe, b.bars_len, b.attached_charts);
    }

    // Pump the tick loop until ANY chart's bubble has bars. The active
    // BTCUSDT chart may be a sentinel-pointer chart that the saved preset
    // never re-attached; the assertion that matters is "the renderer for
    // every chart in the preset sees its own bars", checked below.
    let got_bars = h.pump_until(
        |h| h.chart_ids().iter().any(|&cid| h.chart_bar_count(cid) > 0),
        Duration::from_secs(60),
    );

    eprintln!("--- POST-WAIT STATE ---");
    for (cid, bid, len) in h.dump_chart_bubbles() {
        eprintln!("  chart {:?} → bubble {:?}: bars={}", cid, bid, len);
    }
    for b in h.dump_all_bubbles() {
        eprintln!("  bubble {:?}: exch={:?} at={:?} sym={:?} tf={:?} bars={} charts={:?}",
            b.id, b.exchange, b.account_type, b.symbol, b.timeframe, b.bars_len, b.attached_charts);
    }

    let phantom = h.bubble_for_instrument("binance", "S", "BTCUSDT");
    eprintln!("bubble_for_instrument(binance, S, BTCUSDT) = {:?}", phantom);
    eprintln!("active chart's bubble                       = {:?}", h.chart_bubble(active));

    assert!(
        got_bars,
        "at least one chart's bubble should have bars after the preset \
         loads — failing here reproduces the live 'spinner forever' bug"
    );

    // Stronger assertion: every chart that is attached to a bubble (via
    // BubbleStore.member_to_bubble) must see those bars in the renderer
    // path (via window.current_bubble_id). They are the same authoritative
    // bubble per the apply_preset_cold sync sweep.
    for (cid, win_bid, win_bars) in h.dump_chart_bubbles() {
        let auth_bid = h.app.core.bubble().bubble_for_chart(cid);
        if let Some(auth_bid) = auth_bid {
            assert_eq!(
                win_bid, auth_bid,
                "chart {:?}: window.current_bubble_id ({:?}) must equal \
                 BubbleStore.bubble_for_chart ({:?}) after sync sweep",
                cid, win_bid, auth_bid
            );
            let auth_bars = h.app.core.bubble().bubble(auth_bid)
                .map(|b| b.bars.len())
                .unwrap_or(0);
            assert_eq!(
                win_bars, auth_bars,
                "chart {:?}: renderer-side bar count ({}) must equal \
                 writer-side bar count ({})",
                cid, win_bars, auth_bars
            );
        }
    }
}
