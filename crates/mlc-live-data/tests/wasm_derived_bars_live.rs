//! mlc-live-data wasm e2e: every derived-bar subscription type must produce
//! REAL bars from REAL Binance BTCUSDT aggTrades within a few seconds.
//!
//! Unlike `wasm_bridge_smoke.rs`, this does NOT prove the kline path or
//! the synthetic injection path — those are covered elsewhere. This proves
//! that `bridge.request_<TYPE>_bars(...)` correctly translates into a
//! `Station::subscribe(Stream::<TYPE>(...))` AND the upstream station
//! aggregator produces a bar on the wire fast enough that the chart UI
//! would actually animate.
//!
//! Each test:
//!   1. opens a probe broadcast listener
//!   2. calls `bridge.request_<TYPE>_bars(...)` with parameters tight enough
//!      that BTCUSDT (~30-100 trades/s, ~$5-50 swings/s, ~0.01 BTC volume/s)
//!      WILL produce ≥1 bar inside `MAX_WAIT_SECS`
//!   3. asserts ≥1 BarUpdate / typed *Update on the probe channel
//!
//! Run with:
//!   cargo test --target wasm32-unknown-unknown \
//!     -p mylittlechart-live-data --test wasm_derived_bars_live \
//!     -- --nocapture
//!
//! Each test has its own DataBridge instance — keeps total wall-clock
//! per test under the runner's per-test timeout.

#![cfg(target_arch = "wasm32")]

use std::sync::Arc;
use std::time::Duration;

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use digdigdig3::core::types::{AccountType, ExchangeId};
use mlc_live_data::{DataBridge, LiveUpdate};

const MAX_WAIT_SECS: u64 = 35;

/// Boot a fresh bridge, dial Binance, return `(bridge, probe_rx)`.
async fn boot() -> (Arc<DataBridge>, tokio::sync::broadcast::Receiver<LiveUpdate>) {
    let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let probe_rx = bridge.add_listener();
    bridge.ensure_connector(ExchangeId::Binance);
    (bridge, probe_rx)
}

/// Boot a fresh bridge dialed to an arbitrary venue. Used for streams that
/// Binance does NOT expose via WS (e.g. OpenInterest — Binance OI is REST
/// polling only; per `digdigdig3::Binance::OI` returns `WireAbsent`).
async fn boot_for(exchange: ExchangeId) -> (Arc<DataBridge>, tokio::sync::broadcast::Receiver<LiveUpdate>) {
    let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let probe_rx = bridge.add_listener();
    bridge.ensure_connector(exchange);
    (bridge, probe_rx)
}

/// Drain probe_rx until any matching `LiveUpdate` arrives or the deadline.
/// `matches(update)` returns true on success.
async fn wait_for<F: Fn(&LiveUpdate) -> bool>(
    rx: &mut tokio::sync::broadcast::Receiver<LiveUpdate>,
    max_secs: u64,
    matches: F,
) -> Option<LiveUpdate> {
    use futures_util::{future::Either, pin_mut};
    let sleep_fut = gloo_timers::future::sleep(Duration::from_secs(max_secs));
    pin_mut!(sleep_fut);
    loop {
        let recv_fut = rx.recv();
        pin_mut!(recv_fut);
        match futures_util::future::select(recv_fut, sleep_fut.as_mut()).await {
            Either::Left((Ok(ev), _)) => {
                if matches(&ev) {
                    return Some(ev);
                }
            }
            Either::Left((Err(_), _)) => return None,
            Either::Right(_) => return None,
        }
    }
}

fn assert_bar_for(symbol: &str, ev: Option<LiveUpdate>, label: &str) {
    let Some(LiveUpdate::BarUpdate { symbol: s, bar, .. }) = ev else {
        panic!(
            "[{label}] no BarUpdate for {symbol} within {MAX_WAIT_SECS}s — bridge \
             request_*_bars dispatch broken, or station aggregator not emitting"
        );
    };
    assert_eq!(s, symbol, "[{label}] BarUpdate symbol mismatch");
    assert!(bar.close > 0.0, "[{label}] BarUpdate.close must be positive");
    web_sys::console::log_1(
        &format!(
            "[derived-live {label}] OK first bar: ts={} o={} h={} l={} c={} v={}",
            bar.timestamp, bar.open, bar.high, bar.low, bar.close, bar.volume
        )
        .into(),
    );
}

