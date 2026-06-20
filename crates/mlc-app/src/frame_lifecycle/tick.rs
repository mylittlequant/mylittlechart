//! Live update tick loop: drives `BackendCore::tick_writers` to drain the
//! broadcast channel, then runs per-app reactions on the emitted `BubbleEvent`s.

use crate::ChartApp;
use crate::{account_type_from_label, parse_timeframe_name};
use mlc_backend_core::BubbleEvent;
use mlc_live_data::LiveUpdate;
use mlc_core::{ChartId, ScaleMode, timestamp_ms_to_bar_f64};
use mlc_indicators::RecalcMode;

impl ChartApp {
    /// Called every frame with the current wall-clock time in milliseconds.
    ///
    /// Drives `BackendCore::tick_writers` to drain the broadcast channel and
    /// write incoming data into the shared bubble store (bars, derived series,
    /// mini-ticker, exchange-symbols). Returns one `BubbleEvent` per write so
    /// per-app reactions can run viewport/indicator/panel side-effects.
    pub fn tick(&mut self, current_time_ms: u64) {
        let _ = current_time_ms;
        let tick_start = std::time::Instant::now();

        // Connector warm-up: dial all enabled connectors once (bridge-guarded).
        // Fires on the first tick of the skeleton/loading window (concurrent, off
        // the UI thread); the bridge's one-shot flag makes every later call —
        // including the promoted live window's — a no-op. Cheap to call every tick.
        self.warmup_connectors();

        // Load chat history for any restored chat leaves on the very first tick.
        // PTY sessions spawn-on-demand only (user must click [Start]).
        if !self.agent_autostarted {
            self.agent_autostarted = true;
            // Load chat history per leaf — use the persisted session_id when available
            // so each leaf resumes its own conversation, not just the latest one.
            let chat_leaves: Vec<(gate4agent::InstanceId, Option<String>)> = self.sidebar_state.agent_leaves
                .values()
                .filter(|d| d.mode == gate4agent::InstanceMode::Chat)
                .map(|d| (d.instance_id, d.chat_session_id.clone()))
                .collect();
            for (id, session_id) in chat_leaves {
                if let Some(ref sid) = session_id {
                    self.agent.load_history_instance(id, sid);
                } else {
                    self.agent.load_latest_history_instance(id);
                }
            }
        }

        // Reset per-tick accumulators for profiling.
        self.last_auto_scale_us = 0;
        self.last_indicator_recalc_us = 0;
        self.last_orderbook_panel_us = 0;
        self.last_dom_panel_us = 0;
        self.last_l2_panel_us = 0;
        self.last_heatmap_panel_us = 0;
        self.last_trade_panel_us = 0;
        self.last_bar_apply_us = 0;
        self.last_ob_event_count = 0;
        self.last_trade_event_count = 0;

        // ── Live data: drive backend writers ─────────────────────────────────
        // BackendCore::tick_writers drains the broadcast channel and writes
        // incoming data into the shared bubble store (bars, derived series,
        // mini-ticker, exchange-symbols). It returns one BubbleEvent per write
        // so per-app reactors can run viewport/indicator/panel side-effects.
        let mut had_trade_update = false;
        // had_ticker_update — same idea but for MiniTickerUpdated. Watchlist /
        // sidebar prices come straight out of `core.mini_ticker` (DashMap) at
        // render time; without this flag the sidebar render is skipped until
        // some other input triggers a redraw (a click, an alert, an agent
        // command). Owner regression 2026-06-20: "watchlist values don't
        // refresh unless I click the chart".
        let mut had_ticker_update = false;
        let mut _drain_count = 0u32;
        // Per-tick drain census by event type — printed only when this tick
        // explodes (see the spike log at the end of tick), so we can see WHAT
        // flooded the queue this frame (trade burst? backfill? reconnect?).
        let (mut n_bars, mut n_trade, mut n_ticker, mut n_connector, mut n_scroll, mut n_backfill, mut n_other) =
            (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
        // Startup-freeze diagnostics: count SymbolsLoaded events this tick, total
        // symbols moved, and microseconds spent in the exchange_symbols.insert
        // path. The startup symbol flood (thousands of symbols × N exchanges) is
        // a freeze suspect — this attributes it precisely.
        let (mut n_symbols, mut n_symbols_total, mut symbols_insert_us) = (0u32, 0usize, 0u64);
        let mut trading_updates: Vec<LiveUpdate> = Vec::new();
        // Coalesce orderbook panel updates: collect per-(symbol,exchange,account_type)
        // delta events for L2-tape (which needs per-event timestamps) and force_rebuild
        // flags for DOM (which reads the tracker for full rebuilds).
        //
        // Event routing:
        //   OrderbookSnapshotRest → force_rebuild = true for DOM
        //   OrderbookDelta{bids,asks} → push ObDeltaEvent preserving per-event ts
        /// One WS delta message with its original wire timestamp.
        struct ObDeltaEvent {
            ts_ms: i64,
            bids: Vec<(f64, f64)>,
            asks: Vec<(f64, f64)>,
        }
        struct ObDeltaEntry {
            /// Per-event raw deltas in arrival order. Each entry preserves its own ts
            /// so L2-tape rows get per-event ts instead of a frame-batch ts.
            events: Vec<ObDeltaEvent>,
            /// True when a REST snapshot arrived this frame → DOM must full-rebuild.
            force_rebuild: bool,
        }
        let mut ob_dirty: std::collections::HashMap<(String, String, String), ObDeltaEntry> =
            std::collections::HashMap::new();
        // Coalesce last-trade-price → orderbook ghost-filter updates.  Calling
        // set_last_trade_price per trade is O(book): it sweeps ghost levels
        // (~1000) AND rebuilds the published view (full ~1000×2 clone) every time.
        // On a burst (172 trades/frame) that was the dominant ~21ms tick spike.
        // Only the LAST price per symbol matters for the ghost filter, so record
        // it here and apply once after the drain loop.
        let mut last_trade_price_by_symbol: std::collections::HashMap<(String, String, String), f64> =
            std::collections::HashMap::new();

        let events_start = std::time::Instant::now();

        let events = self.core.tick_writers(current_time_ms);
        _drain_count = events.len() as u32;

        // ── Per-event reactor loop ────────────────────────────────────────────
        for event in &events {
            // Census.
            match event {
                BubbleEvent::BarsLoaded { .. } => n_bars += 1,
                BubbleEvent::BackfillCompleted { .. } => n_backfill += 1,
                BubbleEvent::ScrollBarsAppended { .. } => n_scroll += 1,
                BubbleEvent::TradeUpdatePassthrough { .. } | BubbleEvent::BarUpdatePassthrough { .. } => {
                    n_trade += 1;
                }
                BubbleEvent::MiniTickerUpdated { .. } => n_ticker += 1,
                BubbleEvent::ConnectorReady { .. } => n_connector += 1,
                BubbleEvent::SymbolsLoaded { count, .. } => {
                    let t0 = std::time::Instant::now();
                    n_symbols += 1;
                    n_symbols_total += count;
                    symbols_insert_us += t0.elapsed().as_micros() as u64;
                }
                _ => n_other += 1,
            }

            // Inline accumulator updates for items consumed after the loop.
            match event {
                BubbleEvent::TradeUpdatePassthrough { exchange_id, account_type, symbol, price, .. } => {
                    had_trade_update = true;
                    last_trade_price_by_symbol.insert(
                        (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone()),
                        *price,
                    );
                }
                BubbleEvent::MiniTickerUpdated { exchange_id, symbol, account_type, last_price } => {
                    last_trade_price_by_symbol.insert(
                        (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone()),
                        *last_price,
                    );
                    had_ticker_update = true;
                }
                BubbleEvent::OrderbookSnapshotRest { exchange_id, symbol, account_type } => {
                    self.last_ob_event_count += 1;
                    let key = (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone());
                    let entry = ob_dirty.entry(key).or_insert_with(|| ObDeltaEntry {
                        events: Vec::new(),
                        force_rebuild: false,
                    });
                    entry.force_rebuild = true;
                }
                BubbleEvent::OrderbookDelta { exchange_id, symbol, account_type, bids, asks, ts_ms } => {
                    self.last_ob_event_count += 1;
                    let key = (symbol.clone(), exchange_id.as_str().to_string(), account_type.clone());
                    let entry = ob_dirty.entry(key).or_insert_with(|| ObDeltaEntry {
                        events: Vec::new(),
                        force_rebuild: false,
                    });
                    entry.events.push(ObDeltaEvent { ts_ms: *ts_ms, bids: bids.clone(), asks: asks.clone() });
                }
                BubbleEvent::OrderbookDeltaForMultiDom { exchange_id, symbol, bids, asks, .. } => {
                    let ex_label = exchange_id.as_str().to_string();
                    let sym_ref = symbol.as_str();
                    for state in self.panels_store.multi_dom.values_mut() {
                        if state.symbol != sym_ref { continue; }
                        if let Some(idx) = state.exchange_idx(&ex_label) {
                            let bids_v: Vec<(f64, f64)> = bids.iter().map(|(p, q)| (*p, *q)).collect();
                            let asks_v: Vec<(f64, f64)> = asks.iter().map(|(p, q)| (*p, *q)).collect();
                            state.apply_snapshot(idx, bids_v.into_iter(), asks_v.into_iter());
                            state.rebuild_merged();
                        }
                    }
                }
                BubbleEvent::TradingUpdate { update } => {
                    trading_updates.push(update.clone());
                }
                _ => {}
            }

            // Per-window / indicator / panel side-effects.
            self.react_to_bubble_event(event, current_time_ms);
        }

        // Keep census variables live so the spike / startup-wave logs at the end
        // of tick() can reference them without compiler warnings.
        let _ = last_trade_price_by_symbol;
        let _ = symbols_insert_us;
        let _ = n_scroll;
        let _ = ScaleMode::Auto; // silence import

        self.last_event_process_us = events_start.elapsed().as_micros() as u64;

        // ── Coalesced orderbook panel re-aggregation ─────────────────────────
        // DOM + heatmap: tick once per frame with the merged level changes (they
        // track current book state, not history — per-event ts is irrelevant).
        // L2 tape: iterate the per-event list so each tape row carries its own
        // original wire timestamp instead of a single SystemTime::now() for the
        // whole batch.
        if !ob_dirty.is_empty() {
            let ob_panel_start = std::time::Instant::now();
            for ((symbol, ex_str, at_str), entry) in &ob_dirty {
                // ── DOM: incremental or full-rebuild (bulk — level state only) ──
                let dom_t0 = std::time::Instant::now();
                let (bulk_bids, bulk_asks): (Vec<(f64, f64)>, Vec<(f64, f64)>) = {
                    let mut b: Vec<(f64, f64)> = Vec::new();
                    let mut a: Vec<(f64, f64)> = Vec::new();
                    for ev in &entry.events {
                        b.extend_from_slice(&ev.bids);
                        a.extend_from_slice(&ev.asks);
                    }
                    (b, a)
                };
                for state in self.panels_store.dom.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        state.apply_ob_update(
                            &bulk_bids,
                            &bulk_asks,
                            entry.force_rebuild,
                        );
                    }
                }
                self.last_dom_panel_us += dom_t0.elapsed().as_micros() as u64;

                // ── L2 Tape: per-event calls with original wire ts ─────────────
                let l2_t0 = std::time::Instant::now();
                for state in self.panels_store.l2_tape.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        if entry.force_rebuild || entry.events.is_empty() {
                            state.apply_ob_update(
                                &[],
                                &[],
                                entry.force_rebuild,
                            );
                        } else {
                            for ev in &entry.events {
                                state.apply_ob_update_with_ts(
                                    &ev.bids,
                                    &ev.asks,
                                    ev.ts_ms,
                                );
                            }
                        }
                        state.prune_flash();
                    }
                }
                self.last_l2_panel_us += l2_t0.elapsed().as_micros() as u64;

