//! mlc-app dual-target smoke: prove that the bridge → BackendCore →
//! `ChartApp::tick` → `bubble.bars.last().close` path tickets through end-
//! to-end on BOTH wasm32 (headless Chrome via dig2-wasm-test) AND native
//! (tokio multi-thread runtime), WITHOUT touching the canvas / render path.
//!
//! Layers under test (one layer beyond `bridge_smoke`):
//!
//!   browser/native WS -> dig3-station Station
//!     -> mlc_backend_data_bridge::DataBridge (LiveUpdate::TradeUpdate / BarUpdate)
//!     -> mlc_backend::BackendCore::tick_writers (BubbleEvent)
//!     -> ChartApp::react_to_bubble_event
//!     -> bubble.bars.last_bar_mut().{close,volume,high,low} = ...
//!
//! The web shell drives this exact `chart.tick(now_ms)` from its RAF
//! loop (mlc-app-shell-web/src/lib.rs); the desktop shell does the same.
//! If either is broken, `last_close` here would not advance — same path.
//!
//! Run:
//!   cargo test --target wasm32-unknown-unknown \
//!     -p mylittlechart-app --test chart_app_smoke -- --nocapture
//!   cargo test -p mylittlechart-app --test chart_app_smoke -- --nocapture

use std::sync::Arc;
use std::time::Duration;

use digdigdig3::core::types::{AccountType, ExchangeId};
use mlc_app::ChartApp;
use mlc_core::state::Timeframe;
use mlc_backend_data_bridge::{DataBridge, LiveUpdate};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test_configure;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

type Instant = web_time::Instant;

// ─── cross-platform helpers ─────────────────────────────────────────────────

async fn sleep(d: Duration) {
    #[cfg(target_arch = "wasm32")]
    { gloo_timers::future::sleep(d).await; }
    #[cfg(not(target_arch = "wasm32"))]
    { tokio::time::sleep(d).await; }
}

fn log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    { web_sys::console::log_1(&msg.into()); }
    #[cfg(not(target_arch = "wasm32"))]
    { eprintln!("{}", msg); }
}

// ─── ChartApp readers (target-agnostic) ─────────────────────────────────────

fn read_active_last_close(app: &ChartApp) -> f64 {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c, None => return 0.0,
    };
    let bid = match app.bubble_for_chart(active_cid) {
        Some(b) => b, None => return 0.0,
    };
    let mgr = app.core.bubbles();
    mgr.bars(bid).last().map(|x| x.close).unwrap_or(0.0)
}

fn read_active_bars_len(app: &ChartApp) -> usize {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c, None => return 0,
    };
    let bid = match app.bubble_for_chart(active_cid) {
        Some(b) => b, None => return 0,
    };
    let mgr = app.core.bubbles();
    mgr.bars(bid).len()
}

fn read_active_last_ts(app: &ChartApp) -> i64 {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c, None => return 0,
    };
    let bid = match app.bubble_for_chart(active_cid) {
        Some(b) => b, None => return 0,
    };
    let mgr = app.core.bubbles();
    mgr.bars(bid).last().map(|x| x.timestamp).unwrap_or(0)
}

/// `viewport.bar_count` — pure ChartApp state, ONLY updated by
/// `react_to_bubble_event` arms (BarUpdatePassthrough line 487,
/// TradeUpdatePassthrough line 619, BarsLoaded / BackfillCompleted /
/// ScrollBarsAppended branches). It is NOT updated by tick_writers / bubble
/// itself. If `bubble.bars.len()` grows but `viewport.bar_count` does not,
/// react.rs is silently dropping the event — that's the exact bug the wasm
/// shell hit before the bridge-substrate work.
fn read_active_viewport_bar_count(app: &ChartApp) -> usize {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c, None => return 0,
    };
    app.panel_app.panel_grid.containers().get(&active_cid)
        .map(|w| w.viewport.bar_count).unwrap_or(0)
}