// ─── Tick bars: 10 trades per bar — BTC does ~30-100 trades/s ──────────────

#[wasm_bindgen_test]
async fn derived_live_tick_bars() {
    let (bridge, mut probe) = boot().await;
    bridge.request_tick_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 10);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::BarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    assert_bar_for("BTCUSDT", ev, "tick_bars(10)");
}

// ─── Range bars: $5 price-move per bar — BTC swings $5+ in seconds ─────────

#[wasm_bindgen_test]
async fn derived_live_range_bars() {
    let (bridge, mut probe) = boot().await;
    // price_units = price × 1e8 → $5 = 5_00000000
    bridge.request_range_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 5_00_000_000);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::BarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    assert_bar_for("BTCUSDT", ev, "range_bars($5)");
}

// ─── Volume bars: ~0.05 BTC per bar — BTC trades that much in ~2s ──────────

#[wasm_bindgen_test]
async fn derived_live_volume_bars() {
    let (bridge, mut probe) = boot().await;
    // volume_units = volume × 1e8 → 0.05 BTC = 5_000_000
    bridge.request_volume_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 5_000_000);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::BarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    assert_bar_for("BTCUSDT", ev, "volume_bars(0.05 BTC)");
}

// ─── Dollar bars: $5_000 per bar — BTC trades $5k notional in ~1s ──────────

#[wasm_bindgen_test]
async fn derived_live_dollar_bars() {
    let (bridge, mut probe) = boot().await;
    bridge.request_dollar_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 5_000);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::BarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    assert_bar_for("BTCUSDT", ev, "dollar_bars($5k)");
}

// ─── Tick-imbalance bars: 50 ticks minimum, alpha=20% ──────────────────────

#[wasm_bindgen_test]
async fn derived_live_tick_imbalance_bars() {
    let (bridge, mut probe) = boot().await;
    bridge.request_tick_imbalance_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 20, 50);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::BarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    assert_bar_for("BTCUSDT", ev, "tick_imbalance_bars(α=20% min=50)");
}

// ─── Volume-imbalance bars: same alpha/min as tick variant ─────────────────

#[wasm_bindgen_test]
async fn derived_live_volume_imbalance_bars() {
    let (bridge, mut probe) = boot().await;
    bridge.request_volume_imbalance_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 20, 50);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::BarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    assert_bar_for("BTCUSDT", ev, "volume_imbalance_bars(α=20% min=50)");
}

// ─── Run bars: tightest possible to trigger fast ───────────────────────────

#[wasm_bindgen_test]
async fn derived_live_run_bars() {
    let (bridge, mut probe) = boot().await;
    bridge.request_run_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 20, 50);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::BarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    assert_bar_for("BTCUSDT", ev, "run_bars(α=20% min=50)");
}

// ─── Footprint: typed LiveUpdate::FootprintUpdate, 1m bucket ───────────────

