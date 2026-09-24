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
use mlc_backend::BubbleEvent;
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
        .map(|w| w.viewport().bar_count).unwrap_or(0)
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
        .map(|w| w.viewport().view_start).unwrap_or(0.0)
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

fn tf_h1() -> Timeframe { Timeframe::h1() }

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
        win.force_viewport_for_test(800.0, 8.0, 0.0);
        log(&format!(
            "[smoke-scroll] forced viewport: cw={} bs={} vs={} bars={}",
            win.viewport().chart_width, win.viewport().bar_spacing,
            win.viewport().view_start, bars_before
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

// ─── Test 3: first-visit hydration lands on the latest bars ────────────────
//
// Regression coverage for the owner repro 2026-09-24 ("опять кто-то сломал
// логику выравнивания данных"): a genuinely fresh profile (no saved preset)
// landed `view_start` in the middle of history (725/2001 or 1700/2001 bars)
// instead of at the right edge, while a reload with a saved profile snapped
// correctly. Root cause: `ChartApp::new_window`'s fresh-state branch
// (mlc-app/src/lib.rs) never armed `window.pending_symbol_load` /
// `window.needs_snap_after_backfill` the way every OTHER hydration path
// (`apply_preset_cold`, `seed_default_chart`) does. Without them, the
// window's first-ever `BarsLoaded` is misclassified as a BACKFILL
// (`is_backfill = !pending_symbol_load` reads `!false`) instead of a fresh
// load in `react_to_bubble_event` — `capture_view_anchor` on the
// still-empty prior bars returns `None`, so `reconcile_view_keep_anchor`
// leaves `view_start` at its construction default (0.0). A later backfill
// page (older bars prepended in front) then anchors relative to that stale
// `view_start` instead of the right edge, landing the viewport mid-history.
//
// This test drives the exact sequence documented in
// `hydration_impl.rs`("stale cached bars arrive via BarsLoaded first; fresh
// exchange data arrives later via BackfillComplete") through the real
// reactor (`ChartApp::react_to_events`, the same entry point `ChartApp::tick`
// and the multi-window vello host use) with synthesized bars — no network
// needed, since the bug lives entirely in the reactor's event classification,
// not in bar delivery itself.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn first_visit_hydration_snaps_to_right_edge_after_prepend() {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let core = mlc_backend::BackendCore::new(bridge, None);
    let mut app = ChartApp::new_empty(core);

    // Mirror `ChartApp::new_window`'s fresh-state branch by hand: create the
    // auto-bubble, attach it to the active chart, and arm the two
    // fresh-hydration flags the fix adds.
    let chart_id = app.panel_app.panel_grid.active_container_id()
        .expect("new_empty seeds one active window");
    let bid = app.create_auto_bubble([0.0, 0.0, 0.0, 0.0], "BTCUSDT".to_string(), tf_h1());
    app.attach_chart_to_tag(chart_id, bid);
    // `Timeframe::h1().name` is `"1H"` (uppercase) — the reactor's window
    // match (`win_tf.name == *tf_name`) is a literal string compare against
    // the `BubbleEvent`'s own `timeframe` field, so every synthesized event
    // below must carry this exact spelling.
    let tf_name = tf_h1().name.clone();
    {
        let win = app.panel_app.panel_grid.containers_mut().get_mut(&chart_id)
            .expect("active window");
        win.auto_tag_id = bid;
        win.current_tag_id = bid;
        // The fix under test: every hydration path must arm these two
        // before the first bar delivery (see doc comment above).
        win.pending_symbol_load = true;
        win.needs_snap_after_backfill = true;
        // Settle geometry up front, matching production reality: the wasm
        // resize debounce (CLAUDE.md "two-equal-frames") always settles
        // before the request_bars round-trip returns. `force_viewport_for_test`
        // only overrides `chart_width` here (`bar_spacing` already defaults
        // to 8.0, `Viewport::default`, so its own param is a no-op) — but
        // `consume_cold_snap`'s `reset_to_default()` (armed by
        // `needs_snap_after_backfill`, fires on the very first bar delivery
        // below) recomputes `bar_spacing = chart_width / 100` regardless —
        // 1000.0 / 100 = 10.0, a clean number chosen so the expected-edge
        // math below matches the real post-reset value exactly.
        win.force_viewport_for_test(1000.0, 10.0, 0.0);
    }
    app.core.bubbles_mut().set_bid_instrument_full(
        bid, "BTCUSDT".to_string(), "binance".to_string(), "S".to_string(), tf_h1(),
    );

    let chart_width: f64 = 1000.0;
    // The value `bar_spacing` settles at after the cold-snap's own
    // `reset_to_default()` runs on the first bar delivery below (see the
    // `force_viewport_for_test` comment above) — NOT `Viewport::default`'s
    // 8.0, which only holds before that first snap.
    let bar_spacing: f64 = 10.0;
    let visible_f: f64 = chart_width / bar_spacing;
    let expected_right_edge = |bar_count: usize| -> f64 {
        // Mirrors `ChartContainer::snap_to_end`'s progressive-margin formula.
        let progressive = (0.05 * visible_f).ceil().max(0.0);
        let margin = mlc_app::chart_container::DEFAULT_SNAP_MARGIN.min(progressive);
        bar_count as f64 + margin - visible_f
    };

    // Stage 1: a small CACHE-served batch lands first ("stale cached bars
    // arrive via BarsLoaded first"). A clean multiple of the 1H bar
    // interval (3600s) — stage 3's live trade below relies on exact
    // candle-boundary arithmetic (`react.rs`'s `TradeUpdatePassthrough`
    // handler: `new_candle_start = (trade_ts_secs / period_secs) * period_secs`).
    let base_ts: i64 = 1_699_999_200;
    let cache_bars: Vec<mlc_core::Bar> = (0..100i64)
        .map(|i| mlc_core::Bar::new(base_ts + i * 3600, 100.0, 101.0, 99.0, 100.5))
        .collect();
    // In production `tick_writers` writes the new payload into the bubble
    // store BEFORE emitting the event (react.rs's own module doc: "the
    // write half ... has already happened"); `react_to_events` alone does
    // not, so the audit-D8 active-series resolver `reconcile_view_fresh`
    // reads for `bar_count` would otherwise still see the bubble's
    // construction-time empty array. Mirror the write here.
    app.core.bubbles_mut().replace_bars(bid, cache_bars.clone());
    app.react_to_events(&[BubbleEvent::BarsLoaded {
        bid,
        exchange_id: ExchangeId::Binance,
        symbol: "BTCUSDT".to_string(),
        account_type: "S".to_string(),
        timeframe: tf_name.clone(),
        bars: cache_bars.clone(),
        prev_bars: Vec::new(),
    }], now_ms());

    let vs_after_cache = read_active_view_start(&app);
    let edge_after_cache = expected_right_edge(cache_bars.len());
    assert!(
        (vs_after_cache - edge_after_cache).abs() < 1.0,
        "[stage 1] cache batch did not snap to its own right edge: \
         view_start={vs_after_cache} expected~{edge_after_cache} bars={}",
        cache_bars.len()
    );

    // Stage 2: the real REST backfill lands later, PREPENDING 1901 older
    // bars in front of the 100 already shown — full_bars[1901..2001] is
    // byte-for-byte the same 100 bars as `cache_bars`.
    let full_bars: Vec<mlc_core::Bar> = (0..2001i64)
        .map(|i| mlc_core::Bar::new(base_ts + (i - 1901) * 3600, 90.0, 91.0, 89.0, 90.5))
        .collect();
    assert_eq!(full_bars.len(), 2001);
    // Same write-before-emit mirror as stage 1 — `prev_bars: cache_bars`
    // (already the pre-write snapshot) stays correct since it is passed as
    // the OLD state, not read back from the store.
    app.core.bubbles_mut().replace_bars(bid, full_bars.clone());
    app.react_to_events(&[BubbleEvent::BackfillCompleted {
        bid,
        exchange_id: ExchangeId::Binance,
        symbol: "BTCUSDT".to_string(),
        account_type: "S".to_string(),
        timeframe: tf_name.clone(),
        bars: full_bars.clone(),
        prev_bars: cache_bars,
    }], now_ms());

    let vs_after_backfill = read_active_view_start(&app);
    let bc_after_backfill = read_active_viewport_bar_count(&app);
    let edge_after_backfill = expected_right_edge(full_bars.len());
    assert_eq!(bc_after_backfill, 2001, "viewport.bar_count must track the full 2001-bar history");
    assert!(
        (vs_after_backfill - edge_after_backfill).abs() < 5.0,
        "[stage 2 — the owner repro] history prepend left the viewport \
         mid-history: view_start={vs_after_backfill} (expected ~{edge_after_backfill}, \
         bar_count={bc_after_backfill}, total=2001) — a fresh window whose first \
         BarsLoaded was not armed with pending_symbol_load/needs_snap_after_backfill \
         gets misclassified as a backfill and never reaches the right edge",
    );

    // Stage 3: one live trade ticks in, past the last bar's candle-close
    // boundary. `BarUpdatePassthrough` is a read-only repaint signal for
    // the trade-agg-bar/trade-built-kline family only (react.rs's own
    // `!is_derived && !is_trade_built { continue; }` gate skips every
    // plain kline window) — a plain "candles" window's viewport is driven
    // by `TradeUpdatePassthrough` instead, which both builds the new
    // candle INTO the bubble store itself and advances the viewport, the
    // same event a real live trade produces.
    let last_ts = full_bars.last().map(|b| b.timestamp).expect("full_bars non-empty");
    let trade_ts_secs = last_ts + 3600; // one full 1H interval past the last bar
    app.react_to_events(&[BubbleEvent::TradeUpdatePassthrough {
        exchange_id: ExchangeId::Binance,
        account_type: "S".to_string(),
        symbol: "BTCUSDT".to_string(),
        price: 101.5,
        quantity: 1.0,
        timestamp: trade_ts_secs * 1000,
        is_buyer_maker: false,
    }], now_ms());

    let vs_after_live = read_active_view_start(&app);
    let bc_after_live = read_active_viewport_bar_count(&app);
    assert_eq!(bc_after_live, 2002, "viewport.bar_count must track the live-appended bar");
    let edge_after_live = expected_right_edge(bc_after_live);
    assert!(
        (vs_after_live - edge_after_live).abs() < bar_spacing.max(6.0),
        "[stage 3] a live tick after correct hydration must keep the viewport \
         glued to the right edge: view_start={vs_after_live} expected~{edge_after_live} \
         bar_count={bc_after_live}",
    );
}

// ─── Test 4: R23 — opening a toolbar dropdown must close the open modal ────
//
// Owner repro 2026-09-23: with a modal open (the indicator-settings modal
// for RSI in the report), clicking a toolbar button that opens a DROPDOWN
// opened the dropdown on top of the still-open modal instead of closing it
// first. Fix (`ChartApp::dispatch_panel_click`, `mlc-app/src/input/
// panel_click.rs`): a before/after transition check on `open_dropdown_id`
// (main topbar AND the trading-container header) runs
// `close_modals_for_toolbar_dropdown` the moment ANY click just opened a
// dropdown — no per-button allow-list to remember, so a new dropdown added
// anywhere in `mlc-core`'s `ToolbarState` or this crate is covered for free.
//
// Drives the real click path end to end: open the indicator-settings modal
// exactly as a real RSI-settings click would leave it, then click the
// `cursor_tools` toolbar dropdown trigger (first click on an unopened
// dropdown item always opens it — `toolbar_state.rs`) and assert the modal
// is gone.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn toolbar_dropdown_open_closes_the_open_settings_modal() {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let core = mlc_backend::BackendCore::new(bridge, None);
    let mut app = ChartApp::new_empty(core);

    // Open the indicator-settings modal (the RSI-settings case from the
    // owner's report) — `indicator_id` is a made-up id; the modal only
    // needs to report itself open, nothing renders in this test.
    app.panel_app.indicator_settings_state.open(1);
    assert!(
        app.panel_app.indicator_settings_state.is_open(),
        "setup: indicator-settings modal did not open"
    );
    assert!(
        app.panel_app.toolbar_state.open_dropdown_id.is_none(),
        "setup: no toolbar dropdown should be open yet"
    );

    // Click a toolbar dropdown trigger.
    app.dispatch_panel_click("toolbar:cursor_tools", 0.0, 0.0);

    assert!(
        app.panel_app.toolbar_state.open_dropdown_id.is_some(),
        "the toolbar click did not open its dropdown — test setup is wrong, \
         not exercising the R23 path at all"
    );
    assert!(
        !app.panel_app.indicator_settings_state.is_open(),
        "[R23] indicator-settings modal is still open after a toolbar \
         dropdown opened — the dropdown now paints on top of it (owner \
         repro 2026-09-23, RSI settings modal)"
    );
}