/// `viewport.view_start` — also ONLY moved by react.rs side-effect arms.
/// Auto-scroll increments it by 1.0 every time a new bar is appended AND the
/// viewport was already pinned to the right edge (react.rs:502, 629). A
/// frozen `view_start` while `bubble.bars.len()` grows means
/// TradeUpdatePassthrough / BarUpdatePassthrough did not run.
fn read_active_view_start(app: &ChartApp) -> f64 {
    let active_cid = match app.panel_app.panel_grid.active_container_id() {
        Some(c) => c, None => return 0.0,
    };
    app.panel_app.panel_grid.containers().get(&active_cid)
        .map(|w| w.viewport.view_start).unwrap_or(0.0)
}

fn now_ms() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `Timeframe::m1()` — 60_000 ms — never the wrong-by-x1000 form
/// `Timeframe::new("1m", 60)`, which makes `period_secs = 0` and silently
/// disables the whole TradeUpdatePassthrough aggregation block in react.rs.
fn tf_m1() -> Timeframe { Timeframe::m1() }

// ─── Test 1: bridge → bubble live trade flow on 1m ───────────────────────────

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 4))]
async fn chart_app_tick_sustains_trade_flow_1m() {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let mut probe_rx = bridge.add_listener();

    let core = mlc_backend::BackendCore::new(bridge.clone(), None);
    let mut app = ChartApp::new_empty(core);
    app.seed_default_chart(
        ExchangeId::Binance, "BTCUSDT", tf_m1(), AccountType::Spot, 500,
    );

    // ── Phase A: settle ──
    let phase_a_deadline = Instant::now() + Duration::from_secs(45);
    let mut last_bars_len = 0;
    let mut stable_since = Instant::now();
    loop {
        app.tick(now_ms());
        let n = read_active_bars_len(&app);
        if n != last_bars_len {
            last_bars_len = n;
            stable_since = Instant::now();
        } else if n > 0 && stable_since.elapsed() >= Duration::from_millis(1500) {
            log(&format!("[smoke-1m] phase A settled: bars={n}"));
            break;
        }
        assert!(Instant::now() < phase_a_deadline, "phase A never settled within 45s");
        sleep(Duration::from_millis(30)).await;
    }

    // Baselines BEFORE flow — split by layer:
    //   bubble layer = bubble.bars.last().{close,timestamp}, bubble.bars.len()
    //   chart-app layer = viewport.bar_count, viewport.view_start
    let start_ts        = read_active_last_ts(&app);
    let start_close     = read_active_last_close(&app);
    let start_bubble_n  = read_active_bars_len(&app);
    let start_vp_count  = read_active_viewport_bar_count(&app);
    let start_vp_vstart = read_active_view_start(&app);

    // ── Phase B: 60s flow at ~60Hz ──
    use std::collections::BTreeSet;
    // Distinct samples of TWO observables, one per layer:
    let mut distinct_bubble_close: BTreeSet<u64> = BTreeSet::new();
    let mut distinct_vp_count:     BTreeSet<usize> = BTreeSet::new();
    distinct_bubble_close.insert(start_close.to_bits());
    distinct_vp_count.insert(start_vp_count);

    let mut tick_count: u32 = 0;
    let (mut p_trade, mut p_bar, mut p_bars_loaded, mut p_backfill, mut p_other)
        = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut last_log = Instant::now();
    let flow_deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < flow_deadline {
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
        distinct_bubble_close.insert(read_active_last_close(&app).to_bits());
        distinct_vp_count.insert(read_active_viewport_bar_count(&app));
        if last_log.elapsed() >= Duration::from_secs(5) {
            log(&format!(
                "[smoke-1m] tick={} bubble{{close_uniq={} bars={} last_ts={} last_close={}}} \
                 viewport{{count_uniq={} count={} vstart={:.3}}} probe{{trade={} bar={} bl={} bf={}}}",
                tick_count,
                distinct_bubble_close.len(), read_active_bars_len(&app),
                read_active_last_ts(&app), read_active_last_close(&app),
                distinct_vp_count.len(), read_active_viewport_bar_count(&app),
                read_active_view_start(&app),
                p_trade, p_bar, p_bars_loaded, p_backfill,
            ));
            last_log = Instant::now();
        }
        sleep(Duration::from_millis(16)).await;
    }

    let end_ts        = read_active_last_ts(&app);
    let end_bubble_n  = read_active_bars_len(&app);
    let end_vp_count  = read_active_viewport_bar_count(&app);
    let end_vp_vstart = read_active_view_start(&app);
    log(&format!(
        "[smoke-1m] DONE ticks={} | bubble: close_uniq={} bars: {}->{} ts: {}->{} dt_s={} \
         | viewport: count_uniq={} count: {}->{} vstart: {:.3}->{:.3} \
         | probe: trade={} bar={} bars_loaded={} backfill={} other={}",
        tick_count, distinct_bubble_close.len(),
        start_bubble_n, end_bubble_n, start_ts, end_ts, end_ts - start_ts,
        distinct_vp_count.len(), start_vp_count, end_vp_count,
        start_vp_vstart, end_vp_vstart, p_trade, p_bar, p_bars_loaded, p_backfill, p_other,
    ));

    // ── Layer 1: bridge → BackendCore::tick_writers → bubble ──
    // The bubble write path. Same code that derived_bars_live.rs already
    // covers, repeated here through ChartApp::tick() to confirm tick()
    // calls tick_writers and the bubble grows under the live tape.
    assert!(end_ts >= start_ts, "bubble last_ts went backwards: {} -> {}", start_ts, end_ts);
    // Lower bound matches reality: on calm BTC Spot tape, 60 s often yields
    // only ~25 distinct close values even with thousands of incoming trades
    // (most trades hit identical prices). ≥5 distinct = tick_writers
    // definitely consumed multiple TradeUpdates; 0 or 1 would be the bug.
    assert!(
        distinct_bubble_close.len() >= 5,
        "[layer-1 bridge→bubble] only {} distinct bubble.last_close in 60s \
         @1m (ticks={}, probe trade={}) — tick_writers is not seeing \
         TradeUpdate even though bridge probe emits them",
        distinct_bubble_close.len(), tick_count, p_trade
    );
    assert!(
        end_ts > start_ts,
        "[layer-1 bridge→bubble] no new 1m bar in 60s; bubble last_ts \
         frozen at {} — TradeUpdatePassthrough did not roll the candle \
         when trade_ts_secs >= candle_end",
        start_ts
    );

    // ── Layer 2: bubble → react.rs viewport side-effects ──
    // THE point of this test. If layer-1 is green but layer-2 fails,
    // `react_to_bubble_event` arms (BarUpdatePassthrough line 487 +
    // TradeUpdatePassthrough line 619) did not run — exactly the wasm
    // regression the previous handoff chased through chart_dirty.
    assert!(
        end_vp_count >= end_bubble_n.saturating_sub(2),
        "[layer-2 bubble→react.rs] viewport.bar_count={} lags behind \
         bubble.bars.len()={} by more than 2 — react_to_bubble_event \
         BarUpdate/TradeUpdate arms are not firing (or window mismatch)",
        end_vp_count, end_bubble_n
    );
    // Either the count distinctly moved (new bars rolled) or view_start
    // distinctly moved (auto-scroll kicked) — at least one MUST happen
    // when the candle closes inside a 60s window @1m.
    assert!(
        distinct_vp_count.len() >= 2 || (end_vp_vstart - start_vp_vstart).abs() >= 0.5,
        "[layer-2 bubble→react.rs] viewport state never moved during \
         60s of flow (count_uniq={}, vstart: {:.3}->{:.3}). Either no \
         new bar rolled (1m boundary not crossed) or the react.rs \
         passthrough arms are dead.",
        distinct_vp_count.len(), start_vp_vstart, end_vp_vstart
    );
}