                // ── Heatmap: view-based tick (rate-limited, untouched path) ───
                let heat_t0 = std::time::Instant::now();
                for state in self.panels_store.liquidity_heatmap.values_mut() {
                    if &state.symbol == symbol && &state.exchange == ex_str && &state.account_type == at_str {
                        state.tick();
                    }
                }
                self.last_heatmap_panel_us += heat_t0.elapsed().as_micros() as u64;
            }
            self.last_orderbook_panel_us += ob_panel_start.elapsed().as_micros() as u64;
        }

        if !trading_updates.is_empty() {
            if let Some(mut tm) = self.core.trading_lock() {
                tm.tick(&trading_updates);
            }
            for state in self.panels_store.order_entry.values_mut() {
                state.sync_from_snapshot();
            }
            for state in self.panels_store.position_manager.values_mut() {
                state.sync_from_snapshot();
            }
            for state in self.panels_store.trade_log.values_mut() {
                state.sync_from_snapshot();
            }
        }

        // ── Watchlist refresh trigger ─────────────────────────────────────────
        // mini_ticker writes are passthrough at the BackendCore layer (DashMap,
        // lock-free) — the sidebar render path reads them on demand. But render
        // only rebuilds when sidebar_data_dirty=true, so without an explicit
        // flip here the watchlist freezes between user clicks. Pre-cutover the
        // tick.rs MiniTickerUpdate arm did the equivalent.
        //
        // Rate-limit because the tick can carry 20+ MiniTicker events per
        // frame (one per watchlist symbol at 1Hz, fanned in 16ms windows by
        // the broadcast channel). Mark once per ~250ms wall-clock so we still
        // get ~4 sidebar refreshes per second without rebuilding 20× per tick.
        if had_ticker_update {
            const MARK_INTERVAL_MS: u128 = 250;
            let now = std::time::Instant::now();
            let bump = match self.sidebar_ticker_mark {
                Some(prev) => now.duration_since(prev).as_millis() >= MARK_INTERVAL_MS,
                None => true,
            };
            if bump {
                self.sidebar_data_dirty = true;
                self.sidebar_ticker_mark = Some(now);
            }
        }

        // ── Alert checker: detect price crossings for every visible symbol ────
        // Skip entirely when no trade arrived this tick — nothing changed.
        if had_trade_update {
            // Collect one entry per unique (symbol, exchange, account_type) triple across all windows.
            let mut seen_pairs: std::collections::HashSet<(String, String, String)> = std::collections::HashSet::new();
            struct WindowAlertData {
                symbol: String,
                exchange: String,
                account_type: String,
                current_price: f64,
                current_bar: f64,
                drawing_points: Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)>,
            }
            let window_data: Vec<WindowAlertData> = self.panel_app.panel_grid.containers()
                .iter()
                .filter_map(|(cid, window)| {
                    let sym = self.symbol_for_chart(*cid).unwrap_or_default();
                    let exch = self.exchange_for_chart(*cid).unwrap_or_default();
                    let at = self.account_type_for_chart(*cid).unwrap_or_default();
                    let triple = (sym.clone(), exch.clone(), at.clone());
                    if seen_pairs.contains(&triple) {
                        return None;
                    }
                    seen_pairs.insert(triple);
                    let bubble_bars_alert: Vec<mlc_core::Bar> = self.with_bubble(window.current_bubble_id, |b| b.bars.clone()).unwrap_or_default();
                    let current_price = bubble_bars_alert.last().map(|b| b.close).unwrap_or(0.0);
                    let current_bar = bubble_bars_alert.len().saturating_sub(1) as f64;
                    let drawing_points: Vec<(u64, Vec<(f64, f64)>, mlc_alerts::DrawingExtendMode)> = {
                        let pid = window.chart_panel_id;
                        let guard = self.core.drawings();
                        guard.get(pid).map(|dm| dm.primitives().iter()
                            .map(|p| {
                                let pts_bar: Vec<(f64, f64)> = p.points()
                                    .into_iter()
                                    .map(|(ts_ms, price)| (timestamp_ms_to_bar_f64(&bubble_bars_alert, ts_ms), price))
                                    .collect();
                                (p.data().id, pts_bar, mlc_alerts::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                            })
                            .collect())
                        .unwrap_or_default()
                    };
                    Some(WindowAlertData {
                        symbol: sym,
                        exchange: exch,
                        account_type: at,
                        current_price,
                        current_bar,
                        drawing_points,
                    })
                })
                .collect();

            let indicator_values = Self::build_indicator_values_for_alerts(
                &self.core.bubble().alert,
                &self.core.indicators(),
            );

            let mut all_triggered_ids: Vec<u64> = Vec::new();
            for wd in &window_data {
                let triggered = self.core.bubble_mut().alert.check_crossings_dynamic(
                    wd.current_price,
                    wd.current_bar,
                    &wd.symbol,
                    &wd.exchange,
                    &wd.account_type,
                    &wd.drawing_points,
                    &indicator_values,
                );
                all_triggered_ids.extend(triggered);
            }

            // ── Signal alert checker ───────────────────────────────────────────────
            {
                use mlc_indicators::signals::signal::BarConfirmation;

                let signal_batch: Vec<(u64, usize, i8, u8, String)> = self.core.indicators().instances_iter()
                    .flat_map(|inst| {
                        let ind_id = inst.id;
                        inst.signals.iter().map(move |s| {
                            let conf_u8 = match s.confirmation {
                                BarConfirmation::Pending => 0u8,
                                BarConfirmation::Closed => 1u8,
                                BarConfirmation::WickOnly => 2u8,
                            };
                            (ind_id, s.bar_index, s.direction.as_i8(), conf_u8, s.kind.description().to_string())
                        })
                    })
                    .collect();

                for wd in &window_data {
                    let triggered = self.core.bubble_mut().alert.check_signal_alerts(
                        &wd.symbol,
                        &wd.exchange,
                        &wd.account_type,
                        &signal_batch,
                    );
                    all_triggered_ids.extend(triggered);
                }
            }

            // Deduplicate in case the same alert matched multiple windows.
            all_triggered_ids.sort_unstable();
            all_triggered_ids.dedup();
            let triggered_ids = all_triggered_ids;

            // Use the active window's price for delivery event messages.
            let current_price = self.panel_app.panel_grid.active_container()
                .map(|w| w.current_bubble_id)
                .and_then(|bid| self.with_bubble(bid, |b| b.bars.last().map(|bar| bar.close)).flatten())
                .unwrap_or(0.0);

            // Build delivery events for triggered alerts.
            if !triggered_ids.is_empty() {
                let symbol = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                for id in &triggered_ids {
                    if let Some(alert) = self.core.bubble().alert.get(*id) {
                        self.pending_delivery_events.push(mlc_alert_delivery::DeliveryEvent {
                            alert_name: alert.name.clone(),
                            symbol: symbol.clone(),
                            message: format!("{} {} @ {:.8}",
                                alert.source.display_name(),
                                alert.condition.display_name(),
                                current_price),
                            price: current_price,
                            timestamp: now,
                            screenshot: None,
                        });
                    }
                }

                self.pending_alert_screenshot = true;
                self.sidebar_data_dirty = true;
            }
        }

        // ── Deferred indicator recalculation flush (PerFrame / PerBar) ────────
        // For PerTick mode drain_pending_recalc returns an empty Vec — no-op.
        let pending = self.core.indicators_mut().drain_pending_recalc();
        if !pending.is_empty() {
            let deferred_start = std::time::Instant::now();
            for symbol in &pending {
                let matching_ids: Vec<u64> = self
                    .panel_app
                    .panel_grid
                    .containers()
                    .keys()
                    .filter(|cid| self.symbol_for_chart(**cid)
                        .map_or(false, |s| s == *symbol))
                    .map(|cid| cid.0)
                    .collect();

                for window_id in matching_ids {
                    let chart_id = ChartId(window_id);
                    let bid_opt = self.panel_app.panel_grid.containers().get(&chart_id).map(|w| w.current_bubble_id);
                    if let Some(bid) = bid_opt {
                        let bars_recalc: Vec<mlc_core::Bar> = self.with_bubble(bid, |b| b.bars.clone()).unwrap_or_default();
                        self.core.indicators_mut().calculate_for_window(window_id, &bars_recalc);
                    }
                }
                let _ = symbol; // calculate_for_window filters by window_id, not symbol
                self.recalc_count += 1;
            }
            self.last_indicator_recalc_us += deferred_start.elapsed().as_micros() as u64;
            self.sync_sub_panes_from_manager();
        }

        // ── Periodic RecalcMode diagnostic log (every 5 seconds) ─────────────
        if self.diagnostics_enabled
            && self.recalc_log_timer.elapsed() >= std::time::Duration::from_secs(5)
        {
            let mode = match self.core.indicators().recalc_mode {
                RecalcMode::PerTick => "PerTick",
                RecalcMode::PerFrame => "PerFrame",
                RecalcMode::PerBar => "PerBar",
            };
            crate::dlog!(
                "[ChartApp] RecalcMode={} | trades={} recalcs={} in 5s",
                mode, self.trade_count, self.recalc_count
            );
            self.trade_count = 0;
            self.recalc_count = 0;
            self.recalc_log_timer = std::time::Instant::now();
        }

        // ── Layer 3: trigger scroll-fetch when viewport approaches left edge ───
        {
            let mut scroll_requests: Vec<(mlc_core::state::ChartId, String, String, String, String)> = Vec::new();
            let max_loaded = self.panel_app.user_manager.profile.data_load.max_loaded_bars;

            let instrument_by_cid_scroll: std::collections::HashMap<mlc_core::state::ChartId, (String, mlc_core::state::Timeframe, String, String)> = {
                self.panel_app.panel_grid.containers().keys()
                    .filter_map(|cid| {
                        let sym = self.symbol_for_chart(*cid)?;
                        let tf = self.timeframe_for_chart(*cid)?;
                        let exch = self.exchange_for_chart(*cid)?;
                        let at = self.account_type_for_chart(*cid)?;
                        Some((*cid, (sym, tf, exch, at)))
                    })
                    .collect()
            };

            let bar_counts_by_chart: std::collections::HashMap<mlc_core::state::ChartId, usize> =
                self.panel_app.panel_grid.containers().iter()
                    .map(|(cid, w)| {
                        let n = self.core.bubble().bubble.bubble(w.current_bubble_id)
                            .map(|b| b.bars.len())
                            .unwrap_or(0);
                        (*cid, n)
                    })
                    .collect();

            for (cid_scroll, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                if window.scroll_fetch_in_flight {
                    if let Some(started) = window.scroll_fetch_started {
                        if started.elapsed() > std::time::Duration::from_secs(10) {
                            crate::dlog!("[ChartApp] scroll_fetch_in_flight timeout, resetting for chart {:?}", cid_scroll);
                            window.scroll_fetch_in_flight = false;
                            window.scroll_fetch_started = None;
                        }
                    }
                    if window.scroll_fetch_in_flight { continue; }
                }
                let bar_count_scroll = bar_counts_by_chart.get(cid_scroll).copied().unwrap_or(0);
                if bar_count_scroll == 0 { continue; }
                if window.pending_symbol_load { continue; }

                let visible = window.viewport.visible_bars() as f64;
                let threshold = (visible * 0.20).max(5.0);
                if window.viewport.view_start > threshold { continue; }

                if max_loaded > 0 && bar_count_scroll >= max_loaded as usize { continue; }

                let (win_sym_sc, win_tf_sc, win_exch_sc, win_at_sc) = match instrument_by_cid_scroll.get(cid_scroll) {
                    Some(t) => t,
                    None => continue,
                };

                window.scroll_fetch_in_flight = true;
                window.scroll_fetch_started = Some(std::time::Instant::now());
                scroll_requests.push((
                    *cid_scroll,
                    win_sym_sc.clone(),
                    win_exch_sc.clone(),
                    win_tf_sc.name.clone(),
                    win_at_sc.clone(),
                ));
            }

            for (cid_req, symbol, exchange, tf_name, at_label) in scroll_requests {
                let oldest_ts = self.panel_app.panel_grid.containers()
                    .get(&cid_req)
                    .and_then(|_w| None::<&mlc_core::Bar>.map(|b| b.timestamp))
                    .unwrap_or(0);

                if oldest_ts == 0 { continue; }

                let eid = digdigdig3::ExchangeId::from_str(&exchange)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let at = account_type_from_label(&at_label);

                if let Some(tf) = parse_timeframe_name(&tf_name) {
                    self.core.bridge.request_scroll_bars(eid, &symbol, &tf, at, oldest_ts, 500);
                }
            }
        }

        if self.agent.drain_events() {
            self.sidebar_data_dirty = true;
        }

        // Sync pipe_session_id from gate4agent into sidebar descriptors for persistence.
        {
            let updates: Vec<(uzor::panels::LeafId, Option<String>)> = self
                .sidebar_state
                .agent_leaves
                .iter()
                .filter(|(_, desc)| desc.mode == gate4agent::InstanceMode::Chat)
                .filter_map(|(&leaf_id, desc)| {
                    self.agent
                        .snapshot_instance(desc.instance_id)
                        .and_then(|snap| {
                            if snap.pipe_session_id != desc.chat_session_id {
                                Some((leaf_id, snap.pipe_session_id))
                            } else {
                                None
                            }
                        })
                })
                .collect();
            for (leaf_id, session_id) in updates {
                if let Some(desc) = self.sidebar_state.agent_leaves.get_mut(&leaf_id) {
                    desc.chat_session_id = session_id;
                }
                self.profile_dirty = true;
            }
        }

        self.flush_pending_autosave();

        self.last_tick_us = tick_start.elapsed().as_micros() as u64;

        // broadcast_rx is now owned by BackendCore — direct len() not accessible.
        self.diag_queue_len = 0;
        self.diag_mini_ticker = self.core.mini_ticker.len();

        // ── Tick spike log ───────────────────────────────────────────────────
        if self.last_tick_us > 3000 && std::env::var("MLC_PERF_LOG").is_ok() {
            eprintln!(
                "[TICK-SPIKE] tick={}us events={}us recalc={}us autoscale={}us obpanel={}us(n={}) dom={}us l2={}us heat={}us trpanel={}us(n={}) barapply={}us | drained={} (bars={} backfill={} scroll={} trade={} ticker={} conn={} other={}) lag_events={}",
                self.last_tick_us,
                self.last_event_process_us,
                self.last_indicator_recalc_us,
                self.last_auto_scale_us,
                self.last_orderbook_panel_us,
                self.last_ob_event_count,
                self.last_dom_panel_us,
                self.last_l2_panel_us,
                self.last_heatmap_panel_us,
                self.last_trade_panel_us,
                self.last_trade_event_count,
                self.last_bar_apply_us,
                _drain_count, n_bars, n_backfill, n_scroll, n_trade, n_ticker, n_connector, n_other,
                self.lag_event_count,
            );
        }

        // ── Startup-wave log ─────────────────────────────────────────────────
        if (n_connector > 0 || n_symbols > 0) && std::env::var("MLC_PERF_LOG").is_ok() {
            eprintln!(
                "[STARTUP-WAVE] conn_ready={} symbols_loaded={}(total_syms={} insert={}us) tick={}us events={}us drained={} | skeleton_window={}",
                n_connector, n_symbols, n_symbols_total, symbols_insert_us,
                self.last_tick_us, self.last_event_process_us, _drain_count,
                self.panel_app.panel_grid.containers().is_empty(),
            );
        }

        // ── Indicator heal ───────────────────────────────────────────────────
        let any_unhealed = {
            let attempted = &self.indicator_heal_attempted;
            let im = self.core.indicators();
            self.panel_app.panel_grid.iter_containers().any(|(_, w)| {
                im.get_instances_for_window(w.id.0)
                    .iter()
                    .any(|i| i.values.is_empty() && !attempted.contains(&i.id))
            })
        };
        if any_unhealed {
            let window_ids: Vec<(u64, mlc_core::state::ChartId)> = self
                .panel_app
                .panel_grid
                .iter_containers()
                .map(|(_, w)| (w.id.0, w.id))
                .collect();
            for (wid, cid) in window_ids {
                let needs_heal: Vec<u64> = self.core.indicators().get_instances_for_window(wid)
                    .iter()
                    .filter(|i| i.values.is_empty() && !self.indicator_heal_attempted.contains(&i.id))
                    .map(|i| i.id)
                    .collect();
                if !needs_heal.is_empty() {
                    let bars = self.bars_for_chart(cid);
                    for id in &needs_heal {
                        self.indicator_heal_attempted.insert(*id);
                    }
                    eprintln!(
                        "[ChartApp] indicator heal: window {} has {} bars, {} never-computed instance(s) {:?} — recalculating",
                        wid, bars.len(), needs_heal.len(), needs_heal
                    );
                    self.core.indicators_mut().calculate_for_window(wid, &bars);
                }
            }
        }
    }
}
