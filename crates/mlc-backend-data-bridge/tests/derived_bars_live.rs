//! mlc-backend-data-bridge dual-target e2e: every surviving station-derived
//! subscription type must produce REAL data from REAL exchanges within
//! a few seconds. Shared `body_*` async fns are exercised on BOTH
//! targets:
//!
//!   * **wasm32** — `#[wasm_bindgen_test]` in headless Chrome via
//!     `dig2-wasm-test`. Sleep helper backed by `gloo_timers`.
//!     Console output via `web_sys::console::log_1`.
//!   * **native** — `#[tokio::test(flavor="multi_thread")]`. Sleep helper
//!     backed by `tokio::time::sleep`. Console output via `eprintln!`.
//!
//! Run:
//!   cargo test --target wasm32-unknown-unknown \
//!     -p mylittlechart-backend-data-bridge --test derived_bars_live -- --nocapture
//!   cargo test -p mylittlechart-backend-data-bridge --test derived_bars_live -- --nocapture
//!
//! Each test has its own `DataBridge` instance — keeps total wall-clock
//! per test under the runner's per-test timeout (300 s for the wasm
//! WebDriver session, 60 s default for native `tokio::test`).
//!
//! Unified-data-plane Э5a/Э5/Э5b (station-derived wipe): range_bar/
//! volume_bar/tick_bar/dollar_bar/tick_imbalance_bar/volume_imbalance_bar/
//! run_bar/footprint/renko/pnf/kagi/three_line_break/tpo/cvd (including the
//! tick-TF kline path, which is migrated to the `tick_bar` chart_type on
//! hydration — see `hydration_impl.rs`) now fold client-side over the
//! bubble's own kline/trade layer (`mlc-backend::chart_type` fold
//! machinery) — their station-derived `DataBridge::request_*` entry points
//! are gone, so the matching tests were removed here too. Funding rate,
//! open interest, DOM heatmap, and liquidation all still subscribe through
//! the station and keep their coverage below.

use std::sync::Arc;
use std::time::Duration;

use digdigdig3::core::types::{AccountType, ExchangeId};
use mlc_backend_data_bridge::{DataBridge, LiveUpdate};

// ── wasm test plumbing ──────────────────────────────────────────────────────
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test_configure;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

// ── cross-platform helpers ──────────────────────────────────────────────────

/// Sleep for `d`. wasm32 → `gloo_timers::future::sleep`; native →
/// `tokio::time::sleep`. Both yield the same `Future<Output = ()>`.
async fn sleep(d: Duration) {
    #[cfg(target_arch = "wasm32")]
    { gloo_timers::future::sleep(d).await; }
    #[cfg(not(target_arch = "wasm32"))]
    { tokio::time::sleep(d).await; }
}

/// Monotonic clock. `web_time::Instant` works on both targets (delegates to
/// `performance.now()` on wasm and to `std::time::Instant` on native).
type Instant = web_time::Instant;

/// Console log. Routed to DevTools console on wasm, to stderr on native.
fn log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    { web_sys::console::log_1(&msg.into()); }
    #[cfg(not(target_arch = "wasm32"))]
    { eprintln!("{}", msg); }
}

/// Boot a fresh bridge, dial `exchange`, return `(bridge, probe_rx)`.
async fn boot_for(exchange: ExchangeId) -> (Arc<DataBridge>, tokio::sync::broadcast::Receiver<LiveUpdate>) {
    let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let probe_rx = bridge.add_listener();
    bridge.ensure_connector(exchange);
    (bridge, probe_rx)
}

/// Boot a fresh bridge dialed to Binance — default for nearly every derived
/// stream test. Use `boot_for` when the test needs a different venue.
async fn boot() -> (Arc<DataBridge>, tokio::sync::broadcast::Receiver<LiveUpdate>) {
    boot_for(ExchangeId::Binance).await
}

/// Drain `rx` until any `LiveUpdate` matching `matches` arrives or the
/// deadline expires. Returns `Some(ev)` on match, `None` on timeout.
async fn wait_for<F>(
    rx: &mut tokio::sync::broadcast::Receiver<LiveUpdate>,
    max_secs: u64,
    matches: F,
) -> Option<LiveUpdate>
where F: Fn(&LiveUpdate) -> bool,
{
    let deadline = Instant::now() + Duration::from_secs(max_secs);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return None; }
        use futures_util::{future::Either, pin_mut};
        let recv_fut = rx.recv();
        let sleep_fut = sleep(remaining);
        pin_mut!(recv_fut, sleep_fut);
        match futures_util::future::select(recv_fut, sleep_fut).await {
            Either::Left((Ok(ev), _)) => { if matches(&ev) { return Some(ev); } }
            Either::Left((Err(_), _)) => return None,
            Either::Right(_) => return None,
        }
    }
}