#[wasm_bindgen_test]
async fn derived_live_footprint() {
    let (bridge, mut probe) = boot().await;
    bridge.request_footprint(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, "1m");
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::FootprintUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::FootprintUpdate { symbol, point, .. }) = ev else {
        panic!("no FootprintUpdate within {MAX_WAIT_SECS}s — footprint dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    assert!(!point.levels.is_empty(), "FootprintPoint.levels must not be empty");
    web_sys::console::log_1(
        &format!(
            "[derived-live footprint(1m)] OK levels={} open={} close={}",
            point.levels.len(), point.open, point.close
        ).into(),
    );
}

// ─── Renko bars: brick_size in price units. ~$50 to fire fast on BTC ───────

#[wasm_bindgen_test]
async fn derived_live_renko_bars() {
    let (bridge, mut probe) = boot().await;
    // brick_size: $50 = 50_00000000 in price × 1e8
    bridge.request_renko_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 50_00_000_000, 1);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::RenkoBarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::RenkoBarUpdate { symbol, point, .. }) = ev else {
        panic!("no RenkoBarUpdate within {MAX_WAIT_SECS}s — renko dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    assert!(point.top > point.bottom);
    web_sys::console::log_1(
        &format!(
            "[derived-live renko($50)] OK bottom={} top={} up={} vol={}",
            point.bottom, point.top, point.up, point.volume
        ).into(),
    );
}

// ─── PnF columns: box_size + reversal ──────────────────────────────────────

#[wasm_bindgen_test]
async fn derived_live_pnf_bars() {
    let (bridge, mut probe) = boot().await;
    // box_size $25, reversal 3 boxes
    bridge.request_pnf_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 25_00_000_000, 3);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::PnfBarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::PnfBarUpdate { symbol, point, .. }) = ev else {
        panic!("no PnfBarUpdate within {MAX_WAIT_SECS}s — pnf dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    web_sys::console::log_1(
        &format!(
            "[derived-live pnf($25,3)] OK col_id={} is_x={} bottom={} top={}",
            point.column_id, point.is_x, point.bottom, point.top
        ).into(),
    );
}

// ─── Kagi: reversal threshold ──────────────────────────────────────────────

#[wasm_bindgen_test]
async fn derived_live_kagi_bars() {
    let (bridge, mut probe) = boot().await;
    // Kagi reversal: $50 = 50_00000000
    bridge.request_kagi_bars(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 50_00_000_000);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::KagiBarUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::KagiBarUpdate { symbol, point, .. }) = ev else {
        panic!("no KagiBarUpdate within {MAX_WAIT_SECS}s — kagi dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    web_sys::console::log_1(
        &format!(
            "[derived-live kagi($50)] OK start={} end={} up={}",
            point.start_price, point.end_price, point.up
        ).into(),
    );
}

// ─── CVD line: scalar per-trade ────────────────────────────────────────────

#[wasm_bindgen_test]
async fn derived_live_cvd_line() {
    let (bridge, mut probe) = boot().await;
    bridge.request_cvd_line(ExchangeId::Binance, "BTCUSDT", AccountType::Spot);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::CvdLineUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::CvdLineUpdate { symbol, point, .. }) = ev else {
        panic!("no CvdLineUpdate within {MAX_WAIT_SECS}s — cvd dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    web_sys::console::log_1(
        &format!("[derived-live cvd] OK ts={} value={}", point.ts_ms, point.value).into(),
    );
}

// ─── Open Interest: futures USDM, ~one event per minute ──────────────────
//
// **Venue choice — Bybit, not Binance.** Binance OI is REST-poll only
// (`digdigdig3::cex::binance::protocol` returns `WireAbsent` for
// `Stream::OpenInterest` — there is no realtime WS open-interest channel
// on Binance; their /fapi/v1/openInterest is REST-poll). station.subscribe
// would silently record this in `failed[]` and the handle would never
// produce an OpenInterestUpdate event — the test would time out at 90s
// with `total_seen=0`.
//
// Bybit publishes OI on `tickers.{symbol}` (each linear-perp ticker
// frame carries `openInterest`; the WS protocol surfaces it as
// `Stream::OpenInterest`). dig3 marks Bybit + GateIO + HTX + OKX as
// WS-OI-supported (CLAUDE.md "Flag audit + channel completion").

