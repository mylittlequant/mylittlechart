//! `TradingContainer` full symbol rebind (quality arc W2, 2026-07-25).
//!
//! v1 bound a container's hosted DOM/OrderEntry/PositionManager/TradeLog to
//! a (symbol, exchange, account_type) triple exactly once, at spawn or
//! hydration time — switching the source afterward (or, for a `HyperFocus`
//! container, switching the active chart) never re-subscribed the feeds or
//! re-bound the trading snapshot. [`ChartApp::rebind_trading_container`] is
//! the one place that now does the whole job; it is called from:
//! (a) the overlay source-badge click (`panel_click.rs`), and
//! (b) [`ChartApp::poll_trading_container_hyperfocus_follow`], run once per
//!     tick — there is no `ActiveChartChanged` event anywhere in the
//!     codebase to hook into (checked), so this is a cheap per-tick compare
//!     instead of an event reaction.

use crate::ChartApp;

impl ChartApp {
    /// Per-tick HyperFocus follow for every live `TradingContainer`: when a
    /// container's source mode is `HyperFocus` (the default "follow the
    /// active chart" mode) and this window's active-chart
    /// (symbol, exchange, account_type) has moved since the container's
    /// last bind, this re-runs the full [`Self::rebind_trading_container`]
    /// pipeline.
    pub(crate) fn poll_trading_container_hyperfocus_follow(&mut self) {
        if self.panels_store.trading_container.is_empty() {
            return;
        }

        let active_cid = self.panel_app.panel_grid.active_container_id();
        let Some(cid) = active_cid else { return };
        let active = mlc_app_panels::trading::ResolvedSymbol {
            symbol: self.symbol_for_chart(cid).unwrap_or_default(),
            exchange: self.exchange_for_chart(cid).unwrap_or_default(),
            account_type: self.account_type_for_chart(cid).map(|at| at.to_string()).unwrap_or_default(),
        };
        if active.symbol.is_empty() || active.exchange.is_empty() {
            return;
        }

        let due: Vec<mlc_core::state::PanelId> = self.panels_store.trading_container.iter()
            .filter(|(_, s)| {
                s.source.is_hyper_focus()
                    && (s.symbol != active.symbol
                        || s.exchange != active.exchange
                        || s.account_type != active.account_type)
            })
            .map(|(pid, _)| *pid)
            .collect();

        for pid in due {
            self.rebind_trading_container(pid, active.clone());
        }
    }

    /// Full symbol rebind for a `TradingContainer`: unsubscribes the old
    /// orderbook/trade feeds, restamps the container and every hosted
    /// child to the new `resolved` triple (clearing DOM's per-level
    /// aggregation state — it is keyed to the OLD instrument's tick
    /// buckets), resubscribes the new orderbook/trade feeds, and rebinds
    /// order_entry/position_manager/trade_log to the new (exchange,
    /// account_type) trading snapshot via `snapshot_for`. No-ops when
    /// `resolved` already matches the container's current binding.
    pub(crate) fn rebind_trading_container(
        &mut self,
        pid: mlc_core::state::PanelId,
        resolved: mlc_app_panels::trading::ResolvedSymbol,
    ) {
        let Some((old_symbol, old_exchange, old_account_type)) = self.panels_store.trading_container.get(&pid)
            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
        else {
            return;
        };

        if old_symbol == resolved.symbol
            && old_exchange == resolved.exchange
            && old_account_type == resolved.account_type
        {
            return;
        }

        if !old_symbol.is_empty() {
            if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exchange) {
                let at = crate::account_type_from_label(&old_account_type);
                self.core.unsubscribe_orderbook(eid, &old_symbol, at);
                self.core.unsubscribe_trades(eid, &old_symbol, at);
            }
        }

        let mlc_app_panels::trading::ResolvedSymbol { symbol, exchange, account_type } = resolved;

        let resolved_tick_size = self.core.exchange_symbols()
            .tick_size_for(&exchange, &symbol)
            .filter(|t| *t > 0.0)
            .unwrap_or(0.01);

        let Some(state) = self.panels_store.trading_container.get_mut(&pid) else { return };
        state.symbol = symbol.clone();
        state.exchange = exchange.clone();
        state.account_type = account_type.clone();
        state.tick_size = resolved_tick_size;
        state.dom.symbol = symbol.clone();
        state.dom.exchange = exchange.clone();
        state.dom.account_type = account_type.clone();
        state.dom.tick_size = resolved_tick_size;
        // Clear per-level DOM aggregation — its tick buckets, cursors, and
        // book-derived prices are meaningless for a different instrument
        // (mirrors the hydration arm's fresh-subscribe reset).
        state.dom.shared_orderbook = None;
        state.dom.shared_trades = None;
        state.dom.last_seen_update_id = None;
        state.dom.last_seen_ts_ms = 0;
        state.dom.level_qty_bid.clear();
        state.dom.level_qty_ask.clear();
        state.dom.volume_by_price.clear();
        state.dom.trade_volume_by_price.clear();
        state.dom.user_orders.clear();
        state.dom.recent_fills.clear();
        state.dom.market_price = 0.0;
        state.dom.center_price = 0.0;
        state.dom.best_bid_price = None;
        state.dom.best_ask_price = None;
        state.dom.auto_center = true;
        state.order_entry.symbol = symbol.clone();
        state.order_entry.exchange = exchange.clone();
        state.order_entry.account_type = account_type.clone();

        if symbol.is_empty() {
            return;
        }
        let Some(eid) = digdigdig3::ExchangeId::from_str(&exchange) else { return };
        let at = crate::account_type_from_label(&account_type);

        let shared_orderbook = self.core.subscribe_orderbook(eid, &symbol, at);
        let shared_trades = self.core.subscribe_trades(eid, &symbol, at);
        let snapshot = self.core.trading_mut().map(|mut tm| tm.snapshot_for(eid, at));

        let Some(state) = self.panels_store.trading_container.get_mut(&pid) else { return };
        state.dom.shared_orderbook = shared_orderbook;
        state.dom.shared_trades = shared_trades;
        if let Some(snap) = snapshot {
            state.order_entry.set_snapshot(snap.clone());
            state.position_manager.set_snapshot(snap.clone());
            state.trade_log.set_snapshot(snap.clone());
            state.orders_log.set_snapshot(snap.clone());
            state.risk_calculator.set_snapshot(snap);
        }
    }
}
