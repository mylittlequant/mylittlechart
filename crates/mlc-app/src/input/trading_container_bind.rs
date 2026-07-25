//! `TradingContainer` full symbol rebind (quality arc W2, 2026-07-25;
//! color-sync group follow, 2026-07-25 header rework).
//!
//! v1 bound a container's hosted DOM/OrderEntry/PositionManager/TradeLog to
//! a (symbol, exchange, account_type) triple exactly once, at spawn or
//! hydration time — switching the source afterward never re-subscribed the
//! feeds or re-bound the trading snapshot. [`ChartApp::rebind_trading_container`]
//! is the one place that now does the whole job; it is called from:
//! (a) the color-picker swatch click when the container joins a color group
//!     (`color_picker.rs`), (b) [`ChartApp::apply_key_to_panels_in_group`]
//!     whenever any member chart of the container's group changes symbol,
//!     and (c) [`ChartApp::poll_trading_container_group_follow`], run once
//!     per tick as a safety net — there is no `ActiveChartChanged` event
//!     anywhere in the codebase to hook into (checked), so this is a cheap
//!     per-tick compare instead of an event reaction.
//!
//! Owner directive (2026-07-25): the Auto/Pinned `SymbolSource` toggle is
//! LEGACY and gone from the Trading tab. Symbol sync works ONLY through
//! color-sync groups now — a container that is a member of a real
//! (non-`auto_created`) color-tag group follows THAT group's symbol; a
//! container with no group (or only its own private auto-created group)
//! is edited by hand and is never touched automatically.

use crate::ChartApp;

impl ChartApp {
    /// Per-tick color-group follow for every live `TradingContainer`: when a
    /// container is a member of a real (non-`auto_created`) color-tag group
    /// and that group's resolved instrument has moved since the container's
    /// last bind, this re-runs the full [`Self::rebind_trading_container`]
    /// pipeline. Containers with no group membership — or only their own
    /// private auto-created group — are skipped entirely: no group means
    /// manual symbol editing only.
    pub(crate) fn poll_trading_container_group_follow(&mut self) {
        if self.panels_store.trading_container.is_empty() {
            return;
        }

        use mlc_core::ids::SyncMemberId;

        let due: Vec<(mlc_core::state::PanelId, mlc_app_panels::trading::ResolvedSymbol)> = {
            let tags = self.core.tags();
            let bubbles = self.core.bubbles();
            self.panels_store.trading_container.iter()
                .filter_map(|(pid, s)| {
                    let gid = tags.tag_for_member(SyncMemberId::Panel(pid.0))?;
                    let tag = tags.get(gid)?;
                    if tag.auto_created {
                        // Private/no group — manual editing only, never touched here.
                        return None;
                    }
                    let instrument = bubbles.instrument(gid)?;
                    let resolved = mlc_app_panels::trading::ResolvedSymbol {
                        symbol: instrument.address.symbol,
                        exchange: instrument.address.exchange,
                        account_type: instrument.address.account_type.as_label().to_string(),
                    };
                    if resolved.symbol.is_empty() || resolved.exchange.is_empty() {
                        return None;
                    }
                    if s.symbol != resolved.symbol
                        || s.exchange != resolved.exchange
                        || s.account_type != resolved.account_type
                    {
                        Some((*pid, resolved))
                    } else {
                        None
                    }
                })
                .collect()
        };

        for (pid, resolved) in due {
            self.rebind_trading_container(pid, resolved);
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

    /// Commit a symbol-search pick to a `TradingContainer` (workspace-panel
    /// arc, Stage 3): the search modal's commit handler routes here instead
    /// of [`Self::apply_symbol`] whenever [`crate::focus::WorkspaceFocus`]
    /// is `TradingPanel(pid)`.
    ///
    /// Mirrors what a manual chart symbol pick does
    /// (`sync_group::propagate_symbol_to_sync_group`'s group-update tail):
    /// if the container is a member of a real (non-`auto_created`) color
    /// group, a manual pick is not a private edit — the WHOLE group's
    /// canonical instrument moves and [`Self::apply_key_to_panels_in_group`]
    /// re-derives every member (charts and sibling containers alike) from
    /// it, exactly like it does when a grouped chart's symbol changes.
    /// A container with no group — or only its own private auto-created
    /// group — has nobody to keep in sync, so it is rebound directly.
    pub(crate) fn commit_symbol_search_to_trading_panel(
        &mut self,
        pid: mlc_core::state::PanelId,
        symbol: &str,
        exchange: &str,
        account_type: &str,
    ) {
        use mlc_core::ids::SyncMemberId;

        let real_group = {
            let tags = self.core.tags();
            tags.tag_for_member(SyncMemberId::Panel(pid.0))
                .filter(|&gid| tags.get(gid).map(|t| !t.auto_created).unwrap_or(false))
        };

        if let Some(gid) = real_group {
            self.core.bubbles_mut().set_instrument(
                gid, symbol.to_string(), exchange.to_string(), account_type.to_string(),
            );
            self.apply_key_to_panels_in_group(gid);
        } else {
            self.rebind_trading_container(pid, mlc_app_panels::trading::ResolvedSymbol {
                symbol: symbol.to_string(),
                exchange: exchange.to_string(),
                account_type: account_type.to_string(),
            });
        }
    }
}