/// Generates a single async test fn carrying BOTH attribute stacks — one
/// active per target. wasm32 gets `#[wasm_bindgen_test]`; native gets
/// `#[tokio::test(flavor="multi_thread")]`. Both targets exercise the same
/// body; the test name is identical across targets. Optional `#[ignore]`
/// is honored on both.
macro_rules! dual_test {
    ($name:ident, $body:expr) => {
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
        #[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 4))]
        async fn $name() { $body.await }
    };
    (ignore $name:ident, $body:expr) => {
        #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
        #[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 4))]
        #[ignore]
        async fn $name() { $body.await }
    };
}

// ════════════════════════════════════════════════════════════════════════════
// Funding rate + Open Interest
// ════════════════════════════════════════════════════════════════════════════

dual_test!(derived_live_funding_rate_futures, async {
    let (bridge, mut probe) = boot().await;
    bridge.request_funding_rate(ExchangeId::Binance, "BTCUSDT", AccountType::FuturesCross);
    let ev = wait_for(&mut probe, 60, |u| {
        matches!(u, LiveUpdate::FundingRateUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::FundingRateUpdate { symbol, point, .. }) = ev else {
        panic!("no FundingRateUpdate within 60s — FundingRate dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    log(&format!("[derived-live funding(futures)] OK ts={} rate={} interval_h={}",
        point.ts_ms, point.rate, point.interval_hours));
});

// **Venue choice — Bybit, not Binance.** Binance OI is REST-poll only
// (`digdigdig3::cex::binance::protocol` returns `WireAbsent` for
// `Stream::OpenInterest`). station.subscribe would silently record this in
// `failed[]` → handle would never produce an OpenInterestUpdate event.
// Bybit publishes OI on `tickers.{symbol}` (each linear-perp ticker frame
// carries `openInterest`); WS protocol surfaces it as `Stream::OpenInterest`.
dual_test!(derived_live_open_interest_futures, async {
    let (bridge, mut probe) = boot_for(ExchangeId::Bybit).await;
    bridge.request_open_interest(ExchangeId::Bybit, "BTCUSDT", AccountType::FuturesCross);
    let ev = wait_for(&mut probe, 60, |u| {
        matches!(u, LiveUpdate::OpenInterestUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::OpenInterestUpdate { symbol, point, .. }) = ev else {
        panic!("no OpenInterestUpdate within 60s on Bybit — OI dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    assert!(point.oi > 0.0, "OI must be positive");
    log(&format!("[derived-live oi(futures)] OK ts={} oi={} delta={}",
        point.ts_ms, point.oi, point.delta));
});

// ════════════════════════════════════════════════════════════════════════════
// DOM (heatmap + trade-dot share the same subscription)
// ════════════════════════════════════════════════════════════════════════════

dual_test!(derived_live_dom_heatmap_snapshot, async {
    let (bridge, mut probe) = boot().await;
    bridge.request_dom_heatmap(ExchangeId::Binance, "BTCUSDT", AccountType::Spot);
    let ev = wait_for(&mut probe, 30, |u| {
        matches!(u, LiveUpdate::DomHeatmapSnapshot { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::DomHeatmapSnapshot { symbol, ts_ms, bids, asks, .. }) = ev else {
        panic!("no DomHeatmapSnapshot within 30s — DOM heatmap dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    assert!(!bids.is_empty() || !asks.is_empty(),
        "DOM heatmap snapshot must carry book levels");
    log(&format!("[derived-live dom-heatmap] OK ts={} bids={} asks={}",
        ts_ms, bids.len(), asks.len()));
});

dual_test!(derived_live_dom_trade_dot, async {
    let (bridge, mut probe) = boot().await;
    bridge.request_dom_heatmap(ExchangeId::Binance, "BTCUSDT", AccountType::Spot);
    let ev = wait_for(&mut probe, 30, |u| {
        matches!(u, LiveUpdate::DomTradeDot { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::DomTradeDot { symbol, dot, .. }) = ev else {
        panic!("no DomTradeDot within 30s — DOM trade-dot dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    assert!(dot.price > 0.0, "trade dot price must be positive");
    log(&format!("[derived-live dom-trade-dot] OK ts={} price={} qty={}",
        dot.ts_ms, dot.price, dot.quantity));
});

// ════════════════════════════════════════════════════════════════════════════
// Liquidation (sparse — `#[ignore]` so it's not a flake gate)
// ════════════════════════════════════════════════════════════════════════════

dual_test!(ignore derived_live_liquidation_event_bybit, async {
    let (bridge, mut probe) = boot_for(ExchangeId::Bybit).await;
    bridge.request_liquidation_heatmap(ExchangeId::Bybit, "BTCUSDT", AccountType::FuturesCross);
    let ev = wait_for(&mut probe, 120, |u| {
        matches!(u, LiveUpdate::LiquidationEvent { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::LiquidationEvent { symbol, ts_ms, price, value_usd, side, .. }) = ev else {
        panic!("no LiquidationEvent within 120s on Bybit — liq dispatch broken \
               (sparse stream; rerun if you suspect timing not code)");
    };
    assert_eq!(symbol, "BTCUSDT");
    assert!(price > 0.0, "liq event must carry positive price");
    log(&format!("[derived-live liq-event(bybit)] OK ts={} px={} usd={} side={:?}",
        ts_ms, price, value_usd, side));
});
