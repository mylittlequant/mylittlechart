//! mlc-app wasm smoke: prove that the bridge → BackendCore → ChartApp::tick →
//! bubble.bars.last().close path tickets through end-to-end in a headless
//! browser, WITHOUT touching the canvas / render path.
//!
//! Layers under test (one layer beyond `wasm_bridge_smoke`):
//!
//!   browser WS -> dig3-station Station
//!     -> mlc_live_data::DataBridge (LiveUpdate::TradeUpdate / BarUpdate)
//!     -> mlc_backend_core::BackendCore::tick_writers (BubbleEvent)
//!     -> ChartApp::react_to_bubble_event (TradeUpdatePassthrough / BarUpdatePassthrough)
//!     -> bubble.bars.last_bar_mut().{close,volume,high,low} = ...
//!
//! The web shell drives this exact same `chart.tick(now_ms)` from its RAF
//! loop (mlc-app-web/src/lib.rs). If that loop is broken on wasm, the
//! `last_close` here would not advance either — same code path.
//!
//! ## Why 1m, not 1H
//!
//! On 1H a single candle aggregates **3600 seconds** of trades. The bubble's
//! `last.close` is overwritten on every Trade, but to assert it MOVES you
//! still need ≥1 real trade reach react.rs — and the wall-clock pace of
//! 1H bar creation is far slower than the test budget. 1m gives:
//!   - frequent close edits within the current minute (~1-10/s on BTCUSDT)
//!   - a fresh bar appears every 60s of wall-clock; reachable inside a test
//!
//! ## Scroll-backfill trigger
//!
//! Once steady-state is reached, push `viewport.view_start` far to the LEFT
//! (negative) — `ChartApp::tick` runs the scroll-fetch trigger block that
//! emits `bridge.request_scroll_bars` when `view_start` < `threshold`. We
//! assert a `ScrollBarsAppended` event arrives on the probe channel.
//!
//! Run:
//!   cargo test --target wasm32-unknown-unknown \
//!     -p mylittlechart-app --test wasm_chart_app_smoke \
//!     -- --nocapture

#![cfg(target_arch = "wasm32")]

use std::sync::Arc;
use std::time::Duration;

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use digdigdig3::core::types::{AccountType, ExchangeId};
use mlc_app::ChartApp;
use mlc_core::state::Timeframe;
use mlc_live_data::{DataBridge, LiveUpdate};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn read_active_last_close(app: &ChartApp) -> f64 {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c,
        None => return 0.0,
    };
    let bid = match app.bubble_for_chart(active_cid) {
        Some(b) => b,
        None => return 0.0,
    };
    let mgr = app.core.bubble();
    mgr.bubble
        .bubble(bid)
        .and_then(|b| b.bars.last().map(|x| x.close))
        .unwrap_or(0.0)
}

fn read_active_bars_len(app: &ChartApp) -> usize {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c,
        None => return 0,
    };
    let bid = match app.bubble_for_chart(active_cid) {
        Some(b) => b,
        None => return 0,
    };
    let mgr = app.core.bubble();
    mgr.bubble.bubble(bid).map(|b| b.bars.len()).unwrap_or(0)
}

fn read_active_last_ts(app: &ChartApp) -> i64 {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c,
        None => return 0,
    };
    let bid = match app.bubble_for_chart(active_cid) {
        Some(b) => b,
        None => return 0,
    };
    let mgr = app.core.bubble();
    mgr.bubble
        .bubble(bid)
        .and_then(|b| b.bars.last().map(|x| x.timestamp))
        .unwrap_or(0)
}

fn now_ms() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Use `Timeframe::m1()` — 60_000 ms — never the wrong-by-x1000 form
/// `Timeframe::new("1m", 60)`, which makes `period_secs = 0` and silently
/// disables the whole TradeUpdatePassthrough aggregation block in react.rs.
fn tf_m1() -> Timeframe { Timeframe::m1() }

// ─── Test 1: bridge → bubble live trade flow on 1m ───────────────────────────

