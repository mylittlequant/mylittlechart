//! Coverage for `BackendCore::alert_still_needs_trades` — the single
//! decision `unsubscribe_trades` (`crates/mlc-backend/src/lib.rs`) consults
//! before letting a feed teardown reach the bridge (owner-review-2026-09-23,
//! `docs/mlc/research/owner-review-2026-09-23/
//! alert-bubbles-on-cold-apply.md` item 2). Constructs a real `BackendCore`
//! (no `ChartApp` — `DataBridge::new()` dials no connector, same pattern
//! `ensure_alert_bubble_tests` uses below) and drives it through the public
//! `alerts_mut().add`/`remove` facade so the release-guard mirror
//! (`has_active_alerts`) is set the same way production ever sets it —
//! `AlertManager::resync_has_alerts`, not a direct store poke.

use std::sync::Arc;

use digdigdig3::{AccountType, ExchangeId};
use mlc_core::alert::{AlertSource, AlertSpec};
use mlc_core::state::Timeframe;
use mlc_backend_data_bridge::DataBridge;

use crate::BackendCore;

fn build_core() -> Arc<BackendCore> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    let (bridge, _live_rx, _connector_ready_rx) = rt.block_on(DataBridge::new());
    BackendCore::new(Arc::new(bridge), None)
}

fn price_spec(exchange: &str, symbol: &str, price: f64) -> AlertSpec {
    AlertSpec {
        source: AlertSource::Price { symbol: symbol.to_string() },
        name: "guard-test".to_string(),
        price,
        exchange: exchange.to_string(),
        ..AlertSpec::price_default()
    }
}

#[test]
fn no_bubble_at_all_needs_no_guard() {
    let core = build_core();
    assert!(!core.alert_still_needs_trades(ExchangeId::Binance, "BTCUSDT", AccountType::Spot));
}

#[test]
fn a_bubble_with_no_alert_needs_no_guard() {
    let core = build_core();
    let bid = core.bubbles_mut().create_bubble("BTCUSDT".to_string(), Timeframe::h1());
    core.bubbles_mut().set_bubble_instrument(bid, "binance".to_string(), "spot".to_string());

    assert!(!core.alert_still_needs_trades(ExchangeId::Binance, "BTCUSDT", AccountType::Spot));
}

#[test]
fn an_active_alert_flips_the_guard_on_and_removal_flips_it_back_off() {
    let core = build_core();
    let bid = core.bubbles_mut().create_bubble("BTCUSDT".to_string(), Timeframe::h1());
    core.bubbles_mut().set_bubble_instrument(bid, "binance".to_string(), "spot".to_string());
    assert!(!core.alert_still_needs_trades(ExchangeId::Binance, "BTCUSDT", AccountType::Spot));

    let alert_id = core.alerts_mut().add(bid, price_spec("binance", "BTCUSDT", 100.0));
    assert!(
        core.alert_still_needs_trades(ExchangeId::Binance, "BTCUSDT", AccountType::Spot),
        "an active alert on this exact instrument must hold the guard on"
    );

    // A DIFFERENT instrument must not be affected by this bubble's alert.
    assert!(!core.alert_still_needs_trades(ExchangeId::Binance, "ETHUSDT", AccountType::Spot));
    assert!(!core.alert_still_needs_trades(ExchangeId::Bybit, "BTCUSDT", AccountType::Spot));

    assert!(core.alerts_mut().remove(bid, alert_id), "the alert must be found and removed");
    assert!(
        !core.alert_still_needs_trades(ExchangeId::Binance, "BTCUSDT", AccountType::Spot),
        "removing the only alert on this instrument must release the guard"
    );
}

#[test]
fn unsubscribe_trades_runs_end_to_end_through_the_public_method_without_disturbing_the_guard() {
    // Drives the REAL public entry point (not `alert_still_needs_trades`
    // directly) — `DataBridge::unsubscribe_trades`'s own internal state is
    // private to a sibling crate and unobservable from here (see
    // `build_core`'s doc: this harness dials no connector, so there is no
    // live forwarder to begin with either way). What IS observable and
    // worth pinning: the call must complete without panicking whether the
    // guard is on or off, and it must never itself mutate the
    // `has_active_alerts` mirror it only ever reads.
    let core = build_core();
    let bid = core.bubbles_mut().create_bubble("BTCUSDT".to_string(), Timeframe::h1());
    core.bubbles_mut().set_bubble_instrument(bid, "binance".to_string(), "spot".to_string());
    core.alerts_mut().add(bid, price_spec("binance", "BTCUSDT", 100.0));

    core.unsubscribe_trades(ExchangeId::Binance, "BTCUSDT", AccountType::Spot);
    assert!(
        core.bubbles().with_bubble(bid, |b| b.has_active_alerts).unwrap_or(false),
        "unsubscribe_trades must not mutate the has_active_alerts mirror itself"
    );

    // A genuinely unrelated instrument (guard off) must also run clean.
    core.unsubscribe_trades(ExchangeId::Binance, "ETHUSDT", AccountType::Spot);
}
