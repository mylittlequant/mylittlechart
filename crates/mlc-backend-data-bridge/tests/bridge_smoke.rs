//! mlc-backend-data-bridge dual-target smoke: prove the bridge-daemon layer
//! (`DataBridge`) works end-to-end on BOTH wasm32 (headless Chrome via
//! dig2-wasm-test) AND native (tokio multi-thread runtime).
//!
//! Path under test:
//!   browser/native WS -> dig3-station Station -> mlc_backend_data_bridge::DataBridge
//!     -> broadcast(LiveUpdate::TradeUpdate)
//!
//! If this test goes green but live-tick is still frozen in the running
//! mlc-app-shell-web tab, the leak is downstream of the bridge — bubble write
//! path, BackendCore::tick_writers, or UI react/render.
//!
//! Run:
//!   cargo test --target wasm32-unknown-unknown \
//!     -p mylittlechart-backend-data-bridge --test bridge_smoke -- --nocapture
//!   cargo test -p mylittlechart-backend-data-bridge --test bridge_smoke -- --nocapture

use std::time::Duration;

use digdigdig3::core::types::{AccountType, ExchangeId};
use mlc_backend_data_bridge::{DataBridge, LiveUpdate};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test_configure;
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

type Instant = web_time::Instant;

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

async fn wait_for_trade(
    rx: &mut tokio::sync::broadcast::Receiver<LiveUpdate>,
    symbol: &str,
    max_wait: Duration,
) -> Option<LiveUpdate> {
    use futures_util::{future::Either, pin_mut};
    let deadline = Instant::now() + max_wait;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() { return None; }
        let recv_fut = rx.recv();
        let sleep_fut = sleep(remaining);
        pin_mut!(recv_fut, sleep_fut);
        match futures_util::future::select(recv_fut, sleep_fut).await {
            Either::Left((Ok(ev), _)) => {
                if let LiveUpdate::TradeUpdate { symbol: s, .. } = &ev {
                    if s == symbol { return Some(ev); }
                }
            }
            Either::Left((Err(_), _)) => return None,
            Either::Right(_) => return None,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 4))]
async fn bridge_binance_trade_recv() {
    let (bridge, mut live_rx, _connector_ready_rx) = DataBridge::new().await;
    let symbol = "BTCUSDT";
    bridge.ensure_connector(ExchangeId::Binance);
    let _ = bridge.subscribe_trades(ExchangeId::Binance, symbol, AccountType::Spot);

    let ev = wait_for_trade(&mut live_rx, symbol, Duration::from_secs(20)).await;
    assert!(
        ev.is_some(),
        "DataBridge::subscribe_trades(Binance BTCUSDT Spot) did not deliver \
         a single LiveUpdate::TradeUpdate within 20s — bridge-daemon layer is \
         broken (station/bridge link, not bubble/UI)"
    );

    if let Some(LiveUpdate::TradeUpdate { price, quantity, timestamp, .. }) = ev {
        log(&format!("[bridge_smoke] OK — first trade: price={price} qty={quantity} ts={timestamp}"));
    }
}
