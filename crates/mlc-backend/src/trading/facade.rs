//! `Trading` / `TradingMut` — the only doctrinally-sanctioned access path to
//! `TradingManager` from outside `mlc-backend`.
//!
//! `TradingManager` never touches `BubbleStore` (order/position/session
//! state is keyed by `(ExchangeId, AccountType)`, not by bubble) — the
//! facade holds exactly one guard, no co-acquisition, same leaf shape as
//! `Watchlists`/`WatchlistsMut`.
//!
//! Trading is optional per `BackendCore` (`None` when trading is disabled),
//! so both facade constructors return `Option`.
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A.

use std::sync::MutexGuard;

use mlc_backend_data_bridge::LiveUpdate;
use mlc_backend_trading_engine::{AccountType, ExchangeId, SharedTradingSnapshot};

use super::manager::TradingManager;

/// Read-only facade over `TradingManager`.
pub struct Trading<'a> {
    mgr: MutexGuard<'a, TradingManager>,
}

impl<'a> Trading<'a> {
    pub(crate) fn new(mgr: MutexGuard<'a, TradingManager>) -> Self {
        Self { mgr }
    }

    /// Shared handle to the published trading snapshot (orders, positions,
    /// balances, PnL). Readers hold this independently of the manager lock.
    pub fn snapshot(&self) -> SharedTradingSnapshot {
        self.mgr.snapshot()
    }

    /// Data directory used for trading config persistence.
    pub fn data_dir(&self) -> &std::path::Path {
        self.mgr.data_dir()
    }
}

/// Mutable facade over `TradingManager`.
pub struct TradingMut<'a> {
    mgr: MutexGuard<'a, TradingManager>,
}

impl<'a> TradingMut<'a> {
    pub(crate) fn new(mgr: MutexGuard<'a, TradingManager>) -> Self {
        Self { mgr }
    }

    /// Shared handle to the published trading snapshot.
    pub fn snapshot(&self) -> SharedTradingSnapshot {
        self.mgr.snapshot()
    }

    /// Place an order (routes to paper engine or live exchange API
    /// depending on the session's paper-mode config).
    pub fn place_order(
        &mut self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        req: mlc_backend_trading_engine::OrderRequest,
    ) -> mlc_backend_trading_engine::TradingResult<()> {
        self.mgr.place_order(exchange_id, account_type, req)
    }

    /// Cancel an order (paper or live).
    pub fn cancel_order(
        &mut self,
        exchange_id: ExchangeId,
        account_type: AccountType,
        req: mlc_backend_trading_engine::CancelRequest,
    ) -> mlc_backend_trading_engine::TradingResult<()> {
        self.mgr.cancel_order(exchange_id, account_type, req)
    }

    /// Refresh cached balances for a session (spawns an async fetch).
    pub fn refresh_balances(&mut self, exchange_id: ExchangeId, account_type: AccountType) {
        self.mgr.refresh_balances(exchange_id, account_type);
    }

    /// Refresh positions for a session (spawns an async fetch).
    pub fn refresh_positions(&mut self, exchange_id: ExchangeId, account_type: AccountType) {
        self.mgr.refresh_positions(exchange_id, account_type);
    }

    /// Drain pending task results + live updates, tick paper engines, and
    /// republish the snapshot for every active session. Driven once per
    /// frame from `ChartApp`'s tick loop.
    pub fn tick(&mut self, live_updates: &[LiveUpdate]) {
        self.mgr.tick(live_updates);
    }

    /// Register sessions + capabilities for a freshly connected exchange,
    /// spinning up paper engines where the config calls for paper mode.
    pub fn on_connector_ready(&mut self, exchange_id: ExchangeId) {
        self.mgr.on_connector_ready(exchange_id);
    }

    /// Data directory used for trading config persistence.
    pub fn data_dir(&self) -> &std::path::Path {
        self.mgr.data_dir()
    }
}
