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
                .map(|s| if s.multi_mode {
                    // Multi-DOM: the header shows only the base SYMBOL —
                    // exchanges are the roster's own concern (owner spec).
                    (s.multi.symbol.clone(), String::new(), String::new())
                } else {
                    (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())
                }),
            Panel::TradeTape(id) => self.panels_store.trade_tape.get(id)
                .map(|s| if s.multi_mode {
                    (s.multi.symbol.clone(), String::new(), String::new())
                } else {
                    (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())
                }),
            Panel::L2Tape(id) => self.panels_store.l2_tape.get(id)
                .map(|s| if s.multi_mode {
                    (s.multi.symbol.clone(), String::new(), String::new())
                } else {
                    (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())
                }),
            Panel::BigTrades(id) => self.panels_store.big_trades.get(id)
                .map(|s| if s.multi_mode {
                    (s.multi.symbol.clone(), String::new(), String::new())
                } else {
                    (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())
                }),
            // LIQ is all-venue by construction — header shows base symbol.
            Panel::LiqTape(id) => self.panels_store.liq_tape.get(id)
                .map(|s| (s.symbol.clone(), String::new(), String::new())),
            _ => None,
        }
        .unwrap_or_default()
    }

    /// Open (or refocus) a data-section header's inline symbol editor —
    /// the section counterpart of [`Self::open_trading_symbol_quick_edit`],
    /// same seed grammar and select-all contract. A Dom section in multi
    /// mode opens the Multi-DOM roster picker target instead (result rows
    /// toggle exchange columns; no tag-group coupling).
    pub(crate) fn open_section_symbol_quick_edit(&mut self, item: &mlc_core::state::Panel) {
        use mlc_core::state::Panel;
        let pid = item.panel_id();
        let multi_dom = matches!(item, Panel::Dom(id)
            if self.panels_store.dom.get(id).map_or(false, |s| s.multi_mode));
        // Multi-tape sections open the SAME roster-picker flow (M/TT ·
        // M/L2 · M/BT, owner 2026-07-31).
        let multi_tape = match item {
            Panel::TradeTape(id) => self.panels_store.trade_tape.get(id).map_or(false, |s| s.multi_mode),
            Panel::L2Tape(id) => self.panels_store.l2_tape.get(id).map_or(false, |s| s.multi_mode),
            Panel::BigTrades(id) => self.panels_store.big_trades.get(id).map_or(false, |s| s.multi_mode),
            // LIQ: roster editing only in the explicit Multi mode (Agg owns
            // the full-venue roster itself).
            Panel::LiqTape(id) => self
                .panels_store
                .liq_tape
                .get(id)
                .map_or(false, |s| s.multi_mode && !s.agg_mode),
            _ => false,
        };
        let target = if multi_dom {
            crate::SymbolQuickEditTarget::MultiDomRoster(pid)
        } else if multi_tape {
            crate::SymbolQuickEditTarget::MultiTapeRoster(pid)
        } else {
            crate::SymbolQuickEditTarget::SectionPanel(pid)
        };
        if self.symbol_quick_edit.as_ref().map(|e| e.target) == Some(target) {
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
            target,
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

    /// Subscribe every multi roster entry ("exchange[:AT]") for `sym` —
    /// ensures the connector, resolves the AT label, idempotent per entry.
    pub(crate) fn subscribe_multi_roster(&mut self, sym: &str, entries: &[String]) {
        use mlc_app_panels::trading::order_flow::multi_dom;
        if sym.is_empty() {
            return;
        }
        for e in entries {
            let ex_slug = multi_dom::col_exchange(e).to_string();
            let at_label = multi_dom::col_at(e).to_string();
            if let Some(eid) = digdigdig3::ExchangeId::from_str(&ex_slug) {
                self.core.ensure_connector(eid);
                let at = crate::account_type_from_label(&at_label);
                let _ = self.core.subscribe_orderbook(eid, sym, at);
            }
        }
    }

    /// Multi-DOM roster picker commit (owner spec 2026-07-30): a dropdown
    /// row click on the SAME base symbol toggles that (exchange, AT) column;
    /// a row with a DIFFERENT symbol switches the base symbol (books and
    /// handles reset, roster kept, clicked column ensured present).
    /// No chart apply, no tag-group coupling — the roster is the Multi-DOM's
    /// own concern.
    pub(crate) fn toggle_multi_dom_roster(
        &mut self,
        pid: mlc_core::state::PanelId,
        symbol: &str,
        exchange_id: &str,
        account_type: &str,
    ) {
        use mlc_app_panels::trading::order_flow::multi_dom;
        let entry = multi_dom::col_entry(exchange_id, account_type);
        let subs: Option<(String, Vec<String>)> = {
            let Some(s) = self.panels_store.dom.get_mut(&pid) else { return };
            if !s.multi_mode || exchange_id.is_empty() {
                None
            } else if !s.multi.symbol.eq_ignore_ascii_case(symbol) {
                // Base symbol switch: fresh books/handles for the new key.
                s.multi.symbol = symbol.to_string();
                for (b, a) in s.multi.books_by_exchange.iter_mut() {
                    b.clear();
                    a.clear();
                }
                s.multi.handles.clear();
                s.multi.mid_price = 0.0;
                s.multi.add_exchange(entry);
                Some((s.multi.symbol.clone(), s.multi.config.exchanges.clone()))
            } else if s.multi.config.exchanges.iter()
                .any(|e| multi_dom::col_matches(e, exchange_id, account_type))
            {
                // Same base, column present — remove it.
                s.multi.remove_exchange(&entry);
                None
            } else {
                // Same base, new column — add + subscribe.
                s.multi.add_exchange(entry.clone());
                Some((s.multi.symbol.clone(), vec![entry]))
            }
        };
        if let Some((sym, entries)) = subs {
            if !sym.is_empty() {
                for e in &entries {
                    let ex_slug = multi_dom::col_exchange(e).to_string();
                    let at_label = multi_dom::col_at(e).to_string();
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&ex_slug) {
                        self.core.ensure_connector(eid);
                        let at = crate::account_type_from_label(&at_label);
                        let _ = self.core.subscribe_orderbook(eid, &sym, at);
                    }
                }
            }
        }
        self.sidebar_data_dirty = true;
        self.autosave_snapshot();
    }

    /// Subscribe multi-tape roster entries: trades feed for TT/BT, book
    /// for L2 (`book = true`). Same idempotent contract as
    /// [`Self::subscribe_multi_roster`].
    pub(crate) fn subscribe_multi_tape_roster(
        &mut self,
        sym: &str,
        entries: &[String],
        book: bool,
    ) {
        use mlc_app_panels::trading::order_flow::multi_dom;
        if sym.is_empty() {
            return;
        }
        for e in entries {
            let ex_slug = multi_dom::col_exchange(e).to_string();
            let at_label = multi_dom::col_at(e).to_string();
            if let Some(eid) = digdigdig3::ExchangeId::from_str(&ex_slug) {
                self.core.ensure_connector(eid);
                let at = crate::account_type_from_label(&at_label);
                if book {
                    let _ = self.core.subscribe_orderbook(eid, sym, at);
                } else {
                    let _ = self.core.subscribe_trades(eid, sym, at);
                }
            }
        }
    }

    /// Multi-tape roster picker commit (M/TT · M/L2 · M/BT, owner
    /// 2026-07-31) — same contract as [`Self::toggle_multi_dom_roster`]:
    /// same-symbol row toggles that venue, different-symbol row switches
    /// the base symbol (handles reset, roster kept). The panel id resolves
    /// to whichever tape store holds it.
    pub(crate) fn toggle_multi_tape_roster(
        &mut self,
        pid: mlc_core::state::PanelId,
        symbol: &str,
        exchange_id: &str,
        account_type: &str,
    ) {
        use mlc_app_panels::trading::order_flow::multi_dom;
        use mlc_app_panels::trading::order_flow::multi_tape::MultiTapeState;
        if exchange_id.is_empty() {
            return;
        }
        let entry = multi_dom::col_entry(exchange_id, account_type);
        // One toggle body shared across the three stores.
        fn toggle(
            m: &mut MultiTapeState,
            symbol: &str,
            exchange_id: &str,
            account_type: &str,
            entry: &str,
        ) -> Option<(String, Vec<String>)> {
            if !m.symbol.eq_ignore_ascii_case(symbol) {
                let sym = symbol.to_string();
                m.reset_for_symbol(&sym);
                m.add_entry(entry);
                Some((m.symbol.clone(), m.entries.clone()))
            } else if m
                .entries
                .iter()
                .any(|e| multi_dom::col_matches(e, exchange_id, account_type))
            {
                m.remove_entry(entry);
                None
            } else {
                m.add_entry(entry);
                Some((m.symbol.clone(), vec![entry.to_string()]))
            }
        }
        // (needs_book, subscription work) per hosting store.
        let work: Option<(bool, Option<(String, Vec<String>)>)> =
            if let Some(s) = self.panels_store.trade_tape.get_mut(&pid) {
                if s.multi_mode {
                    Some((false, toggle(&mut s.multi, symbol, exchange_id, account_type, &entry)))
                } else {
                    None
                }
            } else if let Some(s) = self.panels_store.l2_tape.get_mut(&pid) {
                if s.multi_mode {
                    Some((true, toggle(&mut s.multi, symbol, exchange_id, account_type, &entry)))
                } else {
                    None
                }
            } else if let Some(s) = self.panels_store.big_trades.get_mut(&pid) {
                if s.multi_mode {
                    Some((false, toggle(&mut s.multi, symbol, exchange_id, account_type, &entry)))
                } else {
                    None
                }
            } else if let Some(s) = self.panels_store.liq_tape.get_mut(&pid) {
                if s.multi_mode && !s.agg_mode {
                    // Liquidations are Futures-only — the picked venue is
                    // coerced to its :F feed. Handles land via the feed
                    // self-heal (subscribe_liquidations is repair-driven),
                    // so no trade/book subscription work here.
                    let entry_f = multi_dom::col_entry(exchange_id, "F");
                    let _ = toggle(&mut s.multi, symbol, exchange_id, "F", &entry_f);
                    Some((false, None))
                } else {
                    None
                }
            } else {
                None
            };
        if let Some((book, Some((sym, entries)))) = work {
            self.subscribe_multi_tape_roster(&sym, &entries, book);
        }
        self.sidebar_data_dirty = true;
        self.autosave_snapshot();
    }

    /// Section-picker icon click while the panel is in SINGLE mode (owner
    /// order 2026-07-31: «добавляться он должен сюда, а не куда-то в
    /// другое место»): the clicked venue joins THIS panel in parallel —
    /// the panel flips to multi seeded with its own current venue, the
    /// clicked venue lands via the roster toggle, and the live picker
    /// target morphs to the roster target so further icon clicks keep
    /// editing the roster. Returns `false` when `pid` is not a section
    /// panel (caller falls through to the watchlist star).
    pub(crate) fn add_parallel_section_venue(
        &mut self,
        pid: mlc_core::state::PanelId,
        symbol: &str,
        exchange_id: &str,
        account_type: &str,
    ) -> bool {
        use mlc_app_panels::trading::order_flow::multi_dom;
        if exchange_id.is_empty() {
            return false;
        }
        // DOM section: flip to multi WITHOUT the default-trio seeding —
        // the roster starts as [own venue] + the clicked one.
        let mut is_dom = false;
        let mut dom_flip: Option<(String, Vec<String>)> = None;
        if let Some(s) = self.panels_store.dom.get_mut(&pid) {
            is_dom = true;
            if !s.multi_mode {
                if s.symbol.eq_ignore_ascii_case(symbol)
                    && multi_dom::col_matches(
                        &multi_dom::col_entry(&s.exchange, &s.account_type),
                        exchange_id,
                        account_type,
                    )
                {
                    return true; // clicked the panel's own venue — no-op
                }
                if !s.multi.symbol.eq_ignore_ascii_case(&s.symbol) {
                    s.multi.symbol = s.symbol.clone();
                    for (b, a) in &mut s.multi.books_by_exchange {
                        b.clear();
                        a.clear();
                    }
                    s.multi.handles.clear();
                    s.multi.mid_price = 0.0;
                }
                if s.multi.config.exchanges.is_empty() && !s.exchange.is_empty() {
                    let entry = multi_dom::col_entry(&s.exchange, &s.account_type);
                    s.multi.add_exchange(entry);
                }
                s.multi_mode = true;
                dom_flip = Some((s.multi.symbol.clone(), s.multi.config.exchanges.clone()));
            }
        }
        if is_dom {
            if let Some((sym, entries)) = dom_flip {
                self.subscribe_multi_roster(&sym, &entries);
            }
            self.toggle_multi_dom_roster(pid, symbol, exchange_id, account_type);
            if let Some(edit) = self.symbol_quick_edit.as_mut() {
                edit.target = crate::SymbolQuickEditTarget::MultiDomRoster(pid);
            }
            return true;
        }
        // Tape sections: `set_multi_mode` seeds the roster with the
        // panel's own venue; the toggle adds the clicked one.
        // (was_single, roster symbol, roster entries, book-feed panel).
        let flipped: Option<(bool, String, Vec<String>, bool)> = {
            let ps = &mut self.panels_store;
            if let Some(s) = ps.trade_tape.get_mut(&pid) {
                let own = s.symbol.eq_ignore_ascii_case(symbol)
                    && multi_dom::col_matches(
                        &multi_dom::col_entry(&s.exchange, &s.account_type),
                        exchange_id, account_type);
                let was_single = !s.multi_mode;
                if was_single && own { return true; }
                s.set_multi_mode(true);
                Some((was_single, s.multi.symbol.clone(), s.multi.entries.clone(), false))
            } else if let Some(s) = ps.l2_tape.get_mut(&pid) {
                let own = s.symbol.eq_ignore_ascii_case(symbol)
                    && multi_dom::col_matches(
                        &multi_dom::col_entry(&s.exchange, &s.account_type),
                        exchange_id, account_type);
                let was_single = !s.multi_mode;
                if was_single && own { return true; }
                s.set_multi_mode(true);
                Some((was_single, s.multi.symbol.clone(), s.multi.entries.clone(), true))
            } else if let Some(s) = ps.big_trades.get_mut(&pid) {
                let own = s.symbol.eq_ignore_ascii_case(symbol)
                    && multi_dom::col_matches(
                        &multi_dom::col_entry(&s.exchange, &s.account_type),
                        exchange_id, account_type);
                let was_single = !s.multi_mode;
                if was_single && own { return true; }
                s.set_multi_mode(true);
                Some((was_single, s.multi.symbol.clone(), s.multi.entries.clone(), false))
            } else {
                None
            }
        };
        let Some((was_single, sym, entries, book)) = flipped else {
            return false;
        };
        if was_single {
            self.subscribe_multi_tape_roster(&sym, &entries, book);
        }
        self.toggle_multi_tape_roster(pid, symbol, exchange_id, account_type);
        if let Some(edit) = self.symbol_quick_edit.as_mut() {
            edit.target = crate::SymbolQuickEditTarget::MultiTapeRoster(pid);
        }
        true
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

        // Multi-tape roster handles (M/TT · M/L2 · M/BT, owner 2026-07-31):
        // venues holding `None` get their subscribe retried — the same
        // self-heal contract as the singles above. `subscribe_*` is
        // idempotent; `ensure_connector` dials a missing venue connector
        // once (no reseed/backoff machinery needed — tapes stream, they
        // have no REST snapshot to storm).
        {
            use mlc_app_panels::trading::order_flow::multi_dom;
            // Late symbol sync: a panel hydrated from a BoundToChart /
            // HyperFocus source has an EMPTY symbol at restore time (the
            // real key lands via the group rebind later) — the roster's
            // base symbol restored empty with it and the venue scan below
            // would skip the panel forever. Adopt the panel's key once it
            // materializes.
            for s in self.panels_store.trade_tape.values_mut() {
                if s.multi_mode && s.multi.symbol.is_empty() && !s.symbol.is_empty() {
                    s.multi.symbol = s.symbol.clone();
                }
            }
            for s in self.panels_store.l2_tape.values_mut() {
                if s.multi_mode && s.multi.symbol.is_empty() && !s.symbol.is_empty() {
                    s.multi.symbol = s.symbol.clone();
                }
            }
            for s in self.panels_store.big_trades.values_mut() {
                if s.multi_mode && s.multi.symbol.is_empty() && !s.symbol.is_empty() {
                    s.multi.symbol = s.symbol.clone();
                }
            }
            // LIQ tape is all-venue by construction — late sync seeds the
            // whole Futures roster once the symbol materializes.
            for s in self.panels_store.liq_tape.values_mut() {
                if s.multi.symbol.is_empty() && !s.symbol.is_empty() {
                    let sym = s.symbol.clone();
                    s.ensure_roster(&sym);
                }
            }
            enum TapeKind { Tt, L2, Bt, Liq }
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let mut tape_needs: Vec<(TapeKind, mlc_core::state::PanelId, usize, String, String)> =
                Vec::new();
            // Reseed bookkeeping, applied mutably after the scans:
            // (kind, pid, reseeding-now, all-venues-alive).
            let mut tape_marks: Vec<(TapeKind, mlc_core::state::PanelId, bool, bool)> = Vec::new();
            // Dead-venue detection (the Multi-DOM empty-column disease):
            // the handle landed but the venue NEVER delivered — subscribe
            // raced the venue connector dial and silently armed nothing.
            // Re-subscribe with an escalating 5s→60s throttle. TT/BT
            // signal: per-venue cursor still 0; L2 signal: no tracker
            // update-id ever seen.
            for (pid, s) in &self.panels_store.trade_tape {
                if s.multi_mode && !s.multi.symbol.is_empty() {
                    let throttle_ok =
                        now_ms - s.multi.last_reseed_ms >= s.multi.reseed_backoff_ms.max(5_000);
                    let mut any_dead = false;
                    for (i, e) in s.multi.entries.iter().enumerate() {
                        let missing = s.multi.trade_handles.get(i).map_or(true, |h| h.is_none());
                        let dead =
                            !missing && s.multi.trade_cursors.get(i).map_or(false, |c| *c == 0);
                        if dead {
                            any_dead = true;
                        }
                        if missing || (dead && throttle_ok) {
                            tape_needs.push((TapeKind::Tt, *pid, i, e.clone(), s.multi.symbol.clone()));
                        }
                    }
                    tape_marks.push((TapeKind::Tt, *pid, any_dead && throttle_ok, !any_dead));
                }
            }
            for (pid, s) in &self.panels_store.big_trades {
                if s.multi_mode && !s.multi.symbol.is_empty() {
                    let throttle_ok =
                        now_ms - s.multi.last_reseed_ms >= s.multi.reseed_backoff_ms.max(5_000);
                    let mut any_dead = false;
                    for (i, e) in s.multi.entries.iter().enumerate() {
                        let missing = s.multi.trade_handles.get(i).map_or(true, |h| h.is_none());
                        let dead =
                            !missing && s.multi.trade_cursors.get(i).map_or(false, |c| *c == 0);
                        if dead {
                            any_dead = true;
                        }
                        if missing || (dead && throttle_ok) {
                            tape_needs.push((TapeKind::Bt, *pid, i, e.clone(), s.multi.symbol.clone()));
                        }
                    }
                    tape_marks.push((TapeKind::Bt, *pid, any_dead && throttle_ok, !any_dead));
                }
            }
            for (pid, s) in &self.panels_store.l2_tape {
                if s.multi_mode && !s.multi.symbol.is_empty() {
                    let throttle_ok =
                        now_ms - s.multi.last_reseed_ms >= s.multi.reseed_backoff_ms.max(5_000);
                    let mut any_dead = false;
                    for (i, e) in s.multi.entries.iter().enumerate() {
                        let missing = s.multi.book_handles.get(i).map_or(true, |h| h.is_none());
                        let dead =
                            !missing && s.multi.book_uids.get(i).map_or(false, |u| u.is_none());
                        if dead {
                            any_dead = true;
                        }
                        if missing || (dead && throttle_ok) {
                            tape_needs.push((TapeKind::L2, *pid, i, e.clone(), s.multi.symbol.clone()));
                        }
                    }
                    tape_marks.push((TapeKind::L2, *pid, any_dead && throttle_ok, !any_dead));
                }
            }
            // LIQ: missing-handle refill ONLY — no dead-venue detection
            // (liquidations are sparse; a quiet venue is indistinguishable
            // from a dead feed and a cursor-based reseed would storm).
            // The refill itself IS backoff-throttled though: wire-absent
            // venues (Bitget V2 has no public liq channel) fail their
            // subscribe every time, and an unthrottled per-frame retry is
            // a resubscribe storm of its own.
            for (pid, s) in &self.panels_store.liq_tape {
                if !s.multi.symbol.is_empty() {
                    let throttle_ok =
                        now_ms - s.multi.last_reseed_ms >= s.multi.reseed_backoff_ms.max(2_000);
                    let mut any_missing = false;
                    for (i, e) in s.multi.entries.iter().enumerate() {
                        let missing = s.multi.liq_handles.get(i).map_or(true, |h| h.is_none());
                        if missing {
                            any_missing = true;
                        }
                        if missing && throttle_ok {
                            tape_needs.push((TapeKind::Liq, *pid, i, e.clone(), s.multi.symbol.clone()));
                        }
                    }
                    tape_marks.push((TapeKind::Liq, *pid, any_missing && throttle_ok, !any_missing));
                }
            }
            for (kind, pid, reseeding, all_alive) in &tape_marks {
                let m = match kind {
                    TapeKind::Tt => self.panels_store.trade_tape.get_mut(pid).map(|s| &mut s.multi),
                    TapeKind::Bt => self.panels_store.big_trades.get_mut(pid).map(|s| &mut s.multi),
                    TapeKind::L2 => self.panels_store.l2_tape.get_mut(pid).map(|s| &mut s.multi),
                    TapeKind::Liq => self.panels_store.liq_tape.get_mut(pid).map(|s| &mut s.multi),
                };
                let Some(m) = m else { continue };
                if *reseeding {
                    m.last_reseed_ms = now_ms;
                    m.reseed_backoff_ms = (m.reseed_backoff_ms.max(5_000) * 2).min(60_000);
                    // Backoff-throttled by construction (one line per
                    // reseed window, 5s→60s).
                    crate::sglog!(
                        "[MT repair] {:?} dead venues resubscribed, next in {}ms",
                        pid, m.reseed_backoff_ms
                    );
                } else if *all_alive && m.reseed_backoff_ms != 0 {
                    m.reseed_backoff_ms = 0;
                }
            }
            for (kind, pid, i, entry, sym) in tape_needs {
                let ex_slug = multi_dom::col_exchange(&entry).to_string();
                let at_label = multi_dom::col_at(&entry).to_string();
                let Some(eid) = digdigdig3::ExchangeId::from_str(&ex_slug) else { continue };
                self.core.ensure_connector(eid);
                let at = crate::account_type_from_label(&at_label);
                match kind {
                    TapeKind::Tt => {
                        let h = self.core.subscribe_trades(eid, &sym, at);
                        if let Some(s) = self.panels_store.trade_tape.get_mut(&pid) {
                            s.multi.sync_len();
                            if i < s.multi.trade_handles.len() {
                                s.multi.trade_handles[i] = h;
                            }
                        }
                    }
                    TapeKind::Bt => {
                        let h = self.core.subscribe_trades(eid, &sym, at);
                        if let Some(s) = self.panels_store.big_trades.get_mut(&pid) {
                            s.multi.sync_len();
                            if i < s.multi.trade_handles.len() {
                                s.multi.trade_handles[i] = h;
                            }
                        }
                    }
                    TapeKind::L2 => {
                        let h = self.core.subscribe_orderbook(eid, &sym, at);
                        if let Some(s) = self.panels_store.l2_tape.get_mut(&pid) {
                            s.multi.sync_len();
                            if i < s.multi.book_handles.len() {
                                s.multi.book_handles[i] = h;
                            }
                        }
                    }
                    TapeKind::Liq => {
                        let h = self.core.subscribe_liquidations(eid, &sym, at);
                        if let Some(s) = self.panels_store.liq_tape.get_mut(&pid) {
                            s.multi.sync_len();
                            if i < s.multi.liq_handles.len() {
                                s.multi.liq_handles[i] = h;
                            }
                        }
                    }
                }
            }
        }

        // Multi-DOM book handles (v2: per-exchange station trackers, no
        // delta events). Provisioned only while multi mode is ON; cleared
        // on symbol rebind / section reopen so this refills them fresh.
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let mut multi_needs: Vec<(mlc_core::state::PanelId, usize, String, String)> = Vec::new();
        // (pid, any column was UNHEALTHY — drives the reseed backoff —
        //  and the human-readable per-column reasons for the log line).
        let mut reseed_pids: Vec<(mlc_core::state::PanelId, bool, String)> = Vec::new();
        // Deferred `col_revisions` bookkeeping (the scan borrows immutably).
        let mut rev_updates: Vec<(mlc_core::state::PanelId, usize, u64, i64)> = Vec::new();
        // Panels whose columns are ALL healthy — reset their backoff.
        let mut healthy_pids: Vec<mlc_core::state::PanelId> = Vec::new();
        for (pid, s) in &self.panels_store.dom {
            if !s.multi_mode || s.multi.symbol.is_empty() {
                continue;
            }
            // Unhealthy-column reseeds escalate: 5s → 10s → ... → 60s cap
            // (reset once every column is healthy). A column that CANNOT be
            // fixed (banned venue, absent symbol) must not turn this loop
            // into a REST/dial storm — that storm is what kept the exchange
            // connect-rate ban alive in the 2026-07-30 incident.
            let throttle = s.multi.reseed_backoff_ms.max(5_000);
            let throttle_ok = now_ms - s.multi.last_reseed_ms >= throttle;
            let mut any_unhealthy = false;
            let mut panel_reseed = false;
            let mut reasons: Vec<String> = Vec::new();
            for (i, ex) in s.multi.config.exchanges.iter().enumerate() {
                let missing = s.multi.handles.get(i).map_or(true, |h| h.is_none());
                // Tracker probe via try_read — NEVER blocking_read here: a
                // wedged writer must cost us one skipped repair pass, not a
                // parked UI thread.
                let probe = s.multi.handles.get(i)
                    .and_then(|h| h.as_ref())
                    .and_then(|h| h.try_read().ok().map(|g| (g.revision(), g.bbo())));
                // Freeze detector (owner order 2026-07-30 «нет механизма как
                // на клайнах»): the tracker's apply-revision not moving for
                // 15s while the panel runs means the feed behind the column
                // died — books look plausible but are dead. Treated like a
                // stale column: REST reseed (below) repaints, and if the
                // station forwarder exited its gate reopens on resubscribe.
                let (last_rev, last_change) = s.multi.col_revisions.get(i)
                    .copied().unwrap_or((0, 0));
                let mut frozen = false;
                if let Some((rev, _)) = probe {
                    if rev != last_rev || last_change == 0 {
                        rev_updates.push((*pid, i, rev, now_ms));
                    } else if now_ms - last_change >= 15_000 {
                        frozen = true;
                    }
                }
                // A live handle whose bucketed book stays EMPTY means the
                // tracker never got its REST snapshot (client wasn't dialed
                // at subscribe time; deltas alone build up slowly) — re-seed.
                let stale_empty = !missing
                    && s.multi.books_by_exchange.get(i)
                        .map_or(true, |(b, a)| b.is_empty() && a.is_empty())
                    && throttle_ok;
                // A crossed bbo (best bid >= best ask) marks a ghost book —
                // dropped removes left stale levels across the spread. A fresh
                // REST snapshot heals the covered range via the tracker's
                // windowed merge.
                let crossed = !missing
                    && throttle_ok
                    && probe.map_or(false, |(_, bbo)| bbo.map_or(false, |(b, a)| b >= a));
                let frozen = frozen && !missing && throttle_ok;
                // Periodic REST resync (30s): on wasm the station relay can
                // drop delta events under main-thread pressure (Lagged trap);
                // lost inserts/removes accumulate into voids/ghosts around
                // mid. A fresh snapshot heals the covered band via the
                // tracker's windowed merge — standard book-maintenance
                // practice, ~1 REST call per venue per 30s.
                let periodic = !missing && now_ms - s.multi.last_reseed_ms >= 30_000;
                if stale_empty || crossed || frozen {
                    any_unhealthy = true;
                    let why = if stale_empty { "empty" } else if crossed { "crossed" } else { "frozen" };
                    reasons.push(format!("{}:{}", ex, why));
                }
                if missing || stale_empty || crossed || frozen || periodic {
                    multi_needs.push((*pid, i, ex.clone(), s.multi.symbol.clone()));
                    if stale_empty || crossed || frozen || periodic {
                        panel_reseed = true;
                    }
                }
            }
            if panel_reseed {
                reseed_pids.push((*pid, any_unhealthy, reasons.join(" ")));
            }
            if !any_unhealthy {
                healthy_pids.push(*pid);
            }
        }
        for (pid, i, rev, ts) in rev_updates {
            if let Some(s) = self.panels_store.dom.get_mut(&pid) {
                if s.multi.col_revisions.len() <= i {
                    s.multi.col_revisions.resize(i + 1, (0, 0));
                }
                s.multi.col_revisions[i] = (rev, ts);
            }
        }
        for pid in healthy_pids {
            if let Some(s) = self.panels_store.dom.get_mut(&pid) {
                s.multi.reseed_backoff_ms = 5_000;
                // Log the unhealthy→healthy TRANSITION only (never steady
                // state — repeated состояние is noise that buries defects).
                if s.multi.was_unhealthy {
                    s.multi.was_unhealthy = false;
                    crate::sglog!("[MD repair] {:?} all columns healthy", pid);
                }
            }
        }
        for (pid, unhealthy, reasons) in reseed_pids {
            if let Some(s) = self.panels_store.dom.get_mut(&pid) {
                s.multi.last_reseed_ms = now_ms;
                if unhealthy {
                    s.multi.reseed_backoff_ms = (s.multi.reseed_backoff_ms.max(5_000) * 2).min(60_000);
                    // Backoff-throttled by construction (one line per reseed
                    // window, 5s→60s), states WHICH columns and WHY.
                    crate::sglog!(
                        "[MD repair] {:?} reseed [{}] next in {}ms",
                        pid, reasons, s.multi.reseed_backoff_ms
                    );
                    s.multi.was_unhealthy = true;
                }
            }
        }
        for (pid, i, entry, sym) in multi_needs {
            use mlc_app_panels::trading::order_flow::multi_dom;
            let ex_slug = multi_dom::col_exchange(&entry).to_string();
            let at_label = multi_dom::col_at(&entry).to_string();
            let Some(eid) = digdigdig3::ExchangeId::from_str(&ex_slug) else { continue };
            // The roster exchange may have NO data connection yet (only the
            // chart's exchange is dialed by default) — ensure the connector
            // exists, else subscribe_depth's REST seed silently no-ops and
            // the column stays blank (owner defect 2026-07-30).
            self.core.ensure_connector(eid);
            let at = crate::account_type_from_label(&at_label);
            let handle = self.core.subscribe_orderbook(eid, &sym, at);
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