#[wasm_bindgen_test]
async fn derived_live_open_interest_futures() {
    let (bridge, mut probe) = boot_for(ExchangeId::Bybit).await;
    web_sys::console::log_1(&"[oi-test] boot ok (bybit), requesting OI".into());
    bridge.request_open_interest(ExchangeId::Bybit, "BTCUSDT", AccountType::FuturesCross);
    web_sys::console::log_1(&"[oi-test] request_open_interest returned, polling probe".into());
    // Count any LiveUpdate variants we see while waiting so a silent block
    // becomes visible.
    let mut total_seen: u32 = 0;
    let mut oi_seen: u32 = 0;
    let mut other_log_pending = web_time::Instant::now();
    let mut found: Option<LiveUpdate> = None;
    let deadline_inst = web_time::Instant::now() + Duration::from_secs(90);
    while web_time::Instant::now() < deadline_inst {
        use futures_util::{future::Either, pin_mut};
        let recv_fut = probe.recv();
        let sleep_fut = gloo_timers::future::sleep(Duration::from_secs(5));
        pin_mut!(recv_fut, sleep_fut);
        match futures_util::future::select(recv_fut, sleep_fut).await {
            Either::Left((Ok(u), _)) => {
                total_seen += 1;
                if matches!(&u, LiveUpdate::OpenInterestUpdate { symbol, .. } if symbol == "BTCUSDT") {
                    oi_seen += 1;
                    found = Some(u);
                    break;
                }
            }
            Either::Left((Err(_), _)) => {}
            Either::Right(_) => {
                if other_log_pending.elapsed() >= Duration::from_secs(5) {
                    web_sys::console::log_1(
                        &format!("[oi-test] waiting... total_seen={} oi_seen={}", total_seen, oi_seen).into(),
                    );
                    other_log_pending = web_time::Instant::now();
                }
            }
        }
    }
    web_sys::console::log_1(
        &format!("[oi-test] final total_seen={} oi_seen={}", total_seen, oi_seen).into(),
    );
    let ev = found;
    let Some(LiveUpdate::OpenInterestUpdate { symbol, point, .. }) = ev else {
        panic!("no OpenInterestUpdate within 90s on Bybit — OI dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    assert!(point.oi > 0.0, "OI must be positive");
    web_sys::console::log_1(
        &format!(
            "[derived-live oi(futures)] OK ts={} oi={} delta={}",
            point.ts_ms, point.oi, point.delta
        ).into(),
    );
}

// ─── Funding Rate: REST-driven; futures USDM ───────────────────────────────

#[wasm_bindgen_test]
async fn derived_live_funding_rate_futures() {
    let (bridge, mut probe) = boot().await;
    bridge.request_funding_rate(ExchangeId::Binance, "BTCUSDT", AccountType::FuturesCross);
    // Funding-rate forwarder seed-emits the latest known rate immediately
    // after subscribe, so a short wait should hit.
    let ev = wait_for(&mut probe, 60, |u| {
        matches!(u, LiveUpdate::FundingRateUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::FundingRateUpdate { symbol, point, .. }) = ev else {
        panic!("no FundingRateUpdate within 60s — FundingRate dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    web_sys::console::log_1(
        &format!(
            "[derived-live funding(futures)] OK ts={} rate={} interval_h={}",
            point.ts_ms, point.rate, point.interval_hours
        ).into(),
    );
}

// ─── Three-Line-Break: tightest threshold to trigger fast ──────────────────

#[wasm_bindgen_test]
async fn derived_live_three_line_break() {
    let (bridge, mut probe) = boot().await;
    // n_lines = 3 (canonical), interval_ms tick. Drive via 1m bucket.
    bridge.request_three_line_break(ExchangeId::Binance, "BTCUSDT", AccountType::Spot, 3);
    let ev = wait_for(&mut probe, MAX_WAIT_SECS, |u| {
        matches!(u, LiveUpdate::ThreeLineBreakUpdate { symbol, .. } if symbol == "BTCUSDT")
    }).await;
    let Some(LiveUpdate::ThreeLineBreakUpdate { symbol, point, .. }) = ev else {
        panic!("no ThreeLineBreakUpdate within {MAX_WAIT_SECS}s — TLB dispatch broken");
    };
    assert_eq!(symbol, "BTCUSDT");
    web_sys::console::log_1(
        &format!(
            "[derived-live tlb(3)] OK open={} close={} dir={}",
            point.open, point.close, point.direction
        ).into(),
    );
}