// ─── Test 5: Synced sidebar panels must resync across a preset switch ──────
//
// Owner repro 2026-09-24: opening a shared chart renders the chart itself
// fine, but every "Synced" right-sidebar panel (DOM, Trade Tape, L2, Big
// Trades) that is supposed to always track the ACTIVE chart's instrument
// stays empty — it never connects to any data. Switching to the user's own
// normal tab and back "heals" it.
//
// Root cause: `ChartApp::prepare_frame`'s "active chart changed" detector
// (`frame_lifecycle/prepare.rs`) compared `LeafId`s from
// `panel_grid.docking().active_leaf()` — but `LeafId` counters are scoped to
// ONE docking tree and restart per preset (`apply_preset_cold`'s
// `replace_docking` installs a brand new tree deserialized from that
// preset's own `layout` JSON). Two different, simple single-window presets
// routinely land their sole leaf on the exact same small id (typically 0/1),
// which made a genuine tab switch look like a no-op and skipped
// `TagManager::reassign_synced_panels` / `ChartApp::apply_key_to_panels_in_group`
// entirely — every Synced panel kept pointing at a tag group
// `apply_preset_cold`'s `tags_mut().clear()` had already discarded, so it
// could never resolve an instrument to subscribe to.
//
// Fix: key the detector on `ChartId` (`panel_grid.active_container_id()`)
// instead — `ChartId` is a process-global, never-reset counter
// (`generate_chart_id`/`bump_chart_id_past`), so two different presets'
// active windows never collide.
//
// This test reproduces the exact collision shape without needing real
// preset JSON: `ChartPanelGrid::new` always builds a fresh single-leaf
// `DockingManager`, so two independently constructed panel grids (standing
// in for "preset A" then "preset B", the same wholesale swap
// `apply_preset_cold` performs via `replace_docking`) land their one leaf on
// the same `LeafId` while their `ChartId`s (global counter) differ.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn synced_sidebar_panel_resyncs_across_a_preset_switch_with_colliding_leaf_ids() {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let core = mlc_backend::BackendCore::new(bridge, None);
    let mut app = ChartApp::new_empty(core);

    // Window A ("preset A"): a fresh single-leaf panel grid, instrument AAA.
    let container_a = mlc_app::chart_container::ChartContainer::new("AAAUSDT", tf_h1());
    let cid_a = container_a.id;
    let bid_a = app.create_auto_bubble([0.0, 0.0, 0.0, 0.0], "AAAUSDT".to_string(), tf_h1());
    app.core.bubbles_mut().set_bid_instrument_full(
        bid_a, "AAAUSDT".to_string(), "binance".to_string(), "S".to_string(), tf_h1(),
    );
    app.panel_app.panel_grid = mlc_app::panel_grid::ChartPanelGrid::new(container_a);
    {
        let win = app.panel_app.panel_grid.containers_mut().get_mut(&cid_a).expect("window A");
        win.auto_tag_id = bid_a;
        win.current_tag_id = bid_a;
    }
    app.attach_chart_to_tag(cid_a, bid_a);
    let leaf_a = app.panel_app.panel_grid.docking().active_leaf()
        .expect("window A's fresh grid has an active leaf");

    // A "Synced" DOM panel: the class of sidebar panel (DOM/Trade Tape/L2/
    // Big Trades) that always mirrors whichever chart is active — exactly
    // the family the owner reported empty on a shared/read-only tab.
    let pid = mlc_core::state::PanelId(90_001);
    app.panels_store.dom.insert(
        pid,
        mlc_app_panels::trading::order_flow::dom::DomState::new(String::new(), 0.01),
    );
    app.set_panel_synced(mlc_core::ids::SyncMemberId::Panel(pid.0), bid_a);

    // Frame 1: `last_active_chart_id` starts `None`, so the detector always
    // fires on the very first frame and resolves the Synced panel onto A.
    app.prepare_frame(1000.0, 800.0);
    let sym_after_a = app.panels_store.dom.get(&pid).map(|s| s.symbol.clone()).unwrap_or_default();
    assert_eq!(
        sym_after_a, "AAAUSDT",
        "setup: Synced DOM panel did not resolve window A's instrument on the first frame"
    );

    // Window B ("preset B"): a SECOND, independently constructed panel grid
    // — the same wholesale `panel_grid` replace `apply_preset_cold` performs
    // on every cold preset/share load — instrument BBB.
    let container_b = mlc_app::chart_container::ChartContainer::new("BBBUSDT", tf_h1());
    let cid_b = container_b.id;
    assert_ne!(cid_a, cid_b, "ChartId is a global counter — must differ across containers");
    let bid_b = app.create_auto_bubble([0.0, 0.0, 0.0, 0.0], "BBBUSDT".to_string(), tf_h1());
    app.core.bubbles_mut().set_bid_instrument_full(
        bid_b, "BBBUSDT".to_string(), "binance".to_string(), "S".to_string(), tf_h1(),
    );
    app.panel_app.panel_grid = mlc_app::panel_grid::ChartPanelGrid::new(container_b);
    {
        let win = app.panel_app.panel_grid.containers_mut().get_mut(&cid_b).expect("window B");
        win.auto_tag_id = bid_b;
        win.current_tag_id = bid_b;
    }
    app.attach_chart_to_tag(cid_b, bid_b);
    let leaf_b = app.panel_app.panel_grid.docking().active_leaf()
        .expect("window B's fresh grid has an active leaf");

    // Prove the premise the bug depended on: two independently built,
    // simple single-window grids DO land on the same `LeafId` (their
    // `ChartId`s differ regardless — asserted above). If uzor ever changes
    // its leaf-id allocation scheme this stops holding and the assertion
    // below would need a different repro shape, not a silently-wrong pass.
    assert_eq!(
        leaf_a, leaf_b,
        "test premise: two fresh single-leaf panel grids must collide on \
         LeafId for this to reproduce the pre-fix bug"
    );

    // Frame 2 (the "tab switch"): with the fix, `active_container_id()`
    // (ChartId) correctly differs and resyncs the Synced panel onto B.
    // Pre-fix, the LeafId comparison saw no change and left it on A.
    app.prepare_frame(1000.0, 800.0);
    let sym_after_b = app.panels_store.dom.get(&pid).map(|s| s.symbol.clone()).unwrap_or_default();
    assert_eq!(
        sym_after_b, "BBBUSDT",
        "[owner repro 2026-09-24] Synced DOM panel did not resync after the \
         active chart switched to a different preset whose leaf id collided \
         with the previous one — this is why shared-chart sidebar panels \
         never connected to data"
    );
}

