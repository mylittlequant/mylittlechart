//! Live bars from one exchange through `mlc-backend-data-bridge` (feature `live`):
//! Binance spot BTCUSDT. The bridge runs on its own tokio runtime thread natively
//! and on the page's event loop in the browser; bars come back over a channel.
//! When nothing arrives within [`TIMEOUT_S`] the app shows demo data, labelled so.

use std::sync::mpsc;
use std::sync::Arc;

use digdigdig3::{AccountType, ExchangeId};
use mlc_backend_data_bridge::{DataBridge, LiveUpdate};
use mlc_core::state::Timeframe;

use crate::source::{Feed, Source};

/// Seconds to wait for the first bars before falling back to demo data.
pub const TIMEOUT_S: f64 = 12.0;

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Binance BTCUSDT over the data bridge.
pub struct LiveSource {
    req: tokio::sync::mpsc::UnboundedSender<String>,
    req_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    rx: mpsc::Receiver<Feed>,
    tx: mpsc::Sender<Feed>,
    wake: Option<Wake>,
    started: bool,
}

impl Default for LiveSource {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveSource {
    pub fn new() -> Self {
        let (req, req_rx) = tokio::sync::mpsc::unbounded_channel();
        let (tx, rx) = mpsc::channel();
        Self { req, req_rx: Some(req_rx), rx, tx, wake: None, started: false }
    }

    fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        let (Some(req_rx), tx) = (self.req_rx.take(), self.tx.clone()) else { return };
        let wake = self.wake.clone().unwrap_or_else(|| Arc::new(|| {}));
        #[cfg(not(target_arch = "wasm32"))]
        {
            let fallback = tx.clone();
            let spawned = std::thread::Builder::new().name("mlc-live".into()).spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                    Ok(rt) => rt,
                    Err(_) => {
                        let _ = tx.send(Feed::Offline);
                        wake();
                        return;
                    }
                };
                rt.block_on(run(req_rx, tx, wake));
            });
            if spawned.is_err() {
                let _ = fallback.send(Feed::Offline);
            }
        }
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(run(req_rx, tx, wake));
    }
}

async fn run(mut req_rx: tokio::sync::mpsc::UnboundedReceiver<String>, tx: mpsc::Sender<Feed>, wake: Wake) {
    let (bridge, mut live, _ready) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    bridge.ensure_connector(ExchangeId::Binance);
    let mut want: Option<String> = None;
    loop {
        tokio::select! {
            r = req_rx.recv() => {
                let Some(code) = r else { return };
                if let Some(tf) = Timeframe::parse(&code) {
                    bridge.request_bars(ExchangeId::Binance, SYMBOL, &tf, AccountType::Spot, None, None, false);
                    want = Some(code);
                }
            }
            u = live.recv() => {
                let feed = match u {
                    Ok(LiveUpdate::BarsLoaded { symbol, timeframe, bars, .. }) if symbol == SYMBOL && want.as_deref().is_some_and(|w| same_tf(w, &timeframe)) && !bars.is_empty() => {
                        Some(Feed::Bars { tf: want.clone().unwrap_or_default(), bars, live: true })
                    }
                    Ok(LiveUpdate::BarUpdate { symbol, timeframe, bar, .. }) if symbol == SYMBOL && want.as_deref().is_some_and(|w| same_tf(w, &timeframe)) => Some(Feed::Bar(bar)),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    _ => None,
                };
                if let Some(f) = feed {
                    if tx.send(f).is_err() {
                        return;
                    }
                    wake();
                }
            }
        }
    }
}

/// The bridge's symbol for BTC/USDT.
pub const SYMBOL: &str = "BTCUSDT";

fn same_tf(code: &str, name: &str) -> bool {
    code.eq_ignore_ascii_case(name) || Timeframe::parse(code).zip(Timeframe::parse(name)).is_some_and(|(a, b)| a.interval_ms == b.interval_ms)
}

impl Source for LiveSource {
    fn venue(&self) -> &str {
        "Binance"
    }
    fn symbol(&self) -> &str {
        "BTC/USDT"
    }
    fn request(&mut self, code: &str) {
        self.start();
        let _ = self.req.send(code.to_string());
    }
    fn poll(&mut self) -> Vec<Feed> {
        self.rx.try_iter().collect()
    }
    fn set_waker(&mut self, wake: Box<dyn Fn() + Send + Sync>) {
        self.wake = Some(Arc::from(wake));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn timeframe_names_match_across_spellings() {
        assert!(super::same_tf("1h", "1H"));
        assert!(super::same_tf("1h", "60m"));
        assert!(!super::same_tf("1h", "4h"));
    }
}
