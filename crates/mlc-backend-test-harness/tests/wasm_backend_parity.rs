//! wasm32 mirror of the native backend harness scenarios.
//!
//! Every `body_*` function in `mlc_backend_test_harness::scenarios` is reused
//! verbatim — the only difference is how the harness is constructed
//! (`BackendHarness::fresh_async()` on wasm; `BackendHarness::fresh()` on
//! native, via the legacy sync wrapper).
//!
//! Run with:
//!   cargo test --target wasm32-unknown-unknown \
//!     -p mylittlechart-backend-test-harness --test wasm_backend_parity \
//!     -- --nocapture

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use mlc_backend_test_harness::{scenarios, BackendHarness};

async fn fresh() -> BackendHarness { BackendHarness::fresh_async().await }

fn unwrap(r: mlc_backend_test_harness::HarnessResult<()>) {
    if let Err(e) = r {
        panic!("[{}] {}", e.scenario, e.message);
    }
}

// ─── Stream coverage ─────────────────────────────────────────────────────────

#[wasm_bindgen_test]
async fn stream_backfill_complete_writes_bubble() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_backfill_complete_writes_bubble(&mut h));
}

#[wasm_bindgen_test]
async fn stream_scroll_bars_merges_with_live_tail() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_scroll_bars_merges_with_live_tail(&mut h));
}

#[wasm_bindgen_test]
async fn stream_trade_passthrough() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_trade_passthrough(&mut h));
}

#[wasm_bindgen_test]
async fn stream_orderbook_snapshot_ws_emits_event() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_orderbook_snapshot_ws_emits_event(&mut h));
}

#[wasm_bindgen_test]
async fn stream_orderbook_delta_emits_event() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_orderbook_delta_emits_event(&mut h));
}

#[wasm_bindgen_test]
async fn stream_open_interest_passthrough() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_open_interest_passthrough(&mut h));
}

#[wasm_bindgen_test]
async fn stream_funding_rate_passthrough() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_funding_rate_passthrough(&mut h));
}

#[wasm_bindgen_test]
async fn stream_liquidation_event_passthrough() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_liquidation_event_passthrough(&mut h));
}

#[wasm_bindgen_test]
async fn stream_liquidation_bucket_passthrough() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_liquidation_bucket_passthrough(&mut h));
}

#[wasm_bindgen_test]
async fn stream_dom_heatmap_snapshot_passthrough() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_dom_heatmap_snapshot_passthrough(&mut h));
}

// ─── Multi-symbol isolation ──────────────────────────────────────────────────

#[wasm_bindgen_test]
async fn multi_symbol_bars_isolation() {
    let mut h = fresh().await;
    unwrap(scenarios::body_multi_symbol_bars_isolation(&mut h));
}

#[wasm_bindgen_test]
async fn multi_symbol_trade_passthrough_independent() {
    let mut h = fresh().await;
    unwrap(scenarios::body_multi_symbol_trade_passthrough_independent(&mut h));
}

#[wasm_bindgen_test]
async fn multi_symbol_mini_ticker_cache_keys() {
    let mut h = fresh().await;
    unwrap(scenarios::body_multi_symbol_mini_ticker_cache_keys(&mut h));
}

// ─── Derived bars (aggTrades-fueled) ─────────────────────────────────────────
// Station-derived wipe (unified-data-plane Э5a): renko/pnf/kagi/tpo/
// three_line_break/footprint now fold client-side — CVD is the one overlay
// still reaching the bridge as a real station stream.

#[wasm_bindgen_test]
async fn stream_cvd_scalar_writes_bubble() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_cvd_scalar_writes_bubble(&mut h));
}

#[wasm_bindgen_test]
async fn stream_dom_trade_dot_passthrough() {
    let mut h = fresh().await;
    unwrap(scenarios::body_stream_dom_trade_dot_passthrough(&mut h));
}

// ─── Alert delivery on ±0.25 % cross ─────────────────────────────────────────

#[wasm_bindgen_test]
async fn alerts_straddle_fires_on_cross() {
    let mut h = fresh().await;
    unwrap(scenarios::body_alerts_straddle_fires_on_cross(&mut h));
}

#[wasm_bindgen_test]
async fn alerts_straddle_fires_on_drop() {
    let mut h = fresh().await;
    unwrap(scenarios::body_alerts_straddle_fires_on_drop(&mut h));
}
