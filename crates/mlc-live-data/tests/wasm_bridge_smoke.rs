//! mlc-live-data wasm smoke: prove the bridge-daemon layer (DataBridge) works
//! end-to-end in a headless browser before pointing fingers at bubble/UI.
//!
//! Path under test:
//!   browser WS -> dig3-station Station -> mlc_live_data::DataBridge
//!     -> broadcast(LiveUpdate::TradeUpdate)
//!
//! If this test goes green in headless Chrome but live-tick is still frozen
//! in the running mlc-app-web tab, the leak is downstream of the bridge —
//! bubble write path, BackendCore::tick_writers, or UI react/render.
//!
//! Run with:
//!   cargo test --target wasm32-unknown-unknown \
//!     -p mylittlechart-live-data --test wasm_bridge_smoke \
//!     -- bridge_binance_trade_recv --nocapture

#![cfg(target_arch = "wasm32")]

use std::time::Duration;

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use digdigdig3::core::types::{AccountType, ExchangeId};
use mlc_live_data::{DataBridge, LiveUpdate};

/// Try to receive a TradeUpdate matching the given symbol within `max_wait`.
async fn wait_for_trade(
    rx: &mut tokio::sync::broadcast::Receiver<LiveUpdate>,
    symbol: &str,
    max_wait: Duration,
) -> Option<LiveUpdate> {
    use futures_util::{future::Either, pin_mut};

    let sleep_fut = gloo_timers::future::sleep(max_wait);
    pin_mut!(sleep_fut);

    loop {
        let recv_fut = rx.recv();
        pin_mut!(recv_fut);
        match futures_util::future::select(recv_fut, sleep_fut.as_mut()).await {
            Either::Left((Ok(ev), _)) => {
                if let LiveUpdate::TradeUpdate { symbol: s, .. } = &ev {
                    if s == symbol {
                        return Some(ev);
                    }
                }
                // Not the trade we want — keep draining until the deadline.
            }
            Either::Left((Err(_), _)) => return None, // channel closed
            Either::Right(_) => return None,           // deadline
        }
    }
}

/// Bring up DataBridge in a headless browser, subscribe to Binance BTCUSDT
/// trades through the public bridge API, assert at least one TradeUpdate
/// reaches the broadcast channel within 20s.
#[wasm_bindgen_test]
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
         broken on wasm (station/bridge link, not bubble/UI)"
    );

    if let Some(LiveUpdate::TradeUpdate { price, quantity, timestamp, .. }) = ev {
        web_sys::console::log_1(
            &format!(
                "[wasm_bridge_smoke] OK — first trade: price={price} qty={quantity} ts={timestamp}"
            )
            .into(),
        );
    }
}
