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
            state.metrics.set_snapshot(snap.clone());
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

    /// Open (or refocus) the Trading Container header-tab's inline symbol
    /// editor for `pid` (trading-header-symbol-edit arc, 2026-07-25) — the
    /// sidebar-hosted counterpart of `open_symbol_quick_dropdown` (topbar).
    /// Seeds `SYMBOL_QUICK_SEARCH` with the container's own composite key
    /// (`"SYMBOL:EXCH:AT"`, exchange UPPERCASE — same seed grammar
    /// `symbol_quick_seed_text` uses) and SELECTS ALL of it (owner spec:
    /// clicking the tab text opens an immediately-replaceable editable
    /// field), unlike the topbar editor's cursor-at-end/no-selection seed.
    ///
    /// No-ops if `pid` is already the live edit target — a second click on
    /// the tab or the search icon while typing must not reset the user's
    /// in-progress query. Also sets this panel as the workspace focus,
    /// mirroring every other click-to-focus gesture on the Trading tab.
    pub(crate) fn open_trading_symbol_quick_edit(&mut self, pid: mlc_core::state::PanelId) {
        if matches!(
            self.symbol_quick_edit.as_ref().map(|e| e.target),
            Some(crate::SymbolQuickEditTarget::TradingPanel(p)) if p == pid
        ) {
            return;
        }
        let Some(tc) = self.panels_store.trading_container.get(&pid) else { return };
        let seed = if tc.exchange.is_empty() {
            tc.symbol.clone()
        } else {
            format!("{}:{}:{}", tc.symbol, tc.exchange.to_uppercase(), tc.account_type)
        };
        let cursor = seed.chars().count();
        self.workspace_focus = crate::focus::WorkspaceFocus::TradingPanel(pid);
        self.symbol_quick_edit = Some(crate::SymbolQuickEdit {
            target: crate::SymbolQuickEditTarget::TradingPanel(pid),
            text: seed.clone(),
            cursor,
            selection: Some((0, cursor)),
        });
        let q_id = uzor::WidgetId::from(crate::text_input::SYMBOL_QUICK_SEARCH);
        let mut ic = self.input_coordinator.borrow_mut();
        let tf = ic.text_fields_mut();
        tf.set_text(&q_id, &seed);
        tf.begin_edit(&q_id);
        tf.focus(crate::text_input::SYMBOL_QUICK_SEARCH);
        // Select-all (owner spec) — `set_text`/`begin_edit` leave the store
        // at cursor-at-end/no-selection; overwrite directly the same way
        // uzor's own `KeyPress::SelectAll` handler does.
        if let Some(fs) = tf.field_state_mut(&q_id) {
            fs.selection_start = Some(0);
            fs.cursor = cursor;
        }
    }

    /// The single hosted panel of a data-section sidebar tab (global-docking
    /// Stage B: sections host exactly one panel; the one-leaf dock tree
    /// underneath is an implementation detail pending demolition).
    pub(crate) fn section_panel_for(
        &self,
        panel: mlc_app_sidebar_content::state::RightSidebarPanel,
    ) -> Option<mlc_core::state::Panel> {
        use mlc_app_sidebar_content::state::RightSidebarPanel as RSP;
        if panel == RSP::SlotTrading {
            return None;
        }
        let idx = panel.slot_index()? as usize;
        self.sidebar_state.slot_dockings.get(idx)?
            .inner().tree().leaves().iter()
            .find_map(|l| l.active_panel().cloned())
    }

    /// (symbol, exchange, account_type) of a data-section panel.
    pub(crate) fn section_symbol_triplet(&self, item: &mlc_core::state::Panel) -> (String, String, String) {
        use mlc_core::state::Panel;
        match item {
            Panel::Dom(id) => self.panels_store.dom.get(id)
                .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
            Panel::TradeTape(id) => self.panels_store.trade_tape.get(id)
                .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
            Panel::L2Tape(id) => self.panels_store.l2_tape.get(id)
                .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
            Panel::BigTrades(id) => self.panels_store.big_trades.get(id)
                .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())),
            _ => None,
        }
        .unwrap_or_default()
    }

    /// Open (or refocus) a data-section header's inline symbol editor —
    /// the section counterpart of [`Self::open_trading_symbol_quick_edit`],
    /// same seed grammar and select-all contract.
    pub(crate) fn open_section_symbol_quick_edit(&mut self, item: &mlc_core::state::Panel) {
        let pid = item.panel_id();
        if matches!(
            self.symbol_quick_edit.as_ref().map(|e| e.target),
            Some(crate::SymbolQuickEditTarget::SectionPanel(p)) if p == pid
        ) {
            return;
        }
        let (sym, ex, at) = self.section_symbol_triplet(item);
        let seed = if ex.is_empty() {
            sym.clone()
        } else {
            format!("{}:{}:{}", sym, ex.to_uppercase(), at)
        };
        let cursor = seed.chars().count();
        self.symbol_quick_edit = Some(crate::SymbolQuickEdit {
            target: crate::SymbolQuickEditTarget::SectionPanel(pid),
            text: seed.clone(),
            cursor,
            selection: Some((0, cursor)),
        });
        let q_id = uzor::WidgetId::from(crate::text_input::SYMBOL_QUICK_SEARCH);
        let mut ic = self.input_coordinator.borrow_mut();
        let tf = ic.text_fields_mut();
        tf.set_text(&q_id, &seed);
        tf.begin_edit(&q_id);
        tf.focus(crate::text_input::SYMBOL_QUICK_SEARCH);
        if let Some(fs) = tf.field_state_mut(&q_id) {
            fs.selection_start = Some(0);
            fs.cursor = cursor;
        }
    }

    /// Commit a section-header symbol search: write the instrument on the
    /// panel's tag group (every spawned market-data panel is a TagManager
    /// member — auto or real) and run the group apply-key rebind. Real
    /// groups propagate to all members; the private auto group rebinds just
    /// this panel.
    pub(crate) fn commit_symbol_search_to_section_panel(
        &mut self,
        pid: mlc_core::state::PanelId,
        symbol: &str,
        exchange: &str,
        account_type: &str,
    ) {
        use mlc_core::ids::SyncMemberId;
        let gid = self.core.tags().tag_for_member(SyncMemberId::Panel(pid.0));
        if let Some(gid) = gid {
            self.core.bubbles_mut().set_instrument(
                gid, symbol.to_string(), exchange.to_string(), account_type.to_string(),
            );
            self.apply_key_to_panels_in_group(gid);
        } else {
            eprintln!("[ChartApp] section symbol commit: panel {} has no tag group — ignored", pid.0);
        }
        self.autosave_snapshot();
    }

    /// Feed-handle self-heal (owner defect 2026-07-30): panels that read a
    /// shared series (DOM book/trades, Trade Tape trades, Big Trades
    /// trades) but hold `None` while their symbol is concrete get their
    /// subscribe retried once per tick until the handle lands. Covers the
    /// cold-boot hydration race and any historical dead-handle state — the
    /// group rebind alone only repairs on a key CHANGE.
    pub(crate) fn repair_panel_feed_handles(&mut self) {
        enum Need {
            DomBook(mlc_core::state::PanelId),
            DomTrades(mlc_core::state::PanelId),
            TapeTrades(mlc_core::state::PanelId),
            BigTrades(mlc_core::state::PanelId),
        }
        let mut needs: Vec<(Need, String, String, String)> = Vec::new();
        for (pid, s) in &self.panels_store.dom {
            if !s.symbol.is_empty() && !s.exchange.is_empty() {
                if s.shared_orderbook.is_none() {
                    needs.push((Need::DomBook(*pid), s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
                }
                if s.shared_trades.is_none() {
                    needs.push((Need::DomTrades(*pid), s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
                }
            }
        }
        for (pid, s) in &self.panels_store.trade_tape {
            if !s.symbol.is_empty() && !s.exchange.is_empty() && s.shared_trades.is_none() {
                needs.push((Need::TapeTrades(*pid), s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
            }
        }
        for (pid, s) in &self.panels_store.big_trades {
            if !s.symbol.is_empty() && !s.exchange.is_empty() && s.shared_trades.is_none() {
                needs.push((Need::BigTrades(*pid), s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
            }
        }
        for (need, ex, sym, at_label) in needs {
            let Some(eid) = digdigdig3::ExchangeId::from_str(&ex) else { continue };
            let at = crate::account_type_from_label(&at_label);
            match need {
                Need::DomBook(pid) => {
                    let handle = self.core.subscribe_orderbook(eid, &sym, at);
                    if let Some(s) = self.panels_store.dom.get_mut(&pid) {
                        s.shared_orderbook = handle;
                    }
                }
                Need::DomTrades(pid) => {
                    let handle = self.core.subscribe_trades(eid, &sym, at);
                    if let Some(s) = self.panels_store.dom.get_mut(&pid) {
                        s.shared_trades = handle;
                    }
                }
                Need::TapeTrades(pid) => {
                    let handle = self.core.subscribe_trades(eid, &sym, at);
                    if let Some(s) = self.panels_store.trade_tape.get_mut(&pid) {
                        s.shared_trades = handle;
                        s.last_seen_ts_ms = 0;
                    }
                }
                Need::BigTrades(pid) => {
                    let handle = self.core.subscribe_trades(eid, &sym, at);
                    if let Some(s) = self.panels_store.big_trades.get_mut(&pid) {
                        s.shared_trades = handle;
                        s.last_seen_ts_ms = 0;
                    }
                }
            }
        }

        // Multi-DOM book handles (v2: per-exchange station trackers, no
        // delta events). Provisioned only while multi mode is ON; cleared
        // on symbol rebind / section reopen so this refills them fresh.
        let mut multi_needs: Vec<(mlc_core::state::PanelId, usize, String, String)> = Vec::new();
        for (pid, s) in &self.panels_store.dom {
            if !s.multi_mode || s.multi.symbol.is_empty() {
                continue;
            }
            for (i, ex) in s.multi.config.exchanges.iter().enumerate() {
                if s.multi.handles.get(i).map_or(true, |h| h.is_none()) {
                    multi_needs.push((*pid, i, ex.clone(), s.multi.symbol.clone()));
                }
            }
        }
        for (pid, i, ex, sym) in multi_needs {
            let Some(eid) = digdigdig3::ExchangeId::from_str(&ex) else { continue };
            let handle = self.core.subscribe_orderbook(eid, &sym, digdigdig3::AccountType::Spot);
            if let Some(s) = self.panels_store.dom.get_mut(&pid) {
                if s.multi.handles.len() <= i {
                    s.multi.handles.resize_with(i + 1, || None);
                }
                s.multi.handles[i] = handle;
            }
        }
    }

    /// Set the trading container's timeframe (embedded header TF system).
    /// Display-name grammar matches the topbar TF rows (`tf_{name}` →
    /// `name`, e.g. "1m"/"1H"). Stored + persisted on the container; TF
    /// consumers inside the container (footprint / embedded chart, future)
    /// read it from there.
    pub(crate) fn set_trading_container_timeframe(&mut self, pid: mlc_core::state::PanelId, name: &str) {
        if let Some(tc) = self.panels_store.trading_container.get_mut(&pid) {
            tc.timeframe = name.to_string();
        }
        self.autosave_snapshot();
    }
}