/// 60-second sustained-flow test on 1m bars. After backfill settles we tick
/// at ~60 Hz and assert:
///   - bubble.last_close changes ≥ 30 times (real trades writing through),
///   - bubble.last_ts advances by at least one 60-sec boundary (a brand-new
///     1m bar appeared while we watched).
///
/// Side-channel `probe_rx` (a separate broadcast listener) counts TradeUpdate
/// / BarUpdate emitted by the bridge IN PARALLEL with what BackendCore is
/// consuming. If the probe sees thousands of TradeUpdate but our distinct-
/// close count stays at 1, the leak is in tick_writers / react, not bridge.
#[wasm_bindgen_test]
async fn chart_app_tick_sustains_trade_flow_1m() {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let mut probe_rx = bridge.add_listener();

    let mut app = ChartApp::new_empty(bridge.clone());
    app.seed_default_chart(
        ExchangeId::Binance,
        "BTCUSDT",
        tf_m1(),
        AccountType::Spot,
        500,
    );

    // ── Phase A: settle ──────────────────────────────────────────────────────
    let phase_a_deadline = web_time::Instant::now() + Duration::from_secs(45);
    let mut last_bars_len = 0;
    let mut stable_since = web_time::Instant::now();
    loop {
        app.tick(now_ms());
        let n = read_active_bars_len(&app);
        if n != last_bars_len {
            last_bars_len = n;
            stable_since = web_time::Instant::now();
        } else if n > 0 && stable_since.elapsed() >= Duration::from_millis(1500) {
            web_sys::console::log_1(
                &format!("[smoke-1m] phase A settled: bars={n}").into(),
            );
            break;
        }
        assert!(
            web_time::Instant::now() < phase_a_deadline,
            "phase A never settled within 45s"
        );
        gloo_timers::future::sleep(Duration::from_millis(30)).await;
    }

    let start_ts = read_active_last_ts(&app);
    let start_close = read_active_last_close(&app);

    // ── Phase B: 60s flow at ~60Hz ───────────────────────────────────────────
    use std::collections::BTreeSet;
    let mut distinct_closes: BTreeSet<u64> = BTreeSet::new();
    distinct_closes.insert(start_close.to_bits());

    let mut tick_count: u32 = 0;
    let (mut p_trade, mut p_bar, mut p_bars_loaded, mut p_backfill, mut p_other)
        = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut last_log = web_time::Instant::now();
    let flow_deadline = web_time::Instant::now() + Duration::from_secs(60);
    while web_time::Instant::now() < flow_deadline {
        app.tick(now_ms());
        tick_count += 1;
        loop {
            match probe_rx.try_recv() {
                Ok(LiveUpdate::TradeUpdate { .. }) => p_trade += 1,
                Ok(LiveUpdate::BarUpdate { .. }) => p_bar += 1,
                Ok(LiveUpdate::BarsLoaded { .. }) => p_bars_loaded += 1,
                Ok(LiveUpdate::BackfillComplete { .. }) => p_backfill += 1,
                Ok(_) => p_other += 1,
                Err(_) => break,
            }
        }
        distinct_closes.insert(read_active_last_close(&app).to_bits());
        if last_log.elapsed() >= Duration::from_secs(5) {
            web_sys::console::log_1(
                &format!(
                    "[smoke-1m] tick={} distinct={} bars={} last_ts={} last_close={} | \
                     probe trade={} bar={} bl={} bf={} other={}",
                    tick_count,
                    distinct_closes.len(),
                    read_active_bars_len(&app),
                    read_active_last_ts(&app),
                    read_active_last_close(&app),
                    p_trade, p_bar, p_bars_loaded, p_backfill, p_other,
                )
                .into(),
            );
            last_log = web_time::Instant::now();
        }
        gloo_timers::future::sleep(Duration::from_millis(16)).await;
    }

    let end_ts = read_active_last_ts(&app);
    let end_bars = read_active_bars_len(&app);
    web_sys::console::log_1(
        &format!(
            "[smoke-1m] DONE ticks={} distinct={} bars_end={} \
             start_ts={} end_ts={} dt_secs={} | probe trade={} bar={}",
            tick_count,
            distinct_closes.len(),
            end_bars,
            start_ts,
            end_ts,
            end_ts - start_ts,
            p_trade,
            p_bar,
        )
        .into(),
    );

    assert!(
        end_ts >= start_ts,
        "last_ts went backwards: {} -> {}",
        start_ts, end_ts
    );
    // Real trade flow proves the in-process write path works.
    assert!(
        distinct_closes.len() >= 30,
        "expected ≥30 distinct last_close values in 60s @1m; got {} \
         (ticks={}, probe trade={}) — bridge→bubble passthrough is broken \
         even though bridge emits trades",
        distinct_closes.len(), tick_count, p_trade
    );
    // The 1m bar boundary must cross at least once in 60s of wall-clock.
    assert!(
        end_ts > start_ts,
        "no new 1m bar appeared in 60s of wall-clock; last_ts frozen at {} \
         (BarUpdatePassthrough silent — react.rs TradeUpdatePassthrough \
         must roll candles when trade_ts_secs >= candle_end)",
        start_ts
    );
}

// ─── Test 2: scroll-backfill on viewport shift ───────────────────────────────