// ─── Test 2: scroll-backfill on viewport shift ───────────────────────────────

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 4))]
async fn chart_app_scroll_backfill_triggers_on_view_shift() {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let mut probe_rx = bridge.add_listener();

    let core = mlc_backend::BackendCore::new(bridge.clone(), None);
    let mut app = ChartApp::new_empty(core);
    app.seed_default_chart(
        ExchangeId::Binance, "BTCUSDT", tf_m1(), AccountType::Spot, 500,
    );

    // Wait for backfill to land ~500 bars.
    let settle_deadline = Instant::now() + Duration::from_secs(45);
    let mut last_bars_len = 0;
    let mut stable_since = Instant::now();
    loop {
        app.tick(now_ms());
        let n = read_active_bars_len(&app);
        if n != last_bars_len {
            last_bars_len = n;
            stable_since = Instant::now();
        } else if n >= 500 && stable_since.elapsed() >= Duration::from_millis(2000) {
            break;
        }
        assert!(Instant::now() < settle_deadline, "settle never finished");
        sleep(Duration::from_millis(30)).await;
    }
    let bars_before = read_active_bars_len(&app);

    // Drain probe — don't count BarsLoaded/Backfill from phase A.
    while let Ok(_) = probe_rx.try_recv() {}

    // Force viewport to far-left so scroll-fetch trigger fires.
    let active_cid = app.panel_app.panel_grid.active_container_id().expect("active container");
    {
        let win = app.panel_app.panel_grid.containers_mut()
            .get_mut(&active_cid).expect("active window");
        if win.viewport.chart_width <= 0.0 { win.viewport.chart_width = 800.0; }
        if win.viewport.bar_spacing <= 0.0 { win.viewport.bar_spacing = 8.0; }
        win.viewport.view_start = 0.0;
        log(&format!(
            "[smoke-scroll] forced viewport: cw={} bs={} vs={} bars={}",
            win.viewport.chart_width, win.viewport.bar_spacing,
            win.viewport.view_start, bars_before
        ));
    }

    let scroll_deadline = Instant::now() + Duration::from_secs(30);
    let mut got_scroll = false;
    let mut got_bars_loaded_after = 0u32;
    let mut total_drained = 0u32;
    while Instant::now() < scroll_deadline {
        app.tick(now_ms());
        loop {
            match probe_rx.try_recv() {
                Ok(LiveUpdate::ScrollBarsLoaded { .. }) => {
                    got_scroll = true;
                    log("[smoke-scroll] ScrollBarsLoaded arrived");
                }
                Ok(LiveUpdate::BarsLoaded { .. }) => got_bars_loaded_after += 1,
                Ok(_) => {}
                Err(_) => break,
            }
            total_drained += 1;
        }
        if got_scroll { break; }
        sleep(Duration::from_millis(50)).await;
    }

    let bars_after = read_active_bars_len(&app);
    let vp_count_after = read_active_viewport_bar_count(&app);
    log(&format!(
        "[smoke-scroll] DONE got_scroll={} bubble: {}->{} viewport.bar_count={} \
         extra_bars_loaded={} probe_drains={}",
        got_scroll, bars_before, bars_after, vp_count_after,
        got_bars_loaded_after, total_drained,
    ));

    // ── Layer 1: scroll fetch + bubble merge ──
    assert!(
        got_scroll,
        "viewport.view_start=0 did NOT trigger any LiveUpdate::ScrollBarsLoaded \
         within 30s — bridge.request_scroll_bars never fired, or station \
         scroll-fetch silent. Check tick.rs scroll-fetch block."
    );
    assert!(
        bars_after > bars_before,
        "[layer-1 bubble merge] scroll-backfill did not grow bubble: \
         bars_before={} bars_after={} — backend-core ScrollBarsLoaded merge \
         regressed (the older page is overwriting the live tail again)",
        bars_before, bars_after
    );

    // ── Layer 2: bubble → react.rs ScrollBarsAppended arm ──
    // react.rs:334 ScrollBarsAppended branch must reconcile viewport so
    // viewport.bar_count tracks the grown bubble. If layer-1 grew bubble
    // but layer-2 is silent, the chart will still render the OLD count
    // and clip the new older bars — same class of bug as test 1.
    assert!(
        vp_count_after >= bars_after.saturating_sub(2),
        "[layer-2 bubble→react.rs] viewport.bar_count={} lags bubble.bars.len()={} \
         after scroll-backfill — react.rs ScrollBarsAppended arm did not \
         reconcile viewport (chart would render with stale count)",
        vp_count_after, bars_after
    );
}