/// Simulate "user drags the chart far to the left" by mutating
/// `viewport.view_start` past the threshold. `ChartApp::tick` runs the
/// scroll-fetch trigger block that fires `bridge.request_scroll_bars`.
/// Assert at least one `LiveUpdate::ScrollBarsLoaded` arrives on probe.
#[wasm_bindgen_test]
async fn chart_app_scroll_backfill_triggers_on_view_shift() {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let mut probe_rx = bridge.add_listener();

    let mut app = ChartApp::new_empty(bridge.clone());
    app.seed_default_chart(
        ExchangeId::Binance,
        "BTCUSDT",
        tf_m1(),
        AccountType::Spot,
        500,
    );

    // Wait for initial 500 bars to land + backfill to expand to ~2000 so
    // there's actual recent state to scroll left from.
    let settle_deadline = web_time::Instant::now() + Duration::from_secs(45);
    let mut last_bars_len = 0;
    let mut stable_since = web_time::Instant::now();
    loop {
        app.tick(now_ms());
        let n = read_active_bars_len(&app);
        if n != last_bars_len {
            last_bars_len = n;
            stable_since = web_time::Instant::now();
        } else if n >= 500 && stable_since.elapsed() >= Duration::from_millis(2000) {
            break;
        }
        assert!(
            web_time::Instant::now() < settle_deadline,
            "settle never finished"
        );
        gloo_timers::future::sleep(Duration::from_millis(30)).await;
    }
    let bars_before = read_active_bars_len(&app);

    // Drain everything the probe accumulated during phase A so we don't
    // count old BarsLoaded / BackfillComplete events as scroll-backfill.
    while let Ok(_) = probe_rx.try_recv() {}

    // ── Force chart_width + bar_spacing to a non-zero state so the
    //    scroll-fetch math in tick.rs doesn't bail early on uninitialized
    //    viewport (resize() is the normal path; we skip render). Then jam
    //    view_start to 0.0 (full-left) so the next tick crosses the 20%
    //    visibility threshold.
    let active_cid = app
        .panel_app
        .panel_grid
        .active_container_id()
        .expect("active container");
    {
        let win = app
            .panel_app
            .panel_grid
            .containers_mut()
            .get_mut(&active_cid)
            .expect("active window");
        if win.viewport.chart_width <= 0.0 {
            win.viewport.chart_width = 800.0;
        }
        if win.viewport.bar_spacing <= 0.0 {
            win.viewport.bar_spacing = 8.0;
        }
        win.viewport.view_start = 0.0;
        web_sys::console::log_1(
            &format!(
                "[smoke-scroll] forced viewport: cw={} bs={} vs={} bars={}",
                win.viewport.chart_width,
                win.viewport.bar_spacing,
                win.viewport.view_start,
                bars_before
            )
            .into(),
        );
    }

    // Wait for ScrollBarsLoaded on the probe channel.
    let scroll_deadline = web_time::Instant::now() + Duration::from_secs(30);
    let mut got_scroll = false;
    let mut got_bars_loaded_after = 0u32;
    let mut total_drained = 0u32;
    while web_time::Instant::now() < scroll_deadline {
        app.tick(now_ms());
        loop {
            match probe_rx.try_recv() {
                Ok(LiveUpdate::ScrollBarsLoaded { .. }) => {
                    got_scroll = true;
                    web_sys::console::log_1(
                        &"[smoke-scroll] ScrollBarsLoaded arrived".into(),
                    );
                }
                Ok(LiveUpdate::BarsLoaded { .. }) => got_bars_loaded_after += 1,
                Ok(_) => {}
                Err(_) => break,
            }
            total_drained += 1;
        }
        if got_scroll {
            break;
        }
        gloo_timers::future::sleep(Duration::from_millis(50)).await;
    }

    let bars_after = read_active_bars_len(&app);
    web_sys::console::log_1(
        &format!(
            "[smoke-scroll] DONE got_scroll={} bars_before={} bars_after={} \
             extra_bars_loaded={} probe_drains={}",
            got_scroll, bars_before, bars_after, got_bars_loaded_after, total_drained,
        )
        .into(),
    );

    assert!(
        got_scroll,
        "viewport.view_start=0 did NOT trigger any LiveUpdate::ScrollBarsLoaded \
         within 30s — bridge.request_scroll_bars never fired, or station \
         scroll-fetch silent. Check tick.rs scroll-fetch block on wasm."
    );

    // After the merge fix in backend-core, the scroll page must STRICTLY GROW
    // the bubble (it brings older bars; the right-edge stays untouched).
    assert!(
        bars_after > bars_before,
        "scroll-backfill did not grow bubble: bars_before={} bars_after={} \
         — backend-core ScrollBarsLoaded merge regressed (the older page is \
         overwriting the live tail again)",
        bars_before, bars_after
    );
}
